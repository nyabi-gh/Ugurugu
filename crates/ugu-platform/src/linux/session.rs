// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 NewfolderGames

//! Hears that the Linux desktop session or service manager is ending the
//! process: `SIGTERM` / `SIGHUP` signals and `systemd-logind`'s
//! `PrepareForShutdown` delay inhibitor lock.
//!
//! Fault signals (`SIGSEGV`, `SIGBUS`, `SIGILL`, `SIGFPE`, `SIGABRT`) remain
//! owned exclusively by `unix::crash`.

use std::ffi::c_int;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;

use futures_lite::StreamExt;

struct SignalPipe {
    efd: OwnedFd,
}

static SIGNAL_PIPE: OnceLock<SignalPipe> = OnceLock::new();
static LAST_SIGNAL: AtomicI32 = AtomicI32::new(0);
static ACTIVE_LISTENERS: AtomicI32 = AtomicI32::new(0);

extern "C" fn term_handler(signal: c_int) {
    // If no `SessionEnd` guard is currently active, restore default behavior
    // and re-raise immediately using async-signal-safe calls only.
    if ACTIVE_LISTENERS.load(Ordering::Relaxed) <= 0 {
        // SAFETY: `SIG_DFL` and `raise` are async-signal-safe.
        unsafe {
            let mut dflt: libc::sigaction = std::mem::zeroed();
            dflt.sa_sigaction = libc::SIG_DFL;
            libc::sigemptyset(&mut dflt.sa_mask);
            libc::sigaction(signal, &dflt, std::ptr::null_mut());
            libc::raise(signal);
        }
        return;
    }

    LAST_SIGNAL.store(signal, Ordering::Relaxed);
    if let Some(pipe) = SIGNAL_PIPE.get() {
        let one = 1u64.to_ne_bytes();
        // SAFETY: `write` on an open `eventfd` is async-signal-safe.
        let _ = unsafe { libc::write(pipe.efd.as_raw_fd(), one.as_ptr().cast(), one.len()) };
    }
}

fn ensure_signal_pipe() -> Result<i32, &'static str> {
    if let Some(pipe) = SIGNAL_PIPE.get() {
        return Ok(pipe.efd.as_raw_fd());
    }
    // SAFETY: `EFD_CLOEXEC | EFD_NONBLOCK` creates a kernel eventfd.
    let raw = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
    if raw < 0 {
        return Err("cannot create signal eventfd");
    }
    // SAFETY: `raw >= 0` is a fresh file descriptor.
    let efd = unsafe { OwnedFd::from_raw_fd(raw) };
    let fd = efd.as_raw_fd();
    if SIGNAL_PIPE.set(SignalPipe { efd }).is_ok() {
        // SAFETY: installs async-signal-safe `term_handler` on `SIGTERM` and
        // `SIGHUP` only, leaving `unix::crash` fault signals untouched.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = term_handler as *const () as usize;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut());
            libc::sigaction(libc::SIGHUP, &action, std::ptr::null_mut());
        }
    }
    Ok(SIGNAL_PIPE.get().map_or(fd, |p| p.efd.as_raw_fd()))
}

/// Calls back as the Linux session ends, until dropped.
pub struct SessionEnd {
    stop_efd: Arc<OwnedFd>,
    inhibitor: Arc<Mutex<Option<zbus::zvariant::OwnedFd>>>,
    thread: Option<JoinHandle<()>>,
}

impl SessionEnd {
    /// Installs `SIGTERM`/`SIGHUP` watchers and a best-effort `systemd-logind`
    /// `"shutdown"` delay inhibitor lock. `ending` runs once before the
    /// inhibitor lock is released or the termination signal is re-raised.
    ///
    /// # Safety
    /// None on Linux; `unsafe` to match the cross-OS signature.
    pub unsafe fn install(
        _hwnd: isize,
        mut ending: impl FnMut() + Send + 'static,
    ) -> Result<Self, &'static str> {
        let sig_fd = ensure_signal_pipe()?;
        // Drain any stale count left before this `SessionEnd` was installed.
        drain_eventfd(sig_fd);
        LAST_SIGNAL.store(0, Ordering::SeqCst);
        ACTIVE_LISTENERS.fetch_add(1, Ordering::SeqCst);

        let stop_efd = Arc::new(new_eventfd()?);
        let logind_efd = Arc::new(new_eventfd()?);
        let inhibitor: Arc<Mutex<Option<zbus::zvariant::OwnedFd>>> = Arc::new(Mutex::new(None));
        let stopped = Arc::new(AtomicBool::new(false));

        if system_bus_likely_present() {
            let logind_notify = logind_efd.clone();
            let logind_lock = inhibitor.clone();
            let logind_stopped = stopped.clone();
            let _ = std::thread::Builder::new()
                .name("session-logind".to_owned())
                .spawn(move || {
                    let _ = watch_logind_shutdown(logind_notify, logind_lock, logind_stopped);
                });
        }

        let worker_stop = stop_efd.clone();
        let worker_inhibitor = inhibitor.clone();
        let worker_stopped = stopped;
        let thread = std::thread::Builder::new()
            .name("session-end".to_owned())
            .spawn(move || {
                let mut ran_ending = false;
                loop {
                    let mut fds = [
                        libc::pollfd {
                            fd: sig_fd,
                            events: libc::POLLIN,
                            revents: 0,
                        },
                        libc::pollfd {
                            fd: logind_efd.as_raw_fd(),
                            events: libc::POLLIN,
                            revents: 0,
                        },
                        libc::pollfd {
                            fd: worker_stop.as_raw_fd(),
                            events: libc::POLLIN,
                            revents: 0,
                        },
                    ];
                    // SAFETY: `fds` holds three open descriptors for the call.
                    let ready =
                        unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
                    if ready < 0 {
                        if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                            continue;
                        }
                        break;
                    }
                    if fds[2].revents & libc::POLLIN != 0 {
                        worker_stopped.store(true, Ordering::SeqCst);
                        break;
                    }
                    if fds[1].revents & libc::POLLIN != 0 {
                        drain_eventfd(logind_efd.as_raw_fd());
                        if !ran_ending {
                            ran_ending = true;
                            ending();
                        }
                        // Closing the delay inhibitor FD tells `logind` that
                        // recovery data is on disk and shutdown may proceed.
                        if let Ok(mut guard) = worker_inhibitor.lock() {
                            let _ = guard.take();
                        }
                    }
                    if fds[0].revents & libc::POLLIN != 0 {
                        drain_eventfd(sig_fd);
                        let sig = LAST_SIGNAL.swap(0, Ordering::SeqCst);
                        if !ran_ending {
                            ending();
                        }
                        if let Ok(mut guard) = worker_inhibitor.lock() {
                            let _ = guard.take();
                        }
                        if sig != 0 {
                            reraise_default(sig);
                        }
                        break;
                    }
                }
            })
            .map_err(|_| {
                ACTIVE_LISTENERS.fetch_sub(1, Ordering::SeqCst);
                "cannot spawn session-end thread"
            })?;

        Ok(Self {
            stop_efd,
            inhibitor,
            thread: Some(thread),
        })
    }
}

impl Drop for SessionEnd {
    fn drop(&mut self) {
        ACTIVE_LISTENERS.fetch_sub(1, Ordering::SeqCst);
        if let Ok(mut guard) = self.inhibitor.lock() {
            let _ = guard.take();
        }
        signal_eventfd(self.stop_efd.as_raw_fd());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn new_eventfd() -> Result<OwnedFd, &'static str> {
    // SAFETY: `EFD_CLOEXEC | EFD_NONBLOCK` creates a fresh kernel eventfd.
    let raw = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
    if raw < 0 {
        return Err("cannot create eventfd");
    }
    // SAFETY: `raw >= 0` is a fresh file descriptor.
    Ok(unsafe { OwnedFd::from_raw_fd(raw) })
}

fn signal_eventfd(fd: i32) {
    let one = 1u64.to_ne_bytes();
    // SAFETY: `fd` is an open `eventfd`.
    let _ = unsafe { libc::write(fd, one.as_ptr().cast(), one.len()) };
}

fn drain_eventfd(fd: i32) {
    let mut buf = [0u8; 8];
    // SAFETY: `fd` is an open non-blocking `eventfd`.
    unsafe { while libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) > 0 {} }
}

fn reraise_default(signal: c_int) {
    // SAFETY: restores `SIG_DFL`, unblocks `signal` on this thread, and raises
    // it so the kernel terminates the process with the original signal status.
    unsafe {
        let mut dflt: libc::sigaction = std::mem::zeroed();
        dflt.sa_sigaction = libc::SIG_DFL;
        libc::sigemptyset(&mut dflt.sa_mask);
        libc::sigaction(signal, &dflt, std::ptr::null_mut());

        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, signal);
        libc::pthread_sigmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
        libc::raise(signal);
    }
}

fn system_bus_likely_present() -> bool {
    std::env::var_os("DBUS_SYSTEM_BUS_ADDRESS").is_some()
        || Path::new("/run/dbus/system_bus_socket").exists()
}

fn watch_logind_shutdown(
    notify_efd: Arc<OwnedFd>,
    inhibitor_slot: Arc<Mutex<Option<zbus::zvariant::OwnedFd>>>,
    stopped: Arc<AtomicBool>,
) -> Result<(), zbus::Error> {
    zbus::block_on(async {
        let conn = zbus::Connection::system().await?;
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.login1")?
            .path("/org/freedesktop/login1")?
            .interface("org.freedesktop.login1.Manager")?
            .member("PrepareForShutdown")?
            .build();
        let mut stream = zbus::MessageStream::for_match_rule(rule, &conn, Some(8)).await?;

        let reply = conn
            .call_method(
                Some("org.freedesktop.login1"),
                "/org/freedesktop/login1",
                Some("org.freedesktop.login1.Manager"),
                "Inhibit",
                &(
                    "shutdown",
                    "Ugurugu 3",
                    "Saving unsaved work for recovery",
                    "delay",
                ),
            )
            .await?;
        let lock_fd: zbus::zvariant::OwnedFd = reply.body().deserialize()?;
        if stopped.load(Ordering::SeqCst) {
            return Ok(());
        }
        if let Ok(mut guard) = inhibitor_slot.lock() {
            *guard = Some(lock_fd);
        }

        while let Some(msg) = stream.next().await {
            if stopped.load(Ordering::SeqCst) {
                break;
            }
            let Ok(msg) = msg else {
                continue;
            };
            if let Ok(starting) = msg.body().deserialize::<bool>()
                && starting
            {
                signal_eventfd(notify_efd.as_raw_fd());
                break;
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_that_waits_for_sigterm() {
        let Some(path) = std::env::var_os("UGU_SIGTERM_CHILD") else {
            return;
        };
        let target = std::path::PathBuf::from(path);
        // SAFETY: test-only `SessionEnd` installation.
        let _guard = unsafe {
            SessionEnd::install(0, move || {
                let _ = std::fs::write(&target, "saved-before-sigterm");
            })
        }
        .unwrap();

        println!("READY");
        let _ = io::Write::flush(&mut io::stdout());
        std::thread::sleep(std::time::Duration::from_secs(10));
    }

    #[test]
    fn a_sigterm_runs_session_end_and_exits_with_sigterm() {
        use std::io::BufRead;
        use std::os::unix::process::ExitStatusExt;

        let folder =
            std::env::temp_dir().join(format!("ugu-platform-sigterm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let marker = folder.join("recovery-marker.txt");

        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "linux::session::tests::child_that_waits_for_sigterm",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("UGU_SIGTERM_CHILD", &marker)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();

        let stdout = child.stdout.take().unwrap();
        let mut reader = io::BufReader::new(stdout);
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap() > 0 {
            if line.contains("READY") {
                break;
            }
            line.clear();
        }

        // SAFETY: `child.id()` is a live child process spawned above.
        unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
        let status = child.wait().unwrap();
        let saved = std::fs::read_to_string(&marker);
        let _ = std::fs::remove_dir_all(&folder);

        assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}");
        assert_eq!(saved.unwrap(), "saved-before-sigterm");
    }
}
