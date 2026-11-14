// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 NewfolderGames

//! Single-instance serialization and document hand-over on Linux.
//!
//! Starts serialize on an advisory `flock` file in `$XDG_RUNTIME_DIR/ugurugu-3`
//! (falling back to `/tmp/ugurugu-3-<uid>` with `0o700` mode and UID check).
//! A running window listens on a non-blocking Unix stream socket keyed by the
//! SHA-256 prefix of `key` (keeping `sun_path` well under 108 bytes) while
//! verifying the full `key` inside each frame, forwarding `XDG_ACTIVATION_TOKEN`
//! so Wayland compositors allow raising the existing window.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs::{DirBuilder, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

const FRAME_VERSION: u8 = 1;
const ACCEPTED: u8 = 1;
const REJECTED: u8 = 0;
const SEND_TIMEOUT: Duration = Duration::from_secs(5);
const START_TIMEOUT: Duration = Duration::from_secs(15);
const LONGEST_MESSAGE: usize = 1 << 16;

/// Held from a start until its window is ready to receive documents.
pub struct StartLock {
    _file: File,
}

impl StartLock {
    /// Waits up to 15 seconds for earlier starts using `name`. Returns `None`
    /// if the lock file cannot be opened or remains held past the timeout.
    pub fn take(name: &str) -> Option<Self> {
        let dir = runtime_dir().ok()?;
        let path = dir.join(format!("start-{}.lock", short_hash(name)));
        let file = open_lock_file(&path).ok()?;
        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            match try_flock_exclusive(&file) {
                Ok(true) => return Some(Self { _file: file }),
                Ok(false) => {
                    if Instant::now() >= deadline {
                        return None;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return None,
            }
        }
    }
}

/// Sends `document` (or an empty wake-up when `None`) to an existing window
/// listening for `key`, forwarding any `XDG_ACTIVATION_TOKEN` /
/// `DESKTOP_STARTUP_ID` from the shell. Returns `false` when no live listener
/// accepted the start.
pub fn hand_over(_class: &str, key: &str, document: Option<&Path>) -> bool {
    let token = std::env::var("XDG_ACTIVATION_TOKEN")
        .or_else(|_| std::env::var("DESKTOP_STARTUP_ID"))
        .ok()
        .unwrap_or_default();
    hand_over_with_token(key, document, &token)
}

fn hand_over_with_token(key: &str, document: Option<&Path>, token: &str) -> bool {
    let Ok(dir) = runtime_dir() else {
        return false;
    };
    let (sock_path, lock_path) = instance_paths(&dir, key);
    let Ok(frame) = encode_frame(key, document, token) else {
        return false;
    };

    let mut stream = None;
    for attempt in 0..5 {
        match UnixStream::connect(&sock_path) {
            Ok(connected) => {
                stream = Some(connected);
                break;
            }
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                ) =>
            {
                // Clean up a stale socket only while holding its companion
                // advisory lock so we never unlink a live listener's socket.
                if let Ok(lock_file) = open_lock_file(&lock_path) {
                    match try_flock_exclusive(&lock_file) {
                        Ok(true) => {
                            let _ = std::fs::remove_file(&sock_path);
                            return false;
                        }
                        Ok(false) if attempt < 4 => {
                            std::thread::sleep(Duration::from_millis(20));
                            continue;
                        }
                        _ => return false,
                    }
                }
                return false;
            }
            Err(_) => return false,
        }
    }

    let Some(mut stream) = stream else {
        return false;
    };
    let _ = stream.set_write_timeout(Some(SEND_TIMEOUT));
    if stream.write_all(&frame).is_err() || stream.flush().is_err() {
        return false;
    }

    // Use short read slices while pumping any `DocumentReceiver` installed on
    // this same thread so single-threaded callers and tests never deadlock.
    let _ = stream.set_read_timeout(Some(Duration::from_millis(15)));
    let deadline = Instant::now() + SEND_TIMEOUT;
    let mut ack = [0u8; 1];
    loop {
        match stream.read_exact(&mut ack) {
            Ok(()) => return ack[0] == ACCEPTED,
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) =>
            {
                poll_current_thread_receivers();
                if Instant::now() >= deadline {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
}

/// Raises and activates `hwnd` on Wayland or X11.
///
/// # Safety
/// `hwnd` must be a live window owned by the calling thread (or `0` in tests).
pub unsafe fn bring_to_front(hwnd: isize) {
    super::window::bring_to_front(hwnd);
}

struct PendingRequest {
    document: Option<PathBuf>,
    token: String,
    ack: Sender<bool>,
}

type ReceiveFn = Box<dyn FnMut(Option<PathBuf>) -> bool>;

struct LocalReceiver {
    id: u64,
    hwnd: isize,
    queue: Arc<Mutex<VecDeque<PendingRequest>>>,
    receive: ReceiveFn,
}

thread_local! {
    static LOCAL_RECEIVERS: RefCell<Vec<LocalReceiver>> = const { RefCell::new(Vec::new()) };
}

/// Dispatches any pending document hand-overs queued for [`DocumentReceiver`]s
/// installed on the calling (UI) thread.
pub fn poll_current_thread_receivers() {
    LOCAL_RECEIVERS.with(|cell| {
        let Ok(mut list) = cell.try_borrow_mut() else {
            return;
        };
        for entry in list.iter_mut() {
            loop {
                let next = entry
                    .queue
                    .lock()
                    .expect("document queue poisoned")
                    .pop_front();
                let Some(req) = next else {
                    break;
                };
                if !req.token.is_empty() {
                    super::window::set_activation_token(entry.hwnd, req.token);
                }
                let taken = (entry.receive)(req.document);
                let _ = req.ack.send(taken);
            }
        }
    });
}

/// Receives documents handed over to one window until dropped.
pub struct DocumentReceiver {
    /// `None` where there is no directory to listen in: later starts then
    /// end at the running lock without handing their documents over.
    _listening: Option<Listening>,
}

struct Listening {
    id: u64,
    sock_path: PathBuf,
    stop_efd: Arc<OwnedFd>,
    accept_thread: Option<JoinHandle<()>>,
    _lock_file: File,
}

impl DocumentReceiver {
    /// Installs a Unix socket listener for `key`. Incoming connections wake
    /// `hwnd`'s UI loop and run `receive` on the installing thread.
    ///
    /// # Safety
    /// Must be called and dropped on the thread that owns `hwnd`.
    pub unsafe fn install(
        hwnd: isize,
        key: &str,
        receive: impl FnMut(Option<PathBuf>) -> bool + 'static,
    ) -> Result<Self, &'static str> {
        Self::install_in(runtime_dir(), hwnd, key, receive)
    }

    fn install_in(
        dir: io::Result<PathBuf>,
        hwnd: isize,
        key: &str,
        receive: impl FnMut(Option<PathBuf>) -> bool + 'static,
    ) -> Result<Self, &'static str> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);

        let dir = match dir {
            Ok(dir) => dir,
            Err(error) => {
                tracing::warn!(
                    %error,
                    "no runtime directory; later starts cannot hand their documents over"
                );
                return Ok(Self { _listening: None });
            }
        };
        let (sock_path, lock_path) = instance_paths(&dir, key);
        let lock_file = open_lock_file(&lock_path).map_err(|_| "cannot open instance lock")?;
        if !try_flock_exclusive(&lock_file).map_err(|_| "cannot lock instance socket")? {
            return Err("another window already listens for this instance key");
        }

        // Safe because we hold the exclusive `flock` on `lock_file`.
        let _ = std::fs::remove_file(&sock_path);
        let listener = UnixListener::bind(&sock_path).map_err(|_| "cannot bind instance socket")?;
        let _ = std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600));
        listener
            .set_nonblocking(true)
            .map_err(|_| "cannot make instance socket non-blocking")?;

        // SAFETY: `EFD_CLOEXEC | EFD_NONBLOCK` creates a fresh kernel eventfd.
        let raw_efd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        if raw_efd < 0 {
            let _ = std::fs::remove_file(&sock_path);
            return Err("cannot create eventfd");
        }
        // SAFETY: `raw_efd >= 0` is a newly created descriptor.
        let stop_efd = Arc::new(unsafe { OwnedFd::from_raw_fd(raw_efd) });

        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        LOCAL_RECEIVERS.with(|cell| {
            cell.borrow_mut().push(LocalReceiver {
                id,
                hwnd,
                queue: queue.clone(),
                receive: Box::new(receive),
            });
        });

        let expected_key = key.to_owned();
        let worker_queue = queue;
        let worker_stop = stop_efd.clone();
        let accept_thread = std::thread::Builder::new()
            .name("ipc-accept".to_owned())
            .spawn(move || run_accept_loop(listener, worker_stop, hwnd, expected_key, worker_queue))
            .map_err(|_| "cannot spawn instance listener thread")?;

        Ok(Self {
            _listening: Some(Listening {
                id,
                sock_path,
                stop_efd,
                accept_thread: Some(accept_thread),
                _lock_file: lock_file,
            }),
        })
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        // Unlink the socket while still holding `_lock_file` so later starts
        // never connect to a closing socket.
        let _ = std::fs::remove_file(&self.sock_path);
        let one = 1u64.to_ne_bytes();
        // SAFETY: `stop_efd` is a live `OwnedFd`.
        let _ = unsafe { libc::write(self.stop_efd.as_raw_fd(), one.as_ptr().cast(), one.len()) };
        if let Some(thread) = self.accept_thread.take() {
            let _ = thread.join();
        }
        LOCAL_RECEIVERS.with(|cell| {
            if let Ok(mut list) = cell.try_borrow_mut() {
                list.retain(|entry| {
                    if entry.id == self.id {
                        if let Ok(mut q) = entry.queue.lock() {
                            while let Some(req) = q.pop_front() {
                                let _ = req.ack.send(false);
                            }
                        }
                        false
                    } else {
                        true
                    }
                });
            }
        });
    }
}

fn run_accept_loop(
    listener: UnixListener,
    stop_efd: Arc<OwnedFd>,
    hwnd: isize,
    expected_key: String,
    queue: Arc<Mutex<VecDeque<PendingRequest>>>,
) {
    loop {
        let mut fds = [
            libc::pollfd {
                fd: listener.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: stop_efd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: `fds` holds two open file descriptors for this call.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
        if ready < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if fds[1].revents & libc::POLLIN != 0 {
            break;
        }
        if fds[0].revents & libc::POLLIN == 0 {
            continue;
        }

        while let Ok((mut stream, _)) = listener.accept() {
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));

            let Some((document, token)) = read_and_match_frame(&mut stream, &expected_key) else {
                let _ = stream.write_all(&[REJECTED]);
                continue;
            };

            let (ack_tx, ack_rx) = mpsc::channel();
            {
                let mut guard = queue.lock().expect("document queue poisoned");
                guard.push_back(PendingRequest {
                    document,
                    token,
                    ack: ack_tx,
                });
            }
            super::window::wake_ui(hwnd);
            let taken = ack_rx.recv_timeout(SEND_TIMEOUT).unwrap_or(false);
            let _ = stream.write_all(&[if taken { ACCEPTED } else { REJECTED }]);
            let _ = stream.flush();
        }
    }
}

fn encode_frame(key: &str, document: Option<&Path>, token: &str) -> Result<Vec<u8>, &'static str> {
    let key_bytes = key.as_bytes();
    let doc_bytes = document.map(|p| p.as_os_str().as_bytes()).unwrap_or(&[]);
    let token_bytes = token.as_bytes();
    let payload_len = key_bytes
        .len()
        .saturating_add(doc_bytes.len())
        .saturating_add(token_bytes.len());
    if payload_len > LONGEST_MESSAGE {
        return Err("instance message exceeds maximum length");
    }

    let mut frame = Vec::with_capacity(13 + payload_len);
    frame.push(FRAME_VERSION);
    frame.extend_from_slice(&(key_bytes.len() as u32).to_le_bytes());
    frame.extend_from_slice(&(doc_bytes.len() as u32).to_le_bytes());
    frame.extend_from_slice(&(token_bytes.len() as u32).to_le_bytes());
    frame.extend_from_slice(key_bytes);
    frame.extend_from_slice(doc_bytes);
    frame.extend_from_slice(token_bytes);
    Ok(frame)
}

fn decode_frame_for_key(frame: &[u8], expected_key: &str) -> Option<(Option<PathBuf>, String)> {
    if frame.len() < 13 || frame[0] != FRAME_VERSION {
        return None;
    }
    let key_len = u32::from_le_bytes(frame[1..5].try_into().ok()?) as usize;
    let doc_len = u32::from_le_bytes(frame[5..9].try_into().ok()?) as usize;
    let token_len = u32::from_le_bytes(frame[9..13].try_into().ok()?) as usize;
    let total = key_len.checked_add(doc_len)?.checked_add(token_len)?;
    if total > LONGEST_MESSAGE || frame.len() != 13 + total {
        return None;
    }

    let key_end = 13 + key_len;
    let doc_end = key_end + doc_len;
    if &frame[13..key_end] != expected_key.as_bytes() {
        return None;
    }

    let doc_slice = &frame[key_end..doc_end];
    if doc_slice.contains(&0) {
        return None;
    }
    let document =
        (!doc_slice.is_empty()).then(|| PathBuf::from(OsString::from_vec(doc_slice.to_vec())));
    let token = std::str::from_utf8(&frame[doc_end..]).ok()?.to_owned();
    Some((document, token))
}

fn read_and_match_frame(
    stream: &mut UnixStream,
    expected_key: &str,
) -> Option<(Option<PathBuf>, String)> {
    let mut header = [0u8; 13];
    stream.read_exact(&mut header).ok()?;
    if header[0] != FRAME_VERSION {
        return None;
    }
    let key_len = u32::from_le_bytes(header[1..5].try_into().ok()?) as usize;
    let doc_len = u32::from_le_bytes(header[5..9].try_into().ok()?) as usize;
    let token_len = u32::from_le_bytes(header[9..13].try_into().ok()?) as usize;
    let payload_len = key_len.checked_add(doc_len)?.checked_add(token_len)?;
    if payload_len > LONGEST_MESSAGE {
        return None;
    }

    let mut full = Vec::with_capacity(13 + payload_len);
    full.extend_from_slice(&header);
    full.resize(13 + payload_len, 0);
    stream.read_exact(&mut full[13..]).ok()?;
    decode_frame_for_key(&full, expected_key)
}

fn runtime_dir() -> io::Result<PathBuf> {
    // SAFETY: `geteuid` has no preconditions and never fails.
    let euid = unsafe { libc::geteuid() };
    if let Some(xdg) = std::env::var_os("XDG_RUNTIME_DIR")
        && !xdg.is_empty()
    {
        let base = PathBuf::from(xdg);
        if base.is_absolute()
            && let Ok(dir) = ensure_private_dir(&base.join("ugurugu-3"), euid)
        {
            return Ok(dir);
        }
    }
    let fallback = PathBuf::from(format!("/tmp/ugurugu-3-{euid}"));
    ensure_private_dir(&fallback, euid)
}

fn ensure_private_dir(dir: &Path, expected_uid: u32) -> io::Result<PathBuf> {
    if !dir.exists() {
        let mut builder = DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(dir)?;
    }
    let meta = std::fs::symlink_metadata(dir)?;
    if meta.file_type().is_symlink() || !meta.is_dir() || meta.uid() != expected_uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "runtime directory is not owned by the current user",
        ));
    }
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    Ok(dir.to_path_buf())
}

fn instance_paths(dir: &Path, key: &str) -> (PathBuf, PathBuf) {
    let hash = short_hash(key);
    (
        dir.join(format!("inst-{hash}.sock")),
        dir.join(format!("inst-{hash}.lock")),
    )
}

fn short_hash(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    let mut out = String::with_capacity(16);
    for byte in &digest[..8] {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn open_lock_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC)
        .open(path)
}

fn try_flock_exclusive(file: &File) -> io::Result<bool> {
    // SAFETY: `file` is a valid open file descriptor.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc == 0 {
        return Ok(true);
    }
    let err = io::Error::last_os_error();
    if err.kind() == io::ErrorKind::WouldBlock {
        Ok(false)
    } else {
        Err(err)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    #[test]
    fn a_document_reaches_the_window_with_the_same_key_only() {
        let key = format!("ugurugu-instance-test-{}-a", std::process::id());
        let other_key = format!("ugurugu-instance-test-{}-b", std::process::id());
        let received = Rc::new(RefCell::new(Vec::new()));
        let sink = received.clone();

        // SAFETY: `0` is the test window handle on this thread, and `receiver`
        // is dropped on this thread below.
        let receiver = unsafe {
            DocumentReceiver::install(0, &key, move |path| {
                sink.borrow_mut().push(path);
                true
            })
        }
        .unwrap();

        let document = Path::new("/tmp/그림/고양이 1.ugurugu");
        assert!(hand_over("Ugurugu 3", &key, Some(document)));
        assert!(hand_over("Ugurugu 3", &key, None));
        assert!(!hand_over("Ugurugu 3", &other_key, Some(document)));
        assert!(!hand_over("Ugurugu 3", &other_key, None));
        assert_eq!(*received.borrow(), [Some(document.to_path_buf()), None]);

        drop(receiver);
        assert!(!hand_over("Ugurugu 3", &key, Some(document)));
        assert!(!hand_over("Ugurugu 3", &key, None));
    }

    #[test]
    fn a_window_without_a_runtime_directory_still_starts() {
        let key = format!("ugurugu-no-runtime-test-{}", std::process::id());
        let unusable = Err(io::Error::from(io::ErrorKind::PermissionDenied));
        let receiver = DocumentReceiver::install_in(unusable, 0, &key, |_| true);
        assert!(receiver.is_ok());
        assert!(!hand_over("Ugurugu 3", &key, None));
    }

    #[test]
    fn a_runtime_directory_must_be_a_real_directory_of_this_user() {
        // SAFETY: `geteuid` has no preconditions.
        let euid = unsafe { libc::geteuid() };
        let folder = std::env::temp_dir().join(format!("ugu-private-dir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();

        let real = folder.join("real");
        assert_eq!(ensure_private_dir(&real, euid).unwrap(), real);
        let mode = std::fs::metadata(&real).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);

        let link = folder.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(ensure_private_dir(&link, euid).is_err());
        let file = folder.join("file");
        std::fs::write(&file, b"").unwrap();
        assert!(ensure_private_dir(&file, euid).is_err());
        assert!(ensure_private_dir(&real, euid.wrapping_add(1)).is_err());

        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_frame_is_decoded_for_its_matching_key_only() {
        let frame_doc = encode_frame("k", Some(Path::new("/tmp/a.ugurugu")), "tok-1").unwrap();
        let frame_none = encode_frame("k", None, "").unwrap();
        assert_eq!(decode_frame_for_key(&[], "k"), None);
        assert_eq!(decode_frame_for_key(&frame_doc, "j"), None);
        assert_eq!(
            decode_frame_for_key(&frame_doc, "k"),
            Some((Some(PathBuf::from("/tmp/a.ugurugu")), "tok-1".to_owned()))
        );
        assert_eq!(
            decode_frame_for_key(&frame_none, "k"),
            Some((None, String::new()))
        );
    }

    #[test]
    fn starts_take_turns() {
        let name = format!("ugurugu-start-lock-test-{}", std::process::id());
        let first = StartLock::take(&name).unwrap();
        let waited = std::thread::spawn({
            let name = name.clone();
            move || {
                let started = Instant::now();
                drop(StartLock::take(&name).unwrap());
                started.elapsed()
            }
        });
        std::thread::sleep(Duration::from_millis(200));
        drop(first);
        assert!(waited.join().unwrap() >= Duration::from_millis(150));
    }

    #[test]
    fn stale_unowned_socket_is_removed_cleanly() {
        let key = format!("ugurugu-stale-test-{}", std::process::id());
        let dir = runtime_dir().unwrap();
        let (sock_path, _) = instance_paths(&dir, &key);
        let _ = std::fs::remove_file(&sock_path);
        {
            let _dangling = UnixListener::bind(&sock_path).unwrap();
        }
        assert!(sock_path.exists());
        assert!(!hand_over("Ugurugu 3", &key, None));
        assert!(!sock_path.exists());
    }
}
