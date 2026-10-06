// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Measures input-to-screen latency of a drawing app from outside the app.
//!
//! The probe holds the primary mouse button in the target window and extends a
//! spiral stroke one step at a time with `SendInput`. For each step it waits
//! for the first desktop frame, captured with Desktop Duplication, in which
//! the pixels around the new segment changed, and reports that frame's
//! `LastPresentTime` minus the time just before the input was injected. The
//! hardware cursor is not part of duplicated frames, so only app drawing
//! counts. Both the C++ and the Rust app are measured the same way.

use std::f64::consts::TAU;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_FLAG, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, IDXGIAdapter1,
    IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
};
use windows::Win32::Graphics::Gdi::{ClientToScreen, MONITOR_DEFAULTTONEAREST, MonitorFromWindow};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT, SendInput,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClientRect, GetSystemMetrics, GetWindowThreadProcessId, IsWindowVisible,
    SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    SetForegroundWindow,
};
use windows::core::{BOOL, Interface, Result};

const USAGE: &str =
    "usage: latency-probe --pid <pid> [--steps N] [--radius FRACTION] [--margin PX]";

struct Options {
    pid: u32,
    steps: usize,
    radius: f64,
    margin: i32,
}

fn parse() -> Option<Options> {
    let mut options = Options {
        pid: 0,
        steps: 200,
        radius: 0.25,
        margin: 6,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args.next()?;
        match arg.as_str() {
            "--pid" => options.pid = value.parse().ok()?,
            "--steps" => options.steps = value.parse().ok()?,
            "--radius" => options.radius = value.parse().ok()?,
            "--margin" => options.margin = value.parse().ok()?,
            _ => return None,
        }
    }
    (options.pid != 0).then_some(options)
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

fn qpc_frequency() -> f64 {
    let mut value = 0;
    // SAFETY: valid out pointer.
    let _ = unsafe { QueryPerformanceFrequency(&mut value) };
    value as f64
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
    unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
}

struct Capture {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    /// Desktop origin of the duplicated output, in virtual-screen pixels.
    origin: [i32; 2],
    size: [i32; 2],
    latest: Option<ID3D11Texture2D>,
}

impl Capture {
    fn for_window(hwnd: HWND) -> Result<Self> {
        // SAFETY: valid window.
        let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
        // SAFETY: COM factory creation.
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }?;
        let mut adapter_index = 0;
        // SAFETY: enumeration stops at the first error.
        while let Ok(adapter) = unsafe { factory.EnumAdapters1(adapter_index) } {
            adapter_index += 1;
            let mut output_index = 0;
            // SAFETY: enumeration stops at the first error.
            while let Ok(output) = unsafe { adapter.EnumOutputs(output_index) } {
                output_index += 1;
                // SAFETY: plain getter.
                let desc = unsafe { output.GetDesc() }?;
                if desc.Monitor != monitor {
                    continue;
                }
                let rect = desc.DesktopCoordinates;
                return Self::create(&adapter, &output.cast()?, rect);
            }
        }
        Err(windows::core::Error::new(
            windows::Win32::Foundation::E_FAIL,
            "no DXGI output shows the target window",
        ))
    }

    fn create(adapter: &IDXGIAdapter1, output: &IDXGIOutput1, rect: RECT) -> Result<Self> {
        let mut device = None;
        let mut context = None;
        // SAFETY: out pointers are valid for the call.
        unsafe {
            D3D11CreateDevice(
                adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                Default::default(),
                D3D11_CREATE_DEVICE_FLAG(0),
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }?;
        let device = device.expect("D3D11CreateDevice succeeded without a device");
        let context = context.expect("D3D11CreateDevice succeeded without a context");
        // SAFETY: the device belongs to the adapter that owns the output.
        let duplication = unsafe { output.DuplicateOutput(&device) }?;
        Ok(Self {
            device,
            context,
            duplication,
            origin: [rect.left, rect.top],
            size: [rect.right - rect.left, rect.bottom - rect.top],
            latest: None,
        })
    }

    /// Waits up to `timeout` for a desktop update and keeps a copy of it.
    /// Returns the update's present time, or `None` on timeout.
    fn next_update(&mut self, timeout: Duration) -> Result<Option<i64>> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource: Option<IDXGIResource> = None;
            // SAFETY: out pointers are valid for the call.
            match unsafe {
                self.duplication.AcquireNextFrame(
                    remaining.as_millis() as u32,
                    &mut info,
                    &mut resource,
                )
            } {
                Ok(()) => {}
                Err(error) if error.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(None),
                Err(error) => return Err(error),
            }
            let present = info.LastPresentTime;
            let copied = match resource {
                Some(resource) if present != 0 => self.keep(&resource.cast()?),
                _ => Ok(()),
            };
            // SAFETY: a frame was acquired above.
            unsafe { self.duplication.ReleaseFrame() }?;
            copied?;
            if present != 0 {
                return Ok(Some(present));
            }
            if remaining.is_zero() {
                return Ok(None);
            }
        }
    }

    fn keep(&mut self, frame: &ID3D11Texture2D) -> Result<()> {
        if self.latest.is_none() {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            // SAFETY: valid out pointer.
            unsafe { frame.GetDesc(&mut desc) };
            desc.BindFlags = 0;
            desc.MiscFlags = 0;
            let mut texture = None;
            // SAFETY: valid descriptor and out pointer.
            unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut texture)) }?;
            self.latest = texture;
        }
        let latest = self.latest.as_ref().expect("created above");
        // SAFETY: both textures share size and format.
        unsafe { self.context.CopyResource(latest, frame) };
        Ok(())
    }

    /// Reads the BGRA pixels of a desktop rectangle from the latest update.
    fn read(&self, rect: [i32; 4]) -> Result<Vec<u8>> {
        let left = (rect[0] - self.origin[0]).clamp(0, self.size[0]);
        let top = (rect[1] - self.origin[1]).clamp(0, self.size[1]);
        let right = (rect[2] - self.origin[0]).clamp(left + 1, self.size[0]);
        let bottom = (rect[3] - self.origin[1]).clamp(top + 1, self.size[1]);
        let width = (right - left) as u32;
        let height = (bottom - top) as u32;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let Some(latest) = self.latest.as_ref() else {
            return Ok(Vec::new());
        };
        let mut staging = None;
        // SAFETY: valid descriptor and out pointer.
        unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut staging)) }?;
        let staging = staging.expect("CreateTexture2D succeeded without a texture");
        let source = D3D11_BOX {
            left: left as u32,
            top: top as u32,
            front: 0,
            right: right as u32,
            bottom: bottom as u32,
            back: 1,
        };
        // SAFETY: the box lies inside the desktop texture.
        unsafe {
            self.context
                .CopySubresourceRegion(&staging, 0, 0, 0, 0, latest, 0, Some(&source))
        };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: staging texture with CPU read access.
        unsafe {
            self.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        }?;
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for row in 0..height {
            // SAFETY: the mapping covers `height` rows of `RowPitch` bytes.
            let line = unsafe {
                std::slice::from_raw_parts(
                    (mapped.pData as *const u8).add((row * mapped.RowPitch) as usize),
                    (width * 4) as usize,
                )
            };
            pixels.extend_from_slice(line);
        }
        // SAFETY: mapped above.
        unsafe { self.context.Unmap(&staging, 0) };
        Ok(pixels)
    }
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

fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    let index = (fraction * (sorted.len() - 1) as f64).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

fn run(options: &Options) -> Result<()> {
    // SAFETY: called before any window or DXGI object exists in this process.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }?;
    let hwnd = find_window(options.pid).ok_or_else(|| {
        windows::core::Error::new(
            windows::Win32::Foundation::E_FAIL,
            "the process has no visible window",
        )
    })?;
    // SAFETY: valid window.
    let _ = unsafe { SetForegroundWindow(hwnd) };
    std::thread::sleep(Duration::from_millis(500));

    let mut client = RECT::default();
    // SAFETY: valid window and out pointer.
    unsafe { GetClientRect(hwnd, &mut client) }?;
    let mut origin = POINT { x: 0, y: 0 };
    // SAFETY: valid window and out pointer.
    let _ = unsafe { ClientToScreen(hwnd, &mut origin) };
    let center = [
        f64::from(origin.x) + f64::from(client.right) / 2.0,
        f64::from(origin.y) + f64::from(client.bottom) / 2.0,
    ];
    let radius = options.radius * f64::from(client.right.min(client.bottom));
    let point = |step: usize| {
        let progress = step as f64 / options.steps as f64;
        let angle = TAU * 2.0 * progress;
        let r = radius * (1.0 - 0.6 * progress);
        [
            (center[0] + r * angle.cos()).round() as i32,
            (center[1] + r * angle.sin()).round() as i32,
        ]
    };

    let mut capture = Capture::for_window(hwnd)?;
    capture.next_update(Duration::from_millis(1000))?;
    while capture.next_update(Duration::ZERO)?.is_some() {}

    let frequency = qpc_frequency();
    let mut random = Lcg(0x5547_5550);
    let mut latencies = Vec::with_capacity(options.steps);
    let mut misses = 0;

    send_mouse(MOUSEEVENTF_MOVE, Some(point(0)));
    std::thread::sleep(Duration::from_millis(200));
    send_mouse(MOUSEEVENTF_LEFTDOWN, None);
    std::thread::sleep(Duration::from_millis(300));
    while capture.next_update(Duration::ZERO)?.is_some() {}

    for step in 1..=options.steps {
        let [x0, y0] = point(step - 1);
        let [x1, y1] = point(step);
        let rect = [
            x0.min(x1) - options.margin,
            y0.min(y1) - options.margin,
            x0.max(x1) + options.margin + 1,
            y0.max(y1) + options.margin + 1,
        ];
        let before = capture.read(rect)?;
        let sent = qpc();
        send_mouse(MOUSEEVENTF_MOVE, Some([x1, y1]));

        let deadline = Instant::now() + Duration::from_millis(500);
        let mut hit = None;
        while hit.is_none() && Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if let Some(present) = capture.next_update(remaining)?
                && capture.read(rect)? != before
            {
                hit = Some((present - sent) as f64 * 1000.0 / frequency);
            }
        }
        match hit {
            Some(millis) => {
                println!("step {step} latency_ms={millis:.2}");
                latencies.push(millis);
            }
            None => {
                println!("step {step} miss");
                misses += 1;
            }
        }
        std::thread::sleep(Duration::from_millis(
            15 + (random.next_unit() * 25.0) as u64,
        ));
        while capture.next_update(Duration::ZERO)?.is_some() {}
    }
    send_mouse(MOUSEEVENTF_LEFTUP, None);

    latencies.sort_by(f64::total_cmp);
    if latencies.is_empty() {
        println!("summary samples=0 misses={misses}");
    } else {
        println!(
            "summary samples={} misses={misses} p50={:.2}ms p95={:.2}ms max={:.2}ms min={:.2}ms",
            latencies.len(),
            percentile(&latencies, 0.5),
            percentile(&latencies, 0.95),
            latencies[latencies.len() - 1],
            latencies[0],
        );
    }
    Ok(())
}
