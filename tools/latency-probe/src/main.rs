// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Measures input-to-display latency of a drawing app from outside the app.
//!
//! The probe holds the primary mouse button in the target window and extends a
//! spiral stroke one step at a time with `SendInput`, while PresentMon records
//! the app's presents and when each reached the screen. The first present
//! after a step is that step's response, so the target must not present while
//! idle; presents beyond the first per step are reported to check this.
//! Neither app is instrumented, and no screen capture runs, because capture
//! changes how Windows presents the app. PresentMon needs administrator rights.

use std::collections::BTreeMap;
use std::f64::consts::TAU;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
    MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT, SendInput, VIRTUAL_KEY, VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GA_ROOT, GetAncestor, GetClientRect, GetForegroundWindow, GetSystemMetrics,
    GetWindowThreadProcessId, IsWindowVisible, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_MAXIMIZE, SW_RESTORE, SWP_NOACTIVATE, SWP_NOZORDER,
    SetForegroundWindow, SetWindowPos, ShowWindow, WindowFromPoint,
};
use windows::core::BOOL;

const SESSION: &str = "UguruguLatencyProbe";

const USAGE: &str = "usage: latency-probe --exe <app.exe> --presentmon <PresentMon.exe> \
                     [--steps N] [--warmup N] [--radius FRACTION] [--window WxH | --window max] [--key LETTER] [--csv PATH]";

struct Options {
    exe: PathBuf,
    presentmon: PathBuf,
    steps: usize,
    /// Steps drawn first and left out of the results, so that the display
    /// settles after the window was placed.
    warmup: usize,
    radius: f64,
    /// Outer window size in physical pixels, the same for every app measured;
    /// `None` maximizes the window, where Windows 11 draws no rounded corners.
    window: Option<[i32; 2]>,
    /// A key pressed once before measuring, such as the C++ app's playback toggle.
    key: Option<u8>,
    csv: PathBuf,
}

fn parse() -> Option<Options> {
    let mut exe = None;
    let mut presentmon = None;
    let mut steps = 200;
    let mut warmup = 0;
    let mut radius = 0.12;
    let mut window = Some([2560, 1600]);
    let mut key = None;
    let mut csv = std::env::temp_dir().join("latency-probe-presentmon.csv");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args.next()?;
        match arg.as_str() {
            "--exe" => exe = Some(PathBuf::from(value)),
            "--presentmon" => presentmon = Some(PathBuf::from(value)),
            "--steps" => steps = value.parse().ok()?,
            "--warmup" => warmup = value.parse().ok()?,
            "--radius" => radius = value.parse().ok()?,
            "--window" if value == "max" => window = None,
            "--window" => {
                let (width, height) = value.split_once('x')?;
                window = Some([width.parse().ok()?, height.parse().ok()?]);
            }
            "--key" => {
                let letter = value.chars().next()?.to_ascii_uppercase();
                key = letter.is_ascii_alphanumeric().then_some(letter as u8);
            }
            "--csv" => csv = PathBuf::from(value),
            _ => return None,
        }
    }
    Some(Options {
        exe: exe?,
        presentmon: presentmon?,
        steps,
        warmup,
        radius,
        window,
        key,
        csv,
    })
}

fn main() -> ExitCode {
    let Some(options) = parse() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match run(&options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("latency-probe: {error}");
            ExitCode::FAILURE
        }
    }
}

fn qpc() -> i64 {
    let mut value = 0;
    // SAFETY: valid out pointer.
    let _ = unsafe { QueryPerformanceCounter(&mut value) };
    value
}

fn qpc_ticks_per_ms() -> f64 {
    let mut value = 0;
    // SAFETY: valid out pointer.
    let _ = unsafe { QueryPerformanceFrequency(&mut value) };
    value as f64 / 1000.0
}

fn find_window(pid: u32) -> Option<HWND> {
    struct Search {
        pid: u32,
        found: Option<HWND>,
        area: i64,
    }
    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
        // SAFETY: `data` points at the `Search` below for the whole enumeration.
        let search = unsafe { &mut *(data.0 as *mut Search) };
        let mut pid = 0;
        // SAFETY: valid window from the enumeration and out pointer.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        // SAFETY: valid window from the enumeration.
        if pid == search.pid && unsafe { IsWindowVisible(hwnd) }.as_bool() {
            let mut rect = RECT::default();
            // SAFETY: valid window and out pointer.
            if unsafe { GetClientRect(hwnd, &mut rect) }.is_ok() {
                let area = i64::from(rect.right) * i64::from(rect.bottom);
                if area > search.area {
                    search.area = area;
                    search.found = Some(hwnd);
                }
            }
        }
        BOOL(1)
    }
    let mut search = Search {
        pid,
        found: None,
        area: 0,
    };
    // SAFETY: the callback only uses `search`, which outlives the call.
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize)) };
    search.found
}

fn send_mouse(flags: MOUSE_EVENT_FLAGS, position: Option<[i32; 2]>) {
    let (dx, dy, flags) = match position {
        Some([x, y]) => {
            // SAFETY: plain metric queries.
            let (left, top, width, height) = unsafe {
                (
                    GetSystemMetrics(SM_XVIRTUALSCREEN),
                    GetSystemMetrics(SM_YVIRTUALSCREEN),
                    GetSystemMetrics(SM_CXVIRTUALSCREEN),
                    GetSystemMetrics(SM_CYVIRTUALSCREEN),
                )
            };
            let normalise = |value: i32, origin: i32, extent: i32| {
                ((f64::from(value - origin) + 0.5) * 65536.0 / f64::from(extent)) as i32
            };
            (
                normalise(x, left, width),
                normalise(y, top, height),
                flags | MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
            )
        }
        None => (0, 0, flags),
    };
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    // SAFETY: one valid input structure.
    if unsafe { SendInput(&[input], size_of::<INPUT>() as i32) } != 1 {
        panic!("SendInput was refused");
    }
}

fn send_key(virtual_key: u8) {
    let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(u16::from(virtual_key)),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    // SAFETY: valid input structures.
    unsafe {
        SendInput(
            &[key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)],
            size_of::<INPUT>() as i32,
        )
    };
}

struct Lcg(u64);

impl Lcg {
    fn next_unit(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// One present of the target as PresentMon reports it.
struct Frame {
    swap_chain: String,
    present_qpc: i64,
    /// `None` when the frame never reached the screen.
    until_displayed_ms: Option<f64>,
    mode: String,
}

fn read_frames(csv: &PathBuf, pid: u32) -> Result<Vec<Frame>, String> {
    let text = std::fs::read_to_string(csv)
        .map_err(|error| format!("cannot read {}: {error}", csv.display()))?;
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    let header: Vec<&str> = lines.next().unwrap_or_default().split(',').collect();
    let column = |name: &str| {
        header
            .iter()
            .position(|field| *field == name)
            .ok_or_else(|| format!("PresentMon CSV has no {name} column"))
    };
    let (process, swap_chain, time, displayed, mode) = (
        column("ProcessID")?,
        column("SwapChainAddress")?,
        column("TimeInQPC")?,
        column("MsUntilDisplayed")?,
        column("PresentMode")?,
    );
    let mut frames = Vec::new();
    for line in lines {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.get(process).and_then(|value| value.parse().ok()) != Some(pid) {
            continue;
        }
        let Some(present_qpc) = fields.get(time).and_then(|value| value.parse().ok()) else {
            continue;
        };
        frames.push(Frame {
            swap_chain: fields
                .get(swap_chain)
                .copied()
                .unwrap_or_default()
                .to_owned(),
            present_qpc,
            until_displayed_ms: fields.get(displayed).and_then(|value| value.parse().ok()),
            mode: fields.get(mode).copied().unwrap_or_default().to_owned(),
        });
    }
    frames.sort_by_key(|frame| frame.present_qpc);
    Ok(frames)
}

fn summary(name: &str, mut values: Vec<f64>) {
    if values.is_empty() {
        println!("{name}: no samples");
        return;
    }
    values.sort_by(f64::total_cmp);
    let rank = |fraction: f64| values[(fraction * (values.len() - 1) as f64).round() as usize];
    println!(
        "{name}: n={} p50={:.2}ms p95={:.2}ms max={:.2}ms min={:.2}ms",
        values.len(),
        rank(0.5),
        rank(0.95),
        values[values.len() - 1],
        values[0],
    );
}

fn run(options: &Options) -> Result<(), String> {
    // SAFETY: called before any window exists in this process.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }
        .map_err(|error| error.to_string())?;
    let exe_name = options
        .exe
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("the app path has no file name")?;

    // PresentMon must run before the app starts: it misses most presents of
    // a DX12 swap chain created before the trace. It records for a fixed time
    // and exits by itself, which flushes every row; stopping its session
    // from outside loses them.
    let seconds = 15 + (options.warmup + options.steps) * 45 / 1000;
    let mut presentmon = Command::new(&options.presentmon)
        .args(["--process_name", exe_name, "--output_file"])
        .arg(&options.csv)
        .args([
            "--qpc_time",
            // Input tracking delays the app's input by several milliseconds.
            "--no_track_input",
            "--timed",
            &seconds.to_string(),
            "--terminate_after_timed",
            "--stop_existing_session",
            "--no_console_stats",
            "--session_name",
            SESSION,
        ])
        // PresentMon stops at once when its stdout is a pipe.
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("cannot start PresentMon: {error}"))?;
    let started = (0..100).any(|_| {
        std::thread::sleep(Duration::from_millis(100));
        Command::new("logman")
            .args(["query", SESSION, "-ets"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    });
    if !started {
        let _ = presentmon.kill();
        return Err("PresentMon did not start recording; it needs administrator rights".into());
    }

    let mut app = Command::new(&options.exe)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("cannot start {}: {error}", options.exe.display()))?;
    let pid = app.id();
    let result = measure(options, pid);
    // Killed rather than closed so that an app cannot write its settings.
    let _ = app.kill();
    let _ = app.wait();
    presentmon
        .wait()
        .map_err(|error| format!("PresentMon failed: {error}"))?;
    let (inputs, end) = result?;
    report(&read_frames(&options.csv, pid)?, &inputs, end);
    Ok(())
}

/// Drives the stroke and returns the input times and the end of the last step.
fn measure(options: &Options, pid: u32) -> Result<(Vec<i64>, i64), String> {
    let hwnd = (0..200)
        .find_map(|_| {
            std::thread::sleep(Duration::from_millis(100));
            find_window(pid)
        })
        .ok_or("the app showed no window")?;
    std::thread::sleep(Duration::from_secs(2));
    match options.window {
        // SAFETY: valid window.
        Some([width, height]) => unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            SetWindowPos(
                hwnd,
                None,
                100,
                100,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        }
        .map_err(|error| error.to_string())?,
        // SAFETY: valid window.
        None => unsafe {
            let _ = ShowWindow(hwnd, SW_MAXIMIZE);
        },
    }
    // Windows lets a background process take the foreground only right after
    // input of its own; Alt alone does nothing in the target.
    send_key(VK_MENU.0 as u8);
    // SAFETY: valid window.
    let _ = unsafe { SetForegroundWindow(hwnd) };
    std::thread::sleep(Duration::from_millis(1500));
    // Input goes wherever the cursor is, so never press a button over
    // another app.
    // SAFETY: plain query.
    if unsafe { GetForegroundWindow() } != hwnd {
        return Err("the app did not come to the front; no input sent".into());
    }
    if let Some(key) = options.key {
        send_key(key);
        std::thread::sleep(Duration::from_millis(500));
    }

    let mut client = RECT::default();
    // SAFETY: valid window and out pointer.
    unsafe { GetClientRect(hwnd, &mut client) }.map_err(|error| error.to_string())?;
    println!("client {}x{}", client.right, client.bottom);
    let mut origin = POINT { x: 0, y: 0 };
    // SAFETY: valid window and out pointer.
    let _ = unsafe { ClientToScreen(hwnd, &mut origin) };
    let center = [
        f64::from(origin.x) + f64::from(client.right) / 2.0,
        f64::from(origin.y) + f64::from(client.bottom) / 2.0,
    ];
    let radius = options.radius * f64::from(client.right.min(client.bottom));
    let point = |step: usize| {
        let progress = step as f64 / (options.warmup + options.steps) as f64;
        let angle = TAU * 2.0 * progress;
        let r = radius * (1.0 - 0.6 * progress);
        [
            (center[0] + r * angle.cos()).round() as i32,
            (center[1] + r * angle.sin()).round() as i32,
        ]
    };

    let mut random = Lcg(0x5547_5550);
    let mut inputs = Vec::with_capacity(options.steps);
    send_mouse(MOUSEEVENTF_MOVE, Some(point(0)));
    std::thread::sleep(Duration::from_millis(200));
    let [x, y] = point(0);
    // SAFETY: plain queries.
    let under = unsafe { GetAncestor(WindowFromPoint(POINT { x, y }), GA_ROOT) };
    // SAFETY: plain query.
    if under != hwnd || unsafe { GetForegroundWindow() } != hwnd {
        return Err("the app is not under the cursor; no button pressed".into());
    }
    send_mouse(MOUSEEVENTF_LEFTDOWN, None);
    std::thread::sleep(Duration::from_millis(300));
    for step in 1..=options.warmup + options.steps {
        if step > options.warmup {
            inputs.push(qpc());
        }
        send_mouse(MOUSEEVENTF_MOVE, Some(point(step)));
        std::thread::sleep(Duration::from_millis(
            15 + (random.next_unit() * 25.0) as u64,
        ));
    }
    let end = qpc();
    send_mouse(MOUSEEVENTF_LEFTUP, None);
    std::thread::sleep(Duration::from_millis(500));
    Ok((inputs, end))
}

/// Reports each swap chain on its own: an app may show the canvas in one
/// swap chain and the rest of its window in another.
fn report(frames: &[Frame], inputs: &[i64], end: i64) {
    let mut swap_chains: Vec<&str> = frames
        .iter()
        .map(|frame| frame.swap_chain.as_str())
        .collect();
    swap_chains.sort_unstable();
    swap_chains.dedup();
    println!(
        "steps={} frames={} swap_chains={}",
        inputs.len(),
        frames.len(),
        swap_chains.len()
    );
    for swap_chain in swap_chains {
        let frames: Vec<&Frame> = frames
            .iter()
            .filter(|frame| frame.swap_chain == swap_chain)
            .collect();
        report_swap_chain(swap_chain, &frames, inputs, end);
    }
}

fn report_swap_chain(swap_chain: &str, frames: &[&Frame], inputs: &[i64], end: i64) {
    let ticks_per_ms = qpc_ticks_per_ms();
    let mut to_present = Vec::new();
    let mut to_display = Vec::new();
    let mut misses = 0;
    let mut undisplayed = 0;
    let mut extra_presents = 0;
    let mut modes = BTreeMap::<&str, usize>::new();
    let mut by_mode = BTreeMap::<&str, Vec<f64>>::new();
    for (index, &input) in inputs.iter().enumerate() {
        let next = inputs.get(index + 1).copied().unwrap_or(end);
        let mut step_frames = frames
            .iter()
            .filter(|frame| frame.present_qpc >= input && frame.present_qpc < next);
        let Some(first) = step_frames.next() else {
            misses += 1;
            continue;
        };
        extra_presents += step_frames.count();
        *modes.entry(first.mode.as_str()).or_default() += 1;
        let present_ms = (first.present_qpc - input) as f64 / ticks_per_ms;
        to_present.push(present_ms);
        match first.until_displayed_ms {
            Some(displayed) => {
                to_display.push(present_ms + displayed);
                by_mode
                    .entry(first.mode.as_str())
                    .or_default()
                    .push(present_ms + displayed);
            }
            None => undisplayed += 1,
        }
    }

    println!(
        "swap chain {swap_chain}: frames={} misses={misses} undisplayed={undisplayed} extra_presents={extra_presents}",
        frames.len()
    );
    println!("  present modes: {modes:?}");
    summary("  input -> present", to_present);
    summary("  input -> displayed", to_display);
    if by_mode.len() > 1 {
        for (mode, values) in by_mode {
            summary(&format!("    {mode}"), values);
        }
    }
}
