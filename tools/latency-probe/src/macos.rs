// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Measures input-to-display latency of a drawing app from outside the app on
//! macOS, the counterpart of `probe` on Windows.
//!
//! The probe holds the primary mouse button in the target window and extends a
//! spiral stroke one step at a time with `CGEventPost`, while ScreenCaptureKit
//! captures that one window. ScreenCaptureKit hands over a complete frame only
//! when the window's content changed, with the time the window server
//! displayed it, so the first such frame after a step is that step's response
//! and the target must not redraw while idle; frames beyond the first per step
//! are reported to check this. Unlike on Windows, each step waits for its
//! response (or a timeout) before the random pause: only the display time is
//! known, and it can come later than the pause, which would credit a late
//! response to the next step.
//!
//! The terminal running the probe needs Accessibility (to post input and to
//! place the window) and Screen Recording in System Settings.

use std::f64::consts::TAU;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, MainThreadMarker, define_class, msg_send};
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSScreen};
use objc2_application_services::{AXError, AXIsProcessTrusted, AXUIElement, AXValue, AXValueType};
use objc2_core_foundation::{CFRetained, CFString, CFType, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGEvent, CGEventTapLocation, CGEventType, CGMouseButton, CGPreflightPostEventAccess,
    CGPreflightScreenCaptureAccess, CGRequestPostEventAccess, CGRequestScreenCaptureAccess,
};
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::{
    CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight,
    CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress, kCVPixelFormatType_32BGRA,
};
use objc2_foundation::{NSArray, NSDictionary, NSError, NSNumber, NSString};
use objc2_screen_capture_kit::{
    SCContentFilter, SCFrameStatus, SCShareableContent, SCStream, SCStreamConfiguration,
    SCStreamFrameInfoDisplayTime, SCStreamFrameInfoStatus, SCStreamOutput, SCStreamOutputType,
    SCWindow,
};

use crate::stats::{Lcg, summary};

const USAGE: &str = "usage: latency-probe --exe <app executable> \
                     [--steps N] [--warmup N] [--radius FRACTION] [--window WxH] [--key LETTER] [--csv PATH] \
                     [--document PATH] [--app-log PATH]";

/// How long a step waits for its response before it counts as a miss.
const RESPONSE_LIMIT: Duration = Duration::from_millis(250);

/// Frames counted while the cursor rests in the window before the stroke.
/// `kCGDockWindowLevel`.
const DOCK_LEVEL: isize = 20;
/// Points around the stroke's path that still count as the stroke. A stroke
/// covers its path, so this only absorbs rounding to captured pixels.
const STROKE_MARGIN: f64 = 8.0;
const IDLE: Duration = Duration::from_secs(2);

struct Options {
    /// The executable inside the bundle, such as `Ugurugu.app/Contents/MacOS/Ugurugu`.
    exe: PathBuf,
    steps: usize,
    /// Steps drawn first and left out of the results.
    warmup: usize,
    radius: f64,
    /// Outer window size in points, the same for every app measured.
    window: [f64; 2],
    /// A key pressed once before measuring, such as the C++ app's playback toggle.
    key: Option<u16>,
    /// One row per measured step.
    csv: PathBuf,
    /// Opened by the app at launch; an empty canvas without it.
    document: Option<PathBuf>,
    /// Where the app's standard output goes; discarded without it.
    app_log: Option<PathBuf>,
}

fn parse() -> Option<Options> {
    let mut exe = None;
    let mut steps = 200;
    let mut warmup = 0;
    let mut radius = 0.12;
    // 2560x1600 pixels at the 2x scale of a Retina display, the size the
    // Windows probe uses.
    let mut window = [1280.0, 800.0];
    let mut key = None;
    let mut csv = std::env::temp_dir().join("latency-probe-steps.csv");
    let mut document = None;
    let mut app_log = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args.next()?;
        match arg.as_str() {
            "--exe" => exe = Some(PathBuf::from(value)),
            "--steps" => steps = value.parse().ok()?,
            "--warmup" => warmup = value.parse().ok()?,
            "--radius" => radius = value.parse().ok()?,
            "--window" => {
                let (width, height) = value.split_once('x')?;
                window = [width.parse().ok()?, height.parse().ok()?];
            }
            "--key" => key = Some(key_code(value.chars().next()?)?),
            "--csv" => csv = PathBuf::from(value),
            "--document" => document = Some(PathBuf::from(value)),
            "--app-log" => app_log = Some(PathBuf::from(value)),
            _ => return None,
        }
    }
    Some(Options {
        exe: exe?,
        steps,
        warmup,
        radius,
        window,
        key,
        csv,
        document,
        app_log,
    })
}

/// The virtual key code of a letter on the ANSI layout (`kVK_ANSI_*`).
fn key_code(letter: char) -> Option<u16> {
    const CODES: [(char, u16); 26] = [
        ('a', 0x00),
        ('s', 0x01),
        ('d', 0x02),
        ('f', 0x03),
        ('h', 0x04),
        ('g', 0x05),
        ('z', 0x06),
        ('x', 0x07),
        ('c', 0x08),
        ('v', 0x09),
        ('b', 0x0B),
        ('q', 0x0C),
        ('w', 0x0D),
        ('e', 0x0E),
        ('r', 0x0F),
        ('y', 0x10),
        ('t', 0x11),
        ('o', 0x1F),
        ('u', 0x20),
        ('i', 0x22),
        ('p', 0x23),
        ('l', 0x25),
        ('j', 0x26),
        ('k', 0x28),
        ('n', 0x2D),
        ('m', 0x2E),
    ];
    let letter = letter.to_ascii_lowercase();
    CODES
        .iter()
        .find(|(candidate, _)| *candidate == letter)
        .map(|&(_, code)| code)
}

pub fn main() -> ExitCode {
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

#[repr(C)]
struct Timebase {
    numer: u32,
    denom: u32,
}

unsafe extern "C" {
    fn mach_absolute_time() -> u64;
    fn mach_timebase_info(info: *mut Timebase) -> i32;
}

/// The clock of `SCStreamFrameInfoDisplayTime`.
fn now() -> u64 {
    // SAFETY: no arguments.
    unsafe { mach_absolute_time() }
}

fn ticks_per_ms() -> f64 {
    let mut timebase = Timebase { numer: 0, denom: 0 };
    // SAFETY: valid out pointer.
    unsafe { mach_timebase_info(&mut timebase) };
    1e6 * f64::from(timebase.denom) / f64::from(timebase.numer)
}

/// A complete frame of the captured window.
#[derive(Clone, Debug)]
struct Captured {
    /// When the window server displayed it.
    display: u64,
    /// When the probe received it.
    arrival: u64,
    /// What differs from the frame before, or why it could not be read.
    change: Result<Change, &'static str>,
}

/// Where and how much a frame's pixels differ from the frame before.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Change {
    /// `[x, y, width, height]` around the differing pixels, in captured pixels.
    bounds: [usize; 4],
    pixels: usize,
    /// The largest difference of one colour channel.
    largest: u8,
    /// Whether a differing pixel lies where the stroke is drawn.
    on_stroke: bool,
}

/// The rows of the last frame, without their padding.
#[derive(Default)]
struct Pixels {
    width: usize,
    rows: Vec<u8>,
}

/// Frames whose pixels differ from the frame before them. ScreenCaptureKit
/// marked every frame of a captured window complete and wholly changed, at
/// about the display rate, so the pixels decide. Only a change where the
/// stroke is drawn counts as a response: an app may animate elsewhere, as
/// 2.2.13 does at the top right of its window.
#[derive(Default)]
struct Frames {
    list: Mutex<Vec<Captured>>,
    arrived: Condvar,
    last_pixels: Mutex<Pixels>,
    delivered: AtomicUsize,
    /// Where the stroke is drawn, as `[left, top, right, bottom]` fractions
    /// of the window; the whole window until set.
    stroke: OnceLock<[f64; 4]>,
}

impl Captured {
    fn on_stroke(&self) -> bool {
        self.change.is_ok_and(|change| change.on_stroke)
    }
}

impl Frames {
    fn count(&self) -> usize {
        self.list.lock().unwrap().len()
    }

    fn delivered(&self) -> usize {
        self.delivered.load(Ordering::Relaxed)
    }

    /// Waits until a frame displayed at `since` or later arrives.
    fn wait_from(&self, since: u64, limit: Duration) {
        let list = self.list.lock().unwrap();
        let _ = self
            .arrived
            .wait_timeout_while(list, limit, |list| {
                !list
                    .iter()
                    .any(|frame| frame.on_stroke() && frame.display >= since)
            })
            .unwrap();
    }
}

struct OutputIvars {
    frames: Arc<Frames>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and `Output` does not
    // implement `Drop`.
    #[unsafe(super(NSObject))]
    #[name = "UguruguLatencyProbeOutput"]
    #[ivars = OutputIvars]
    struct Output;

    unsafe impl NSObjectProtocol for Output {}

    unsafe impl SCStreamOutput for Output {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn did_output(
            &self,
            _stream: &SCStream,
            sample: &CMSampleBuffer,
            kind: SCStreamOutputType,
        ) {
            let arrival = now();
            if kind != SCStreamOutputType::Screen {
                return;
            }
            let Some(display) = complete_display_time(sample) else {
                return;
            };
            let frames = &self.ivars().frames;
            frames.delivered.fetch_add(1, Ordering::Relaxed);
            let stroke = frames.stroke.get().copied().unwrap_or([0.0, 0.0, 1.0, 1.0]);
            let change = match pixel_change(sample, &mut frames.last_pixels.lock().unwrap(), stroke)
            {
                Ok(None) => return,
                Ok(Some(change)) => Ok(change),
                Err(reason) => Err(reason),
            };
            frames.list.lock().unwrap().push(Captured {
                display,
                arrival,
                change,
            });
            frames.arrived.notify_all();
        }
    }
);

impl Output {
    fn new(frames: Arc<Frames>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(OutputIvars { frames });
        // SAFETY: NSObject's designated initialiser.
        unsafe { msg_send![super(this), init] }
    }
}

/// The display time of a frame ScreenCaptureKit marks complete; `None` for
/// the idle, blank and other status buffers.
fn complete_display_time(sample: &CMSampleBuffer) -> Option<u64> {
    // SAFETY: valid buffer; `false` creates nothing.
    let attachments = unsafe { sample.sample_attachments_array(false) }?;
    // SAFETY: CFArray and CFDictionary are toll-free bridged to NSArray and
    // NSDictionary, and ScreenCaptureKit attaches one dictionary with string
    // keys to each frame.
    let array: &NSArray<NSDictionary<NSString, AnyObject>> =
        unsafe { CFRetained::as_ptr(&attachments).cast().as_ref() };
    let info = array.firstObject()?;
    let number = |key: &NSString| info.objectForKey(key)?.downcast::<NSNumber>().ok();
    // SAFETY: constants exported by ScreenCaptureKit.
    let (status, display) = unsafe { (SCStreamFrameInfoStatus, SCStreamFrameInfoDisplayTime) };
    if number(status)?.integerValue() != SCFrameStatus::Complete.0 {
        return None;
    }
    Some(number(display)?.unsignedLongLongValue())
}

/// How the frame differs from `last`, which it then replaces: `Ok(None)`
/// when nothing differs.
fn pixel_change(
    sample: &CMSampleBuffer,
    last: &mut Pixels,
    stroke: [f64; 4],
) -> Result<Option<Change>, &'static str> {
    // SAFETY: valid buffer.
    let buffer = unsafe { sample.image_buffer() }.ok_or("no image buffer")?;
    if CVPixelBufferGetPixelFormatType(&buffer) != kCVPixelFormatType_32BGRA {
        return Err("not BGRA");
    }
    // SAFETY: valid pixel buffer, unlocked below with the same flags.
    if unsafe { CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags::ReadOnly) } != 0 {
        return Err("cannot lock");
    }
    let base = CVPixelBufferGetBaseAddress(&buffer).cast::<u8>();
    let stride = CVPixelBufferGetBytesPerRow(&buffer);
    let width = CVPixelBufferGetWidth(&buffer);
    let height = CVPixelBufferGetHeight(&buffer);
    let readable = !base.is_null() && width * 4 <= stride;
    let mut rows = Vec::with_capacity(width * 4 * height);
    if readable {
        for row in 0..height {
            // SAFETY: the locked BGRA buffer holds `height` rows of
            // `stride` bytes; the padding past `width` pixels is left out.
            rows.extend_from_slice(unsafe {
                std::slice::from_raw_parts(base.add(row * stride), width * 4)
            });
        }
    }
    // SAFETY: locked above with these flags.
    unsafe { CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags::ReadOnly) };
    if !readable {
        return Err("no base address");
    }
    let previous = std::mem::replace(last, Pixels { width, rows });
    if previous.width != width || previous.rows.len() != last.rows.len() {
        return Ok(Some(Change {
            bounds: [0, 0, width, height],
            pixels: width * height,
            largest: u8::MAX,
            on_stroke: true,
        }));
    }
    let [left, top, right, bottom] = stroke;
    let scale = |fraction: f64, size: usize| (fraction * size as f64).round() as usize;
    let stroke = [
        scale(left, width),
        scale(top, height),
        scale(right, width),
        scale(bottom, height),
    ];
    Ok(differences(&previous.rows, &last.rows, width, stroke))
}

/// `stroke` is `[left, top, right, bottom]` in pixels, the last two outside.
fn differences(before: &[u8], after: &[u8], width: usize, stroke: [usize; 4]) -> Option<Change> {
    let (mut left, mut top, mut right, mut bottom) = (usize::MAX, usize::MAX, 0, 0);
    let (mut pixels, mut largest) = (0, 0);
    let mut on_stroke = false;
    for (index, (old, new)) in before
        .as_chunks::<4>()
        .0
        .iter()
        .zip(after.as_chunks::<4>().0)
        .enumerate()
    {
        let difference = old
            .iter()
            .zip(new)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        if difference > 0 {
            let (x, y) = (index % width, index / width);
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
            pixels += 1;
            largest = largest.max(difference);
            on_stroke |= (stroke[0]..stroke[2]).contains(&x) && (stroke[1]..stroke[3]).contains(&y);
        }
    }
    (pixels > 0).then(|| Change {
        bounds: [left, top, right + 1 - left, bottom + 1 - top],
        pixels,
        largest,
        on_stroke,
    })
}

/// The most frequent changes among the frames in `range`.
fn change_summary(frames: &Frames, range: std::ops::Range<usize>) -> String {
    let list = frames.list.lock().unwrap();
    let mut tally: Vec<(Result<Change, &'static str>, usize)> = Vec::new();
    for frame in &list[range] {
        match tally.iter_mut().find(|(seen, _)| *seen == frame.change) {
            Some((_, count)) => *count += 1,
            None => tally.push((frame.change, 1)),
        }
    }
    tally.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    tally
        .iter()
        .take(5)
        .map(|(change, count)| match change {
            Ok(change) => format!(
                "{count}x {:?} {} px up to {}{}",
                change.bounds,
                change.pixels,
                change.largest,
                if change.on_stroke {
                    " on the stroke"
                } else {
                    ""
                }
            ),
            Err(reason) => format!("{count}x unreadable ({reason})"),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Moves a value made on a ScreenCaptureKit queue to the waiting thread.
struct Handed<T>(T);

// SAFETY: only used for shareable content snapshots and error text, which
// nothing mutates after ScreenCaptureKit hands them over.
unsafe impl<T> Send for Handed<T> {}

fn error_text(error: *mut NSError) -> String {
    // SAFETY: ScreenCaptureKit passes a valid error or null.
    match unsafe { error.as_ref() } {
        Some(error) => error.localizedDescription().to_string(),
        None => "no reason given".into(),
    }
}

/// On-screen windows, all of them or only those above `window`.
fn shareable(above: Option<&SCWindow>) -> Result<Retained<SCShareableContent>, String> {
    let (send, receive) = mpsc::channel();
    let handler = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            // SAFETY: ScreenCaptureKit passes valid content or null.
            let result = match unsafe { Retained::retain(content) } {
                Some(content) => Ok(content),
                None => Err(error_text(error)),
            };
            let _ = send.send(Handed(result));
        },
    );
    // SAFETY: valid block, called once.
    unsafe {
        match above {
            Some(window) => SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnlyAboveWindow_completionHandler(true, window, &handler),
            None => SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(true, true, &handler),
        }
    }
    let Handed(result) = receive
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "ScreenCaptureKit did not list the windows")?;
    result.map_err(|error| format!("cannot list the windows: {error}"))
}

/// The largest ordinary window of `pid` on screen.
fn find_window(pid: i32) -> Result<Option<Retained<SCWindow>>, String> {
    let content = shareable(None)?;
    // SAFETY: plain property reads of a snapshot.
    let window = unsafe {
        content
            .windows()
            .iter()
            .filter(|window| {
                window.windowLayer() == 0
                    && window
                        .owningApplication()
                        .is_some_and(|app| app.processID() == pid)
            })
            .max_by(|a, b| area(a.frame()).total_cmp(&area(b.frame())))
    };
    Ok(window)
}

fn area(rect: CGRect) -> f64 {
    rect.size.width * rect.size.height
}

fn contains(rect: CGRect, point: CGPoint) -> bool {
    point.x >= rect.origin.x
        && point.y >= rect.origin.y
        && point.x < rect.origin.x + rect.size.width
        && point.y < rect.origin.y + rect.size.height
}

/// What covers any of `points` (in screen points, origin top left) over
/// `window`, if anything. The Dock's window spans the whole screen while it
/// draws only the Dock, so the Dock and the menu bar are judged by the visible
/// frame of the screens, and only windows below the Dock's level by their frame.
fn covered(window: &SCWindow, points: &[CGPoint]) -> Result<Option<String>, String> {
    let main_thread = MainThreadMarker::new().ok_or("the probe must run on the main thread")?;
    let screens = NSScreen::screens(main_thread);
    let primary_height = screens
        .firstObject()
        .ok_or("macOS reports no screen")?
        .frame()
        .size
        .height;
    let visible = |point: &CGPoint| {
        let flipped = CGPoint::new(point.x, primary_height - point.y);
        screens
            .iter()
            .any(|screen| contains(screen.visibleFrame(), flipped))
    };
    if !points.iter().all(visible) {
        return Ok(Some("the Dock or the menu bar".into()));
    }
    let content = shareable(Some(window))?;
    // SAFETY: plain property reads of a snapshot.
    let cover = unsafe {
        content.windows().iter().find(|other| {
            (0..DOCK_LEVEL).contains(&other.windowLayer())
                && points.iter().any(|&point| contains(other.frame(), point))
        })
    };
    // SAFETY: plain property reads of a snapshot.
    Ok(cover.map(|other| unsafe {
        other
            .owningApplication()
            .map_or("an unknown app".into(), |app| {
                app.applicationName().to_string()
            })
    }))
}

fn attribute(element: &AXUIElement, name: &'static str) -> Option<CFRetained<CFType>> {
    let mut value: *const CFType = std::ptr::null();
    // SAFETY: valid element and out pointer.
    let error = unsafe {
        element.copy_attribute_value(&CFString::from_static_str(name), NonNull::from(&mut value))
    };
    // SAFETY: the copy rule hands over one reference.
    (error == AXError::Success)
        .then(|| NonNull::new(value.cast_mut()).map(|value| unsafe { CFRetained::from_raw(value) }))
        .flatten()
}

/// Places the app's main window with Accessibility, as the Windows probe does
/// with `SetWindowPos`.
fn place(pid: i32, origin: CGPoint, size: CGSize) -> Result<(), String> {
    // SAFETY: any pid is accepted; a wrong one fails the calls below.
    let app = unsafe { AXUIElement::new_application(pid) };
    let window = attribute(&app, "AXMainWindow")
        .or_else(|| attribute(&app, "AXFocusedWindow"))
        .ok_or("Accessibility gives no main window of the app")?
        .downcast::<AXUIElement>()
        .map_err(|_| "the app's main window is not an element")?;
    let set = |name: &'static str, kind: AXValueType, value: NonNull<std::ffi::c_void>| {
        // SAFETY: `value` points at the CGPoint or CGSize that `kind` names.
        let value = unsafe { AXValue::new(kind, value) }.ok_or("cannot make an AXValue")?;
        // SAFETY: valid element and value.
        match unsafe { window.set_attribute_value(&CFString::from_static_str(name), &value) } {
            AXError::Success => Ok(()),
            error => Err(format!("cannot set {name}: AXError {}", error.0)),
        }
    };
    set(
        "AXPosition",
        AXValueType::CGPoint,
        NonNull::from(&origin).cast(),
    )?;
    set("AXSize", AXValueType::CGSize, NonNull::from(&size).cast())
}

fn post_mouse(kind: CGEventType, at: CGPoint) {
    let event = CGEvent::new_mouse_event(None, kind, at, CGMouseButton::Left)
        .expect("CGEventCreateMouseEvent failed");
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
}

fn post_key(code: u16) {
    for down in [true, false] {
        let event = CGEvent::new_keyboard_event(None, code, down)
            .expect("CGEventCreateKeyboardEvent failed");
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
    }
}

/// Lets the primary button go wherever the probe stops, errors included.
struct Held {
    at: CGPoint,
}

impl Drop for Held {
    fn drop(&mut self) {
        post_mouse(CGEventType::LeftMouseUp, self.at);
    }
}

struct Capture {
    stream: Retained<SCStream>,
    // Kept alive while the stream runs; the stream does not retain it.
    _output: Retained<Output>,
}

fn start_capture(window: &SCWindow, frames: Arc<Frames>) -> Result<Capture, String> {
    // SAFETY: ScreenCaptureKit objects set up before the stream starts.
    let stream = unsafe {
        let filter =
            SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), window);
        let config = SCStreamConfiguration::new();
        // Half the size is enough to see a stroke step and quick to compare.
        let frame = window.frame();
        config.setWidth((frame.size.width / 2.0).max(16.0) as usize);
        config.setHeight((frame.size.height / 2.0).max(16.0) as usize);
        config.setMinimumFrameInterval(CMTime::new(1, 240));
        config.setQueueDepth(8);
        config.setShowsCursor(false);
        config.setPixelFormat(kCVPixelFormatType_32BGRA);
        SCStream::initWithFilter_configuration_delegate(SCStream::alloc(), &filter, &config, None)
    };
    let output = Output::new(frames);
    let queue = DispatchQueue::new("latency-probe.capture", None);
    // SAFETY: valid stream, output and queue.
    unsafe {
        stream.addStreamOutput_type_sampleHandlerQueue_error(
            ProtocolObject::from_ref(&*output),
            SCStreamOutputType::Screen,
            Some(&queue),
        )
    }
    .map_err(|error| {
        format!(
            "cannot add the capture output: {}",
            error.localizedDescription()
        )
    })?;
    let (send, receive) = mpsc::channel();
    let handler = RcBlock::new(move |error: *mut NSError| {
        let _ = send.send(Handed((!error.is_null()).then(|| error_text(error))));
    });
    // SAFETY: valid block, called once.
    unsafe { stream.startCaptureWithCompletionHandler(Some(&handler)) };
    match receive.recv_timeout(Duration::from_secs(10)) {
        Ok(Handed(None)) => Ok(Capture {
            stream,
            _output: output,
        }),
        Ok(Handed(Some(error))) => Err(format!("cannot start the capture: {error}")),
        Err(_) => Err("the capture did not start".into()),
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let (send, receive) = mpsc::channel();
        let handler = RcBlock::new(move |_: *mut NSError| {
            let _ = send.send(());
        });
        // SAFETY: valid block, called once.
        unsafe { self.stream.stopCaptureWithCompletionHandler(Some(&handler)) };
        let _ = receive.recv_timeout(Duration::from_secs(5));
    }
}

fn permitted() -> Result<(), String> {
    // SAFETY: plain query.
    let accessibility = unsafe { AXIsProcessTrusted() } && CGPreflightPostEventAccess();
    let recording = CGPreflightScreenCaptureAccess();
    if accessibility && recording {
        return Ok(());
    }
    // These add the terminal to the lists in System Settings, switched off.
    if !accessibility {
        CGRequestPostEventAccess();
    }
    if !recording {
        CGRequestScreenCaptureAccess();
    }
    let mut missing = Vec::new();
    if !accessibility {
        missing.push("Accessibility");
    }
    if !recording {
        missing.push("Screen & System Audio Recording");
    }
    Err(format!(
        "turn on the terminal in System Settings > Privacy & Security > {}, then start the terminal again",
        missing.join(" and ")
    ))
}

fn run(options: &Options) -> Result<(), String> {
    permitted()?;
    let output = match &options.app_log {
        Some(path) => Stdio::from(
            std::fs::File::create(path)
                .map_err(|error| format!("cannot create {}: {error}", path.display()))?,
        ),
        None => Stdio::null(),
    };
    // The app's settings, recovery, logs and instance lock stay next to the
    // results, away from the user's; the C++ app reads the last two names.
    // Each run starts from none, as one app's leftovers can stop the next:
    // the C++ app takes the recovery path for a file, Ugurugu 3 for a folder.
    let own = options.csv.with_file_name(format!(
        "{}-app",
        options
            .csv
            .file_stem()
            .map_or("latency".into(), |stem| stem.to_string_lossy())
    ));
    if own.exists() {
        std::fs::remove_dir_all(&own)
            .map_err(|error| format!("cannot clear {}: {error}", own.display()))?;
    }
    let mut app = Command::new(&options.exe)
        .args(&options.document)
        .env("UGURUGU_SETTINGS_PATH", own.join("settings.json"))
        .env("UGURUGU_RECOVERY_PATH", own.join("recovery"))
        .env("UGURUGU_LOG_PATH", own.join("logs"))
        .env("UGURUGU_INSTANCE_LOCK_PATH", own.join("instance.lock"))
        .stdout(output)
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("cannot start {}: {error}", options.exe.display()))?;
    let pid = app.id() as i32;
    let result = measure(options, pid);
    // Killed rather than closed so that an app cannot write its settings.
    let _ = app.kill();
    let _ = app.wait();
    let (steps, idle) = result?;
    write_csv(&options.csv, &steps)?;
    report(&steps, idle);
    Ok(())
}

/// A measured step: when its input was posted and the frames that followed
/// before the next step.
#[derive(Debug, PartialEq)]
struct Step {
    /// Input to display and input to arrival at the probe, in milliseconds.
    response: Option<(f64, f64)>,
    extra_frames: usize,
}

/// Credits each frame to the last input posted at or before its display time.
fn steps(inputs: &[u64], end: u64, frames: &[Captured], ticks_per_ms: f64) -> Vec<Step> {
    inputs
        .iter()
        .enumerate()
        .map(|(index, &input)| {
            let next = inputs.get(index + 1).copied().unwrap_or(end);
            let mut within = frames.iter().filter(|frame| {
                frame.on_stroke() && frame.display >= input && frame.display < next
            });
            let response = within.next().map(|first| {
                (
                    (first.display - input) as f64 / ticks_per_ms,
                    first.arrival.saturating_sub(input) as f64 / ticks_per_ms,
                )
            });
            Step {
                response,
                extra_frames: within.count(),
            }
        })
        .collect()
}

/// Frames seen while idle, with the pointer beside the window and over it.
struct Idle {
    beside: usize,
    over: usize,
}

/// Drives the stroke and returns its steps and the frames seen while idle.
fn measure(options: &Options, pid: i32) -> Result<(Vec<Step>, Idle), String> {
    let started = Instant::now();
    let window = loop {
        if let Some(window) = find_window(pid)? {
            break window;
        }
        if started.elapsed() > Duration::from_secs(20) {
            return Err("the app showed no window".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    std::thread::sleep(Duration::from_secs(2));
    let [width, height] = options.window;
    place(pid, CGPoint::new(80.0, 60.0), CGSize::new(width, height))?;
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
    }
    std::thread::sleep(Duration::from_millis(1500));
    // The frame changed with `place`; the window number did not.
    // SAFETY: plain property read.
    let number = unsafe { window.windowID() };
    let same = || -> Result<Retained<SCWindow>, String> {
        let found = find_window(pid)?.ok_or("the app's window went away")?;
        // SAFETY: plain property reads.
        let (id, size) = unsafe { (found.windowID(), found.frame().size) };
        if id == number {
            Ok(found)
        } else {
            Err(format!(
                "the app's largest window is now {id}, {}x{} points, not {number}",
                size.width, size.height
            ))
        }
    };
    let mut window = same()?;
    // macOS may refuse the activation request of a background process, and
    // the first click in an inactive window may only activate it.
    let asked = Instant::now();
    // SAFETY: plain property read.
    if !unsafe { window.isActive() } {
        println!("click the title bar of the app's window to bring it to the front");
    }
    // SAFETY: plain property read.
    while !unsafe { window.isActive() } {
        if asked.elapsed() > Duration::from_secs(20) {
            return Err("the app's window did not become active".into());
        }
        std::thread::sleep(Duration::from_millis(250));
        window = same()?;
    }
    // SAFETY: plain property read.
    let frame = unsafe { window.frame() };
    println!(
        "window {}x{} points at ({}, {})",
        frame.size.width, frame.size.height, frame.origin.x, frame.origin.y
    );

    let center = CGPoint::new(
        frame.origin.x + frame.size.width / 2.0,
        frame.origin.y + frame.size.height / 2.0,
    );
    let radius = options.radius * frame.size.width.min(frame.size.height);
    let total = options.warmup + options.steps;
    let point = |step: usize| {
        let progress = step as f64 / total as f64;
        let angle = TAU * 2.0 * progress;
        let r = radius * (1.0 - 0.6 * progress);
        CGPoint::new(
            (center.x + r * angle.cos()).round(),
            (center.y + r * angle.sin()).round(),
        )
    };
    // Input goes wherever the cursor is, so never press a button over
    // another app.
    let stroke: Vec<CGPoint> = (0..=total).map(point).collect();
    if let Some(app) = covered(&window, &stroke)? {
        return Err(format!("{app} covers the app's window; no input sent"));
    }
    if let Some(key) = options.key {
        post_key(key);
        std::thread::sleep(Duration::from_millis(500));
    }

    let frames = Arc::new(Frames::default());
    let margin = STROKE_MARGIN;
    let (low, high) = stroke.iter().fold(
        (
            CGPoint::new(f64::MAX, f64::MAX),
            CGPoint::new(f64::MIN, f64::MIN),
        ),
        |(low, high), point| {
            (
                CGPoint::new(low.x.min(point.x), low.y.min(point.y)),
                CGPoint::new(high.x.max(point.x), high.y.max(point.y)),
            )
        },
    );
    let _ = frames.stroke.set([
        (low.x - margin - frame.origin.x) / frame.size.width,
        (low.y - margin - frame.origin.y) / frame.size.height,
        (high.x + margin - frame.origin.x) / frame.size.width,
        (high.y + margin - frame.origin.y) / frame.size.height,
    ]);
    let capture = start_capture(&window, frames.clone())?;
    let idle_frames = |place: &str| {
        std::thread::sleep(Duration::from_secs(1));
        let before = frames.count();
        let delivered = frames.delivered();
        std::thread::sleep(IDLE);
        let after = frames.count();
        let on_stroke = frames.list.lock().unwrap()[before..after]
            .iter()
            .filter(|frame| frame.on_stroke())
            .count();
        println!(
            "idle {place}: {} of {} complete frames changed, {on_stroke} on the stroke: {}",
            after - before,
            frames.delivered() - delivered,
            change_summary(&frames, before..after)
        );
        on_stroke
    };
    // Counted with the pointer beside the window and over it, to tell
    // whether a redraw while idle follows the pointer.
    post_mouse(
        CGEventType::MouseMoved,
        CGPoint::new(frame.origin.x - 40.0, center.y),
    );
    let idle_beside = idle_frames("beside");
    post_mouse(CGEventType::MouseMoved, point(0));
    let idle_over = idle_frames("over");
    if let Some(app) = covered(&window, &stroke)? {
        return Err(format!("{app} covers the app's window; no button pressed"));
    }

    let mut random = Lcg(0x5547_5550);
    let mut inputs = Vec::with_capacity(options.steps);
    let mut held = Held { at: point(0) };
    post_mouse(CGEventType::LeftMouseDown, held.at);
    std::thread::sleep(Duration::from_millis(300));
    for step in 1..=total {
        let input = now();
        if step > options.warmup {
            inputs.push(input);
        }
        held.at = point(step);
        post_mouse(CGEventType::LeftMouseDragged, held.at);
        frames.wait_from(input, RESPONSE_LIMIT);
        std::thread::sleep(Duration::from_millis(
            15 + (random.next_unit() * 25.0) as u64,
        ));
    }
    let end = now();
    drop(held);
    std::thread::sleep(Duration::from_millis(500));
    drop(capture);
    let captured = frames.list.lock().unwrap().clone();
    Ok((
        steps(&inputs, end, &captured, ticks_per_ms()),
        Idle {
            beside: idle_beside,
            over: idle_over,
        },
    ))
}

fn write_csv(path: &Path, steps: &[Step]) -> Result<(), String> {
    let mut text = String::from("step,input_to_display_ms,input_to_arrival_ms,extra_frames\n");
    for (index, step) in steps.iter().enumerate() {
        let (display, arrival) = step
            .response
            .map_or((String::new(), String::new()), |(display, arrival)| {
                (format!("{display:.3}"), format!("{arrival:.3}"))
            });
        text.push_str(&format!(
            "{},{display},{arrival},{}\n",
            index + 1,
            step.extra_frames
        ));
    }
    std::fs::write(path, text).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn report(steps: &[Step], idle: Idle) {
    let responses: Vec<(f64, f64)> = steps.iter().filter_map(|step| step.response).collect();
    println!(
        "steps={} misses={} extra_frames={} idle_frames={} beside, {} over the window, each in {}s",
        steps.len(),
        steps.len() - responses.len(),
        steps.iter().map(|step| step.extra_frames).sum::<usize>(),
        idle.beside,
        idle.over,
        IDLE.as_secs(),
    );
    summary(
        "input -> displayed",
        responses.iter().map(|&(display, _)| display).collect(),
    );
    summary(
        "input -> probe",
        responses.iter().map(|&(_, arrival)| arrival).collect(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_map_to_ansi_key_codes() {
        assert_eq!(key_code('a'), Some(0x00));
        assert_eq!(key_code('B'), Some(0x0B));
        assert_eq!(key_code('o'), Some(0x1F));
        assert_eq!(key_code('p'), Some(0x23));
        assert_eq!(key_code('k'), Some(0x28));
        assert_eq!(key_code('m'), Some(0x2E));
        assert_eq!(key_code('1'), None);
    }

    #[test]
    fn only_changes_inside_the_stroke_area_are_on_the_stroke() {
        let before = vec![0; 4 * 4 * 4];
        let mut after = before.clone();
        after[(3 * 4) * 4] = 9;
        let outside = differences(&before, &after, 4, [1, 1, 3, 3]).unwrap();
        assert_eq!(outside.bounds, [0, 3, 1, 1]);
        assert!(!outside.on_stroke);
        after[(2 * 4 + 2) * 4 + 1] = 5;
        let inside = differences(&before, &after, 4, [1, 1, 3, 3]).unwrap();
        assert_eq!(
            (inside.bounds, inside.pixels, inside.largest),
            ([0, 2, 3, 2], 2, 9)
        );
        assert!(inside.on_stroke);
        assert_eq!(differences(&before, &before, 4, [0, 0, 4, 4]), None);
    }

    #[test]
    fn frames_go_to_the_step_before_them() {
        let frame = |display, arrival| Captured {
            display,
            arrival,
            change: Ok(Change {
                bounds: [0, 0, 1, 1],
                pixels: 1,
                largest: 1,
                on_stroke: true,
            }),
        };
        let frames = [frame(5, 6), frame(12, 20), frame(15, 21), frame(40, 44)];
        let steps = steps(&[10, 30, 50], 70, &frames, 1.0);
        assert_eq!(
            steps,
            [
                Step {
                    response: Some((2.0, 10.0)),
                    extra_frames: 1,
                },
                Step {
                    response: Some((10.0, 14.0)),
                    extra_frames: 0,
                },
                Step {
                    response: None,
                    extra_frames: 0,
                },
            ]
        );
    }
}
