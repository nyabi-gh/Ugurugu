// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 NewfolderGames

//! Active Linux window registration and foreign Wayland display service loop.
//!
//! `Session` in `apps/ugurugu/src/app.rs` installs a [`LinuxWindow`] before
//! starting the render thread and drops it before dropping the `winit::Window`.
//! That gives `dialog`, `clipboard`, and (in later slices) `instance`,
//! drag-and-drop, and tablet input exact Wayland or X11 handles without
//! modifying the cross-OS `hwnd: isize` call signatures.

use std::collections::HashMap;
use std::ffi::c_void;
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ugu_platform_types::clipboard::{Encoded, ReadError};
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::protocol::{
    wl_data_device::{self, WlDataDevice},
    wl_data_device_manager::{DndAction, WlDataDeviceManager},
    wl_data_offer::{self, WlDataOffer},
    wl_data_source::{self, WlDataSource},
    wl_keyboard::{self, WlKeyboard},
    wl_pointer::{self, WlPointer},
    wl_registry::{self, WlRegistry},
    wl_seat::{self, WlSeat},
    wl_surface::WlSurface,
    wl_touch::{self, WlTouch},
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
use wayland_protocols::xdg::activation::v1::client::{
    xdg_activation_token_v1::{self, XdgActivationTokenV1},
    xdg_activation_v1::XdgActivationV1,
};
use wayland_protocols::xdg::foreign::zv2::client::{
    zxdg_exported_v2::{self, ZxdgExportedV2},
    zxdg_exporter_v2::ZxdgExporterV2,
};

const PNG_MIME: &str = "image/png";
const TIFF_MIME: &str = "image/tiff";
const URI_LIST_MIME: &str = "text/uri-list";
const PIPE_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_URI_LIST_BYTES: usize = 256 * 1024;

type WakeFn = Arc<dyn Fn() + Send + Sync>;
type DropFileFn = Arc<dyn Fn(PathBuf) + Send + Sync>;

enum Entry {
    Wayland { worker: WaylandHandle, wake: WakeFn },
    X11 { window: u32, wake: WakeFn },
}

struct Registry {
    by_hwnd: HashMap<isize, Entry>,
    active: Option<isize>,
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        Mutex::new(Registry {
            by_hwnd: HashMap::new(),
            active: None,
        })
    })
}

/// RAII registration of the active Linux window on the UI thread.
pub struct LinuxWindow {
    hwnd: isize,
}

impl LinuxWindow {
    /// Registers a Wayland `(wl_display, wl_surface)` window and starts its
    /// dedicated `EventQueue` worker thread.
    ///
    /// # Safety
    /// `display` and `surface` must be valid Wayland client pointers that
    /// outlive the returned [`LinuxWindow`].
    pub unsafe fn install_wayland(
        hwnd: isize,
        display: *mut c_void,
        surface: *mut c_void,
        wake: impl Fn() + Send + Sync + 'static,
        on_drop: impl Fn(PathBuf) + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let wake: WakeFn = Arc::new(wake);
        let on_drop: DropFileFn = Arc::new(on_drop);
        // SAFETY: forwarded from the caller; `Session` drops `LinuxWindow`
        // (and joins its thread) before dropping `Arc<Window>`.
        let worker = unsafe { WaylandHandle::spawn(display, surface, on_drop)? };
        let mut reg = registry().lock().expect("window registry poisoned");
        reg.by_hwnd.insert(hwnd, Entry::Wayland { worker, wake });
        reg.active = Some(hwnd);
        Ok(Self { hwnd })
    }

    /// Registers an X11 window XID (`Xlib` or `Xcb`).
    pub fn install_x11(hwnd: isize, window: u32, wake: impl Fn() + Send + Sync + 'static) -> Self {
        let wake: WakeFn = Arc::new(wake);
        let mut reg = registry().lock().expect("window registry poisoned");
        reg.by_hwnd.insert(hwnd, Entry::X11 { window, wake });
        reg.active = Some(hwnd);
        Self { hwnd }
    }
}

impl Drop for LinuxWindow {
    fn drop(&mut self) {
        let removed = {
            let mut reg = registry().lock().expect("window registry poisoned");
            if reg.active == Some(self.hwnd) {
                reg.active = None;
            }
            reg.by_hwnd.remove(&self.hwnd)
        };
        if let Some(Entry::Wayland { mut worker, .. }) = removed {
            worker.shutdown();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DisplayKind {
    Wayland,
    X11,
    Headless,
}

pub(crate) fn active_display_kind() -> DisplayKind {
    let reg = registry().lock().expect("window registry poisoned");
    if let Some(hwnd) = reg.active
        && let Some(entry) = reg.by_hwnd.get(&hwnd)
    {
        return match entry {
            Entry::Wayland { .. } => DisplayKind::Wayland,
            Entry::X11 { .. } => DisplayKind::X11,
        };
    }
    drop(reg);
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        DisplayKind::Wayland
    } else if std::env::var_os("DISPLAY").is_some() {
        DisplayKind::X11
    } else {
        DisplayKind::Headless
    }
}

/// Wakes the `winit` event loop for `hwnd` (or the active window when `hwnd == 0`).
pub(crate) fn wake_ui(hwnd: isize) {
    let wake = {
        let reg = registry().lock().expect("window registry poisoned");
        let key = if hwnd != 0 { Some(hwnd) } else { reg.active };
        key.and_then(|k| match reg.by_hwnd.get(&k)? {
            Entry::Wayland { wake, .. } | Entry::X11 { wake, .. } => Some(wake.clone()),
        })
    };
    if let Some(wake) = wake {
        wake();
    }
}

/// Records an `XDG_ACTIVATION_TOKEN` / `DESKTOP_STARTUP_ID` forwarded from a
/// second start so [`bring_to_front`] can activate the Wayland surface.
pub(crate) fn set_activation_token(hwnd: isize, token: String) {
    let sender = {
        let reg = registry().lock().expect("window registry poisoned");
        let key = if hwnd != 0 { Some(hwnd) } else { reg.active };
        key.and_then(|k| match reg.by_hwnd.get(&k)? {
            Entry::Wayland { worker, .. } => Some(worker.sender.clone()),
            Entry::X11 { .. } => None,
        })
    };
    if let Some(sender) = sender
        && sender.send(Command::SetActivationToken(token)).is_ok()
    {
        let _ = sender.wake();
    }
}

/// Raises and activates `hwnd` on Wayland (`xdg_activation_v1`) or X11
/// (`_NET_ACTIVE_WINDOW` + `MapWindow`).
pub(crate) fn bring_to_front(hwnd: isize) {
    let target = {
        let reg = registry().lock().expect("window registry poisoned");
        let key = if hwnd != 0 { Some(hwnd) } else { reg.active };
        key.and_then(|k| match reg.by_hwnd.get(&k)? {
            Entry::Wayland { worker, .. } => Some(Ok(worker.sender.clone())),
            Entry::X11 { window, .. } => Some(Err(*window)),
        })
    };
    match target {
        Some(Ok(sender)) => {
            if sender.send(Command::Activate).is_ok() {
                let _ = sender.wake();
            }
        }
        Some(Err(xid)) => x11_bring_to_front(xid),
        None => {}
    }
}

fn x11_bring_to_front(window: u32) {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{
        ClientMessageEvent, ConfigureWindowAux, ConnectionExt as _, EventMask, StackMode,
    };

    let Ok((conn, screen_num)) = x11rb::connect(None) else {
        return;
    };
    let Some(screen) = conn.setup().roots.get(screen_num) else {
        return;
    };
    let root = screen.root;
    let _ = conn.map_window(window);
    let _ = conn.configure_window(
        window,
        &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
    );
    if let Ok(cookie) = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW")
        && let Ok(reply) = cookie.reply()
    {
        let msg =
            ClientMessageEvent::new(32, window, reply.atom, [1, x11rb::CURRENT_TIME, 0, 0, 0]);
        let _ = conn.send_event(
            false,
            root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            msg,
        );
    }
    let _ = conn.flush();
}

/// Formats `parent_window` for `org.freedesktop.portal.FileChooser` (`"wayland:<handle>"`,
/// `"x11:<hex>"`, or `""` when unattached).
pub(crate) fn portal_parent_window(owner: isize) -> String {
    let worker_or_x11 = {
        let reg = registry().lock().expect("window registry poisoned");
        let key = if owner != 0 { Some(owner) } else { reg.active };
        key.and_then(|k| match reg.by_hwnd.get(&k)? {
            Entry::Wayland { worker, .. } => Some(Ok(worker.sender.clone())),
            Entry::X11 { window, .. } => Some(Err(*window)),
        })
    };
    match worker_or_x11 {
        Some(Ok(sender)) => {
            let (tx, rx) = mpsc::channel();
            if sender.send(Command::ParentHandle(tx)).is_ok() {
                let _ = sender.wake();
                if let Ok(Some(handle)) = rx.recv_timeout(Duration::from_millis(500)) {
                    return format!("wayland:{handle}");
                }
            }
            String::new()
        }
        Some(Err(xid)) => format!("x11:{xid:x}"),
        None => String::new(),
    }
}

pub(crate) fn wayland_write_png(png: Arc<[u8]>) -> Result<(), String> {
    let sender = active_wayland_sender()
        .ok_or_else(|| "no active Wayland window for the clipboard".to_owned())?;
    let (tx, rx) = mpsc::channel();
    sender
        .send(Command::WritePng { png, reply: tx })
        .map_err(|_| "the Wayland clipboard worker stopped".to_owned())?;
    sender.wake()?;
    rx.recv_timeout(PIPE_TIMEOUT)
        .map_err(|_| "timed out writing to the Wayland clipboard".to_owned())?
}

pub(crate) fn wayland_read_encoded(limit: usize) -> Result<Option<Encoded>, ReadError> {
    let sender = active_wayland_sender().ok_or_else(|| {
        ReadError::System("no active Wayland window for the clipboard".to_owned())
    })?;
    let (tx, rx) = mpsc::channel();
    sender
        .send(Command::BeginRead { reply: tx })
        .map_err(|_| ReadError::System("the Wayland clipboard worker stopped".to_owned()))?;
    sender.wake().map_err(ReadError::System)?;
    let started = rx
        .recv_timeout(PIPE_TIMEOUT)
        .map_err(|_| ReadError::System("timed out querying the Wayland clipboard".to_owned()))??;
    let Some((read_fd, kind)) = started else {
        return Ok(None);
    };
    let bytes = read_pipe_bounded(read_fd, limit)?;
    Ok(Some(match kind {
        ImageMime::Png => Encoded::Png(bytes),
        ImageMime::Tiff => Encoded::Tiff(bytes),
    }))
}

fn active_wayland_sender() -> Option<CommandSender> {
    let reg = registry().lock().expect("window registry poisoned");
    let hwnd = reg.active?;
    match reg.by_hwnd.get(&hwnd)? {
        Entry::Wayland { worker, .. } => Some(worker.sender.clone()),
        Entry::X11 { .. } => None,
    }
}

#[derive(Clone, Copy)]
enum ImageMime {
    Png,
    Tiff,
}

impl ImageMime {
    fn as_str(self) -> &'static str {
        match self {
            Self::Png => PNG_MIME,
            Self::Tiff => TIFF_MIME,
        }
    }
}

enum Command {
    ParentHandle(Sender<Option<String>>),
    WritePng {
        png: Arc<[u8]>,
        reply: Sender<Result<(), String>>,
    },
    BeginRead {
        reply: Sender<Result<Option<(OwnedFd, ImageMime)>, ReadError>>,
    },
    SetActivationToken(String),
    Activate,
    FinishDnd(WlDataOffer),
    Shutdown,
}

#[derive(Clone)]
struct CommandSender {
    tx: Sender<Command>,
    wake_fd: Arc<OwnedFd>,
}

impl CommandSender {
    fn send(&self, cmd: Command) -> Result<(), mpsc::SendError<Command>> {
        self.tx.send(cmd)
    }

    fn wake(&self) -> Result<(), String> {
        let value = 1u64.to_ne_bytes();
        // SAFETY: `wake_fd` is a valid open `eventfd` owned by `Arc<OwnedFd>`.
        let written =
            unsafe { libc::write(self.wake_fd.as_raw_fd(), value.as_ptr().cast(), value.len()) };
        if written < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() != std::io::ErrorKind::WouldBlock {
                return Err(err.to_string());
            }
        }
        Ok(())
    }
}

struct WaylandHandle {
    sender: CommandSender,
    thread: Option<JoinHandle<()>>,
}

impl WaylandHandle {
    /// # Safety
    /// `display` and `surface` must be live Wayland client pointers for the
    /// lifetime of the spawned worker.
    unsafe fn spawn(
        display: *mut c_void,
        surface: *mut c_void,
        on_drop: DropFileFn,
    ) -> Result<Self, String> {
        // SAFETY: `EFD_CLOEXEC | EFD_NONBLOCK` creates a fresh kernel eventfd.
        let raw_efd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        if raw_efd < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        // SAFETY: `raw_efd >= 0` is a newly created file descriptor.
        let wake_fd = Arc::new(unsafe { OwnedFd::from_raw_fd(raw_efd) });

        // SAFETY: `display` is winit's live `*mut wl_display`.
        let backend = unsafe { Backend::from_foreign_display(display.cast()) };
        let conn = Connection::from_backend(backend);
        // SAFETY: `surface` is winit's live `*mut wl_surface` on `display`.
        let surface_id = unsafe { ObjectId::from_ptr(WlSurface::interface(), surface.cast()) }
            .map_err(|error| error.to_string())?;
        let wl_surface =
            WlSurface::from_id(&conn, surface_id).map_err(|error| error.to_string())?;

        let (tx, rx) = mpsc::channel();
        let sender = CommandSender {
            tx,
            wake_fd: wake_fd.clone(),
        };
        let worker_sender = sender.clone();
        let thread = std::thread::Builder::new()
            .name("wayland-services".to_owned())
            .spawn(move || {
                run_wayland_worker(conn, wl_surface, rx, wake_fd, worker_sender, on_drop)
            })
            .map_err(|error| error.to_string())?;

        Ok(Self {
            sender,
            thread: Some(thread),
        })
    }

    fn shutdown(&mut self) {
        let _ = self.sender.send(Command::Shutdown);
        let _ = self.sender.wake();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Default)]
struct SeatSlot {
    keyboard: Option<WlKeyboard>,
    pointer: Option<WlPointer>,
    touch: Option<WlTouch>,
    data_device: Option<WlDataDevice>,
    latest_serial: u32,
    has_focus: bool,
    selection_offer: Option<ObjectId>,
    dnd_offer: Option<(ObjectId, bool)>,
}

struct WaylandState {
    surface: WlSurface,
    sender: CommandSender,
    on_drop: DropFileFn,
    data_manager: Option<WlDataDeviceManager>,
    exporter: Option<ZxdgExporterV2>,
    exported: Option<ZxdgExportedV2>,
    exported_handle: Option<String>,
    activation: Option<XdgActivationV1>,
    pending_token: Option<String>,
    seats: HashMap<ObjectId, (WlSeat, SeatSlot)>,
    latest_seat: Option<ObjectId>,
    offer_mimes: HashMap<ObjectId, (WlDataOffer, Vec<String>)>,
    active_sources: Vec<(WlDataSource, Arc<[u8]>)>,
    exit: bool,
}

impl WaylandState {
    fn new(surface: WlSurface, sender: CommandSender, on_drop: DropFileFn) -> Self {
        Self {
            surface,
            sender,
            on_drop,
            data_manager: None,
            exporter: None,
            exported: None,
            exported_handle: None,
            activation: None,
            pending_token: None,
            seats: HashMap::new(),
            latest_seat: None,
            offer_mimes: HashMap::new(),
            active_sources: Vec::new(),
            exit: false,
        }
    }

    fn record_serial(&mut self, seat_id: &ObjectId, serial: u32) {
        if let Some((_, slot)) = self.seats.get_mut(seat_id) {
            slot.latest_serial = serial;
            self.latest_seat = Some(seat_id.clone());
        }
    }

    fn active_seat_mut(&mut self) -> Option<&mut SeatSlot> {
        if let Some(id) = &self.latest_seat
            && self.seats.contains_key(id)
        {
            let id = id.clone();
            return self.seats.get_mut(&id).map(|(_, slot)| slot);
        }
        self.seats
            .values_mut()
            .map(|(_, slot)| slot)
            .find(|slot| slot.has_focus || slot.data_device.is_some())
    }

    fn active_seat_with_serial(&self) -> Option<(&WlSeat, u32)> {
        if let Some(id) = &self.latest_seat
            && let Some((seat, slot)) = self.seats.get(id)
            && slot.latest_serial != 0
        {
            return Some((seat, slot.latest_serial));
        }
        self.seats
            .values()
            .find(|(_, slot)| slot.latest_serial != 0)
            .map(|(seat, slot)| (seat, slot.latest_serial))
    }

    fn cleanup(&mut self) {
        for (source, _) in self.active_sources.drain(..) {
            source.destroy();
        }
        for (_, (offer, _)) in self.offer_mimes.drain() {
            offer.destroy();
        }
        for (_, (_, mut slot)) in self.seats.drain() {
            if let Some(dev) = slot.data_device.take()
                && dev.version() >= 2
            {
                dev.release();
            }
            if let Some(kb) = slot.keyboard.take()
                && kb.version() >= 3
            {
                kb.release();
            }
            if let Some(ptr) = slot.pointer.take()
                && ptr.version() >= 3
            {
                ptr.release();
            }
            if let Some(touch) = slot.touch.take()
                && touch.version() >= 3
            {
                touch.release();
            }
        }
        if let Some(exported) = self.exported.take() {
            exported.destroy();
        }
        if let Some(exporter) = self.exporter.take() {
            exporter.destroy();
        }
        if let Some(activation) = self.activation.take() {
            activation.destroy();
        }
    }
}

fn run_wayland_worker(
    conn: Connection,
    surface: WlSurface,
    rx: Receiver<Command>,
    wake_fd: Arc<OwnedFd>,
    sender: CommandSender,
    on_drop: DropFileFn,
) {
    let mut queue: EventQueue<WaylandState> = conn.new_event_queue();
    let qh = queue.handle();
    let mut state = WaylandState::new(surface, sender, on_drop);

    let _registry = conn.display().get_registry(&qh, ());
    // Two roundtrips bind globals (`wl_seat`, `wl_data_device_manager`,
    // `zxdg_exporter_v2`) and receive initial seat capabilities + exported
    // window handle.
    let _ = queue.roundtrip(&mut state);
    let _ = queue.roundtrip(&mut state);

    while !state.exit {
        let _ = queue.dispatch_pending(&mut state);
        let _ = conn.flush();

        while let Ok(cmd) = rx.try_recv() {
            handle_command(cmd, &conn, &qh, &mut state);
            if state.exit {
                break;
            }
        }
        if state.exit {
            break;
        }
        let _ = conn.flush();

        let guard = loop {
            if let Some(guard) = queue.prepare_read() {
                break Some(guard);
            }
            if queue.dispatch_pending(&mut state).is_err() {
                break None;
            }
        };
        let Some(guard) = guard else {
            break;
        };

        let wl_fd = guard.connection_fd().as_raw_fd();
        let mut fds = [
            libc::pollfd {
                fd: wl_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: wake_fd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: `fds` is a live 2-element stack array of open descriptors.
        let polled = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
        if polled < 0 {
            drop(guard);
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }

        if fds[0].revents & (libc::POLLIN | libc::POLLERR | libc::POLLHUP) != 0 {
            let _ = guard.read();
        } else {
            drop(guard);
        }

        if fds[1].revents & libc::POLLIN != 0 {
            let mut buf = [0u8; 8];
            // SAFETY: `wake_fd` is open and non-blocking; `buf` is 8 stack bytes.
            unsafe {
                while libc::read(wake_fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) > 0 {}
            }
        }
    }

    state.cleanup();
    let _ = conn.flush();
}

fn handle_command(
    cmd: Command,
    conn: &Connection,
    qh: &QueueHandle<WaylandState>,
    state: &mut WaylandState,
) {
    match cmd {
        Command::ParentHandle(reply) => {
            let _ = reply.send(state.exported_handle.clone());
        }
        Command::WritePng { png, reply } => {
            let result = (|| {
                let manager =
                    state.data_manager.as_ref().cloned().ok_or_else(|| {
                        "Wayland compositor lacks wl_data_device_manager".to_owned()
                    })?;
                let slot = state
                    .active_seat_mut()
                    .ok_or_else(|| "no Wayland seat available for clipboard".to_owned())?;
                let device = slot
                    .data_device
                    .as_ref()
                    .cloned()
                    .ok_or_else(|| "no Wayland data device on seat".to_owned())?;
                let serial = slot.latest_serial;

                let source = manager.create_data_source(qh, ());
                source.offer(PNG_MIME.to_owned());
                device.set_selection(Some(&source), serial);
                // Keep previous cancelled sources cleaned up while retaining the active one.
                if state.active_sources.len() >= 4 {
                    let (old, _) = state.active_sources.remove(0);
                    old.destroy();
                }
                state.active_sources.push((source, png));
                conn.flush().map_err(|error| error.to_string())?;
                Ok(())
            })();
            let _ = reply.send(result);
        }
        Command::BeginRead { reply } => {
            let result = (|| {
                let offer_id = state
                    .active_seat_mut()
                    .and_then(|slot| slot.selection_offer.clone());
                let Some(offer_id) = offer_id else {
                    return Ok(None);
                };
                let Some((offer, mimes)) = state.offer_mimes.get(&offer_id) else {
                    return Ok(None);
                };
                let chosen = if mimes.iter().any(|m| m == PNG_MIME) {
                    Some(ImageMime::Png)
                } else if mimes.iter().any(|m| m == TIFF_MIME) {
                    Some(ImageMime::Tiff)
                } else {
                    None
                };
                let Some(kind) = chosen else {
                    return Ok(None);
                };
                let (read_fd, write_fd) = make_pipe().map_err(ReadError::System)?;
                offer.receive(kind.as_str().to_owned(), write_fd.as_fd());
                conn.flush()
                    .map_err(|error| ReadError::System(error.to_string()))?;
                drop(write_fd);
                Ok(Some((read_fd, kind)))
            })();
            let _ = reply.send(result);
        }
        Command::SetActivationToken(token) => {
            if !token.is_empty() {
                state.pending_token = Some(token);
            }
        }
        Command::Activate => {
            let Some(activation) = state.activation.as_ref().cloned() else {
                return;
            };
            if let Some(token) = state.pending_token.take() {
                activation.activate(token, &state.surface);
                let _ = conn.flush();
                return;
            }
            if let Some((seat, serial)) = state.active_seat_with_serial() {
                let req = activation.get_activation_token(qh, ());
                req.set_serial(serial, seat);
                req.set_surface(&state.surface);
                req.commit();
                let _ = conn.flush();
            }
        }
        Command::FinishDnd(offer) => {
            if offer.version() >= 3 {
                offer.finish();
            }
            offer.destroy();
            let _ = conn.flush();
        }
        Command::Shutdown => {
            state.exit = true;
        }
    }
}

fn make_pipe() -> Result<(OwnedFd, OwnedFd), String> {
    let mut fds = [-1, -1];
    // SAFETY: `fds` points to 2 `c_int`s; `O_CLOEXEC` prevents leaking across exec.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // SAFETY: `pipe2` initialized both file descriptors on success.
    unsafe { Ok((OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1]))) }
}

fn set_nonblocking(fd: i32) -> std::io::Result<()> {
    // SAFETY: `F_GETFL` / `F_SETFL` operate on a valid open file descriptor.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

fn read_pipe_bounded(read_fd: OwnedFd, limit: usize) -> Result<Vec<u8>, ReadError> {
    set_nonblocking(read_fd.as_raw_fd()).map_err(|error| ReadError::System(error.to_string()))?;
    let mut file = std::fs::File::from(read_fd);
    let mut out = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    let deadline = Instant::now() + PIPE_TIMEOUT;

    loop {
        match file.read(&mut chunk) {
            Ok(0) => return Ok(out),
            Ok(n) => {
                if out.len().saturating_add(n) > limit {
                    return Err(ReadError::TooLarge);
                }
                out.extend_from_slice(&chunk[..n]);
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(ReadError::System(
                        "timed out reading clipboard pipe".to_owned(),
                    ));
                }
                let timeout_ms = i32::try_from(remaining.as_millis()).unwrap_or(i32::MAX);
                let mut pfd = libc::pollfd {
                    fd: file.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                // SAFETY: `pfd` points to a single live `pollfd` for `file`.
                let ready = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
                if ready == 0 {
                    return Err(ReadError::System(
                        "timed out reading clipboard pipe".to_owned(),
                    ));
                }
                if ready < 0 {
                    let poll_err = std::io::Error::last_os_error();
                    if poll_err.kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    return Err(ReadError::System(poll_err.to_string()));
                }
            }
            Err(err) => return Err(ReadError::System(err.to_string())),
        }
    }
}

fn spawn_pipe_writer(write_fd: OwnedFd, bytes: Arc<[u8]>) {
    let _ = std::thread::Builder::new()
        .name("wayland-clip-write".to_owned())
        .spawn(move || {
            let mut file = std::fs::File::from(write_fd);
            let _ = file.write_all(&bytes);
        });
}

fn spawn_dnd_reader(
    read_fd: OwnedFd,
    offer: WlDataOffer,
    sender: CommandSender,
    on_drop: DropFileFn,
) {
    let _ = std::thread::Builder::new()
        .name("wayland-dnd-read".to_owned())
        .spawn(move || {
            let paths = read_pipe_bounded(read_fd, MAX_URI_LIST_BYTES)
                .ok()
                .map(|bytes| parse_uri_list(&bytes))
                .unwrap_or_default();
            if sender.send(Command::FinishDnd(offer)).is_ok() {
                let _ = sender.wake();
            }
            for path in paths {
                on_drop(path);
            }
        });
}

/// Decodes an RFC 2483 `text/uri-list` payload into local filesystem paths,
/// ignoring `#` comments, blank lines, and non-`file://` URIs.
pub(crate) fn parse_uri_list(bytes: &[u8]) -> Vec<PathBuf> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Vec::new();
    };
    text.lines()
        .map(|line| line.trim_end_matches('\r').trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(super::folder::file_uri_to_path)
        .collect()
}

fn supports_uri_list(mimes: &[String]) -> bool {
    mimes.iter().any(|mime| {
        let base = mime.split(';').next().unwrap_or(mime).trim();
        base.eq_ignore_ascii_case(URI_LIST_MIME)
    })
}

impl Dispatch<WlRegistry, ()> for WaylandState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        else {
            return;
        };
        match interface.as_str() {
            "wl_data_device_manager" if state.data_manager.is_none() => {
                let mgr = registry.bind::<WlDataDeviceManager, _, _>(name, version.min(3), qh, ());
                for (seat_id, (seat, slot)) in &mut state.seats {
                    if slot.data_device.is_none() {
                        slot.data_device = Some(mgr.get_data_device(seat, qh, seat_id.clone()));
                    }
                }
                state.data_manager = Some(mgr);
            }
            "wl_seat" => {
                let seat = registry.bind::<WlSeat, _, _>(name, version.min(5), qh, ());
                let seat_id = seat.id();
                let mut slot = SeatSlot::default();
                if let Some(mgr) = &state.data_manager {
                    slot.data_device = Some(mgr.get_data_device(&seat, qh, seat_id.clone()));
                }
                if state.latest_seat.is_none() {
                    state.latest_seat = Some(seat_id.clone());
                }
                state.seats.insert(seat_id, (seat, slot));
            }
            "zxdg_exporter_v2" if state.exporter.is_none() => {
                let exporter = registry.bind::<ZxdgExporterV2, _, _>(name, version.min(1), qh, ());
                let exported = exporter.export_toplevel(&state.surface, qh, ());
                state.exporter = Some(exporter);
                state.exported = Some(exported);
            }
            "xdg_activation_v1" if state.activation.is_none() => {
                state.activation =
                    Some(registry.bind::<XdgActivationV1, _, _>(name, version.min(1), qh, ()));
            }
            _ => {}
        }
    }
}

impl Dispatch<WlDataDeviceManager, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlDataDeviceManager,
        _: <WlDataDeviceManager as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZxdgExporterV2, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZxdgExporterV2,
        _: <ZxdgExporterV2 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZxdgExportedV2, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _: &ZxdgExportedV2,
        event: zxdg_exported_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zxdg_exported_v2::Event::Handle { handle } = event {
            state.exported_handle = Some(handle);
        }
    }
}

impl Dispatch<XdgActivationV1, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &XdgActivationV1,
        _: <XdgActivationV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<XdgActivationTokenV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        token_obj: &XdgActivationTokenV1,
        event: xdg_activation_token_v1::Event,
        _: &(),
        conn: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_activation_token_v1::Event::Done { token } = event {
            if let Some(activation) = &state.activation {
                activation.activate(token, &state.surface);
            }
            token_obj.destroy();
            let _ = conn.flush();
        }
    }
}

impl Dispatch<WlSeat, ()> for WaylandState {
    fn event(
        state: &mut Self,
        seat: &WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(caps),
        } = event
        else {
            return;
        };
        let seat_id = seat.id();
        let Some((_, slot)) = state.seats.get_mut(&seat_id) else {
            return;
        };
        if caps.contains(wl_seat::Capability::Keyboard) && slot.keyboard.is_none() {
            slot.keyboard = Some(seat.get_keyboard(qh, seat_id.clone()));
        } else if !caps.contains(wl_seat::Capability::Keyboard)
            && let Some(kb) = slot.keyboard.take()
            && kb.version() >= 3
        {
            kb.release();
        }

        if caps.contains(wl_seat::Capability::Pointer) && slot.pointer.is_none() {
            slot.pointer = Some(seat.get_pointer(qh, seat_id.clone()));
        } else if !caps.contains(wl_seat::Capability::Pointer)
            && let Some(ptr) = slot.pointer.take()
            && ptr.version() >= 3
        {
            ptr.release();
        }

        if caps.contains(wl_seat::Capability::Touch) && slot.touch.is_none() {
            slot.touch = Some(seat.get_touch(qh, seat_id));
        } else if !caps.contains(wl_seat::Capability::Touch)
            && let Some(touch) = slot.touch.take()
            && touch.version() >= 3
        {
            touch.release();
        }
    }
}

impl Dispatch<WlKeyboard, ObjectId> for WaylandState {
    fn event(
        state: &mut Self,
        _: &WlKeyboard,
        event: wl_keyboard::Event,
        seat_id: &ObjectId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Enter { serial, .. } => {
                if let Some((_, slot)) = state.seats.get_mut(seat_id) {
                    slot.has_focus = true;
                }
                state.record_serial(seat_id, serial);
            }
            wl_keyboard::Event::Leave { serial, .. } => {
                if let Some((_, slot)) = state.seats.get_mut(seat_id) {
                    slot.has_focus = false;
                }
                state.record_serial(seat_id, serial);
            }
            wl_keyboard::Event::Key { serial, .. } => {
                state.record_serial(seat_id, serial);
            }
            _ => {}
        }
    }
}

impl Dispatch<WlPointer, ObjectId> for WaylandState {
    fn event(
        state: &mut Self,
        _: &WlPointer,
        event: wl_pointer::Event,
        seat_id: &ObjectId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_pointer::Event::Enter { serial, .. } | wl_pointer::Event::Button { serial, .. } =
            event
        {
            state.record_serial(seat_id, serial);
        }
    }
}

impl Dispatch<WlTouch, ObjectId> for WaylandState {
    fn event(
        state: &mut Self,
        _: &WlTouch,
        event: wl_touch::Event,
        seat_id: &ObjectId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_touch::Event::Down { serial, .. } = event {
            state.record_serial(seat_id, serial);
        }
    }
}

impl Dispatch<WlDataDevice, ObjectId> for WaylandState {
    fn event(
        state: &mut Self,
        _: &WlDataDevice,
        event: wl_data_device::Event,
        seat_id: &ObjectId,
        conn: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_device::Event::DataOffer { id } => {
                state.offer_mimes.insert(id.id(), (id, Vec::new()));
            }
            wl_data_device::Event::Selection { id } => {
                let prev = state.seats.get_mut(seat_id).and_then(|(_, slot)| {
                    std::mem::replace(&mut slot.selection_offer, id.as_ref().map(Proxy::id))
                });
                if let Some(old_id) = prev
                    && id.as_ref().map(Proxy::id) != Some(old_id.clone())
                    && let Some((old_offer, _)) = state.offer_mimes.remove(&old_id)
                {
                    old_offer.destroy();
                }
            }
            wl_data_device::Event::Enter {
                serial,
                surface,
                id,
                ..
            } => {
                state.record_serial(seat_id, serial);
                if let Some((old_id, _)) = state
                    .seats
                    .get_mut(seat_id)
                    .and_then(|(_, slot)| slot.dnd_offer.take())
                    && let Some((old_offer, _)) = state.offer_mimes.remove(&old_id)
                {
                    old_offer.destroy();
                }
                let Some(offer) = id else {
                    return;
                };
                let offer_id = offer.id();
                if surface != state.surface {
                    offer.destroy();
                    state.offer_mimes.remove(&offer_id);
                    return;
                }
                let accepted = state
                    .offer_mimes
                    .get(&offer_id)
                    .is_some_and(|(_, mimes)| supports_uri_list(mimes));
                if offer.version() >= 3 {
                    let action = if accepted {
                        DndAction::Copy
                    } else {
                        DndAction::None
                    };
                    offer.set_actions(action, action);
                }
                offer.accept(serial, accepted.then(|| URI_LIST_MIME.to_owned()));
                if let Some((_, slot)) = state.seats.get_mut(seat_id) {
                    slot.dnd_offer = Some((offer_id, accepted));
                }
                let _ = conn.flush();
            }
            wl_data_device::Event::Leave => {
                if let Some((offer_id, _)) = state
                    .seats
                    .get_mut(seat_id)
                    .and_then(|(_, slot)| slot.dnd_offer.take())
                    && let Some((offer, _)) = state.offer_mimes.remove(&offer_id)
                {
                    offer.destroy();
                }
            }
            wl_data_device::Event::Drop => {
                let Some((offer_id, accepted)) = state
                    .seats
                    .get_mut(seat_id)
                    .and_then(|(_, slot)| slot.dnd_offer.take())
                else {
                    return;
                };
                let Some((offer, _)) = state.offer_mimes.remove(&offer_id) else {
                    return;
                };
                if !accepted {
                    offer.destroy();
                    return;
                }
                let Ok((read_fd, write_fd)) = make_pipe() else {
                    offer.destroy();
                    return;
                };
                offer.receive(URI_LIST_MIME.to_owned(), write_fd.as_fd());
                let _ = conn.flush();
                drop(write_fd);
                spawn_dnd_reader(read_fd, offer, state.sender.clone(), state.on_drop.clone());
            }
            _ => {}
        }
    }

    fn event_created_child(
        opcode: u16,
        qhandle: &QueueHandle<Self>,
    ) -> Arc<dyn wayland_backend::client::ObjectData> {
        match opcode {
            wl_data_device::EVT_DATA_OFFER_OPCODE => qhandle.make_data::<WlDataOffer, ()>(()),
            _ => unreachable!("unexpected wl_data_device child opcode {opcode}"),
        }
    }
}

impl Dispatch<WlDataOffer, ()> for WaylandState {
    fn event(
        state: &mut Self,
        offer: &WlDataOffer,
        event: wl_data_offer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_data_offer::Event::Offer { mime_type } = event
            && let Some((_, mimes)) = state.offer_mimes.get_mut(&offer.id())
            && !mimes.contains(&mime_type)
        {
            mimes.push(mime_type);
        }
    }
}

impl Dispatch<WlDataSource, ()> for WaylandState {
    fn event(
        state: &mut Self,
        source: &WlDataSource,
        event: wl_data_source::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_source::Event::Send { mime_type, fd } => {
                if mime_type == PNG_MIME
                    && let Some((_, bytes)) = state
                        .active_sources
                        .iter()
                        .find(|(s, _)| s.id() == source.id())
                {
                    spawn_pipe_writer(fd, bytes.clone());
                }
            }
            wl_data_source::Event::Cancelled => {
                if let Some(pos) = state
                    .active_sources
                    .iter()
                    .position(|(s, _)| s.id() == source.id())
                {
                    let (dead, _) = state.active_sources.remove(pos);
                    dead.destroy();
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{parse_uri_list, supports_uri_list};

    #[test]
    fn uri_list_decodes_percent_encoded_local_files_and_skips_comments_and_web_urls() {
        let payload = concat!(
            "# Dolphin drag payload\r\n",
            "file:///tmp/%EA%B3%A0%EC%96%91%EC%9D%B4%201.ugurugu\r\n",
            "\r\n",
            "https://ugurugu.dev/ignored.png\r\n",
            "file://localhost/var/tmp/frame%232.png\n",
        );
        assert_eq!(
            parse_uri_list(payload.as_bytes()),
            vec![
                PathBuf::from("/tmp/고양이 1.ugurugu"),
                PathBuf::from("/var/tmp/frame#2.png"),
            ]
        );
    }

    #[test]
    fn uri_list_mime_matches_charset_parameters() {
        assert!(supports_uri_list(&["text/uri-list".to_owned()]));
        assert!(supports_uri_list(&[
            "text/plain".to_owned(),
            "text/uri-list;charset=utf-8".to_owned(),
        ]));
        assert!(!supports_uri_list(&["image/png".to_owned()]));
    }
}
