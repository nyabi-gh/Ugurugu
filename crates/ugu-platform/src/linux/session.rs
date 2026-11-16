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
    // SAFETY: `__errno_location` is async-signal-safe and gives the `errno`
    // of this thread, which the code this signal interrupted may be about
    // to read.
    let errno = unsafe { *libc::__errno_location() };
    handle_term(signal);
    // SAFETY: as above.
    unsafe { *libc::__errno_location() = errno };
}

fn handle_term(signal: c_int) {
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
            for signal in [libc::SIGTERM, libc::SIGHUP] {
                // One the process was started to ignore (`nohup`) stays
                // ignored.
                let mut before: libc::sigaction = std::mem::zeroed();
                libc::sigaction(signal, std::ptr::null(), &mut before);
                if before.sa_sigaction != libc::SIG_IGN {
                    libc::sigaction(signal, &action, std::ptr::null_mut());
                }
            }
        }
    }
    Ok(SIGNAL_PIPE.get().map_or(fd, |p| p.efd.as_raw_fd()))
}

/// Counted in `ACTIVE_LISTENERS` for as long as the session-end thread runs,
/// however it ends: a count left behind would have the handler swallow every
/// later signal.
struct Listening;

impl Listening {
    fn start() -> Self {
        ACTIVE_LISTENERS.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        ACTIVE_LISTENERS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// What the logind watcher and the session-end thread share.
struct Logind {
    /// Signalled when `shutting_down` changes.
    changed: OwnedFd,
    /// The last `PrepareForShutdown`: a shutdown is under way, or it was
    /// called off.
    shutting_down: AtomicBool,
    inhibitor: Mutex<Option<zbus::zvariant::OwnedFd>>,
    stopped: AtomicBool,
}

impl Logind {
    fn release(&self) {
        if let Ok(mut guard) = self.inhibitor.lock() {
            let _ = guard.take();
        }
    }
}

/// Calls back as the Linux session ends, until dropped.
pub struct SessionEnd {
    stop_efd: Arc<OwnedFd>,
    logind: Arc<Logind>,
    thread: Option<JoinHandle<()>>,
}

impl SessionEnd {
    /// Installs `SIGTERM`/`SIGHUP` watchers and a best-effort `systemd-logind`
    /// `"shutdown"` delay inhibitor lock. `ending` runs before the inhibitor
    /// lock is released or the termination signal is re-raised, and again
    /// for a later end when a shutdown was called off.
    ///
    /// # Safety
    /// None on Linux; `unsafe` to match the cross-OS signature.
    pub unsafe fn install(
        _hwnd: isize,
        ending: impl FnMut() + Send + 'static,
    ) -> Result<Self, &'static str> {
        Self::install_with(system_bus_likely_present(), ending)
    }

    fn install_with(
        watch_logind: bool,
        ending: impl FnMut() + Send + 'static,
    ) -> Result<Self, &'static str> {
        let sig_fd = ensure_signal_pipe()?;
        // Drain any stale count left before this `SessionEnd` was installed.
        drain_eventfd(sig_fd);
        LAST_SIGNAL.store(0, Ordering::SeqCst);

        let stop_efd = Arc::new(new_eventfd()?);
        let logind = Arc::new(Logind {
            changed: new_eventfd()?,
            shutting_down: AtomicBool::new(false),
            inhibitor: Mutex::new(None),
            stopped: AtomicBool::new(false),
        });

        if watch_logind {
            let logind = logind.clone();
            let _ = std::thread::Builder::new()
                .name("session-logind".to_owned())
                .spawn(move || {
                    let _ = watch_logind_shutdown(&logind);
                });
        }

        let worker_stop = stop_efd.clone();
        let worker_logind = logind.clone();
        // Dropped with the closure when the thread cannot start.
        let listening = Listening::start();
        let thread = std::thread::Builder::new()
            .name("session-end".to_owned())
            .spawn(move || {
                let _listening = listening;
                wait_for_the_end(sig_fd, &worker_stop, &worker_logind, ending);
            })
            .map_err(|_| "cannot spawn session-end thread")?;

        Ok(Self {
            stop_efd,
            logind,
            thread: Some(thread),
        })
    }
}

fn wait_for_the_end(sig_fd: i32, stop: &OwnedFd, logind: &Logind, mut ending: impl FnMut()) {
    // Whether `ending` has run for the shutdown now under way.
    let mut ran_ending = false;
    loop {
        let mut fds = [
            libc::pollfd {
                fd: sig_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: logind.changed.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: stop.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: `fds` holds three open descriptors for the call.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
        if ready < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if fds[2].revents & libc::POLLIN != 0 {
            logind.stopped.store(true, Ordering::SeqCst);
            break;
        }
        if fds[1].revents & libc::POLLIN != 0 {
            drain_eventfd(logind.changed.as_raw_fd());
            if logind.shutting_down.load(Ordering::SeqCst) {
                if !ran_ending {
                    ran_ending = true;
                    ending();
                }
                // Closing the delay inhibitor FD tells `logind` that
                // recovery data is on disk and shutdown may proceed.
                logind.release();
            } else {
                // Called off: the work done from here on is not in what
                // `ending` wrote.
                ran_ending = false;
            }
        }
        if fds[0].revents & libc::POLLIN != 0 {
            drain_eventfd(sig_fd);
            let sig = LAST_SIGNAL.swap(0, Ordering::SeqCst);
            if !ran_ending {
                ending();
            }
            logind.release();
            if sig != 0 {
                reraise_default(sig);
            }
            break;
        }
    }
}

impl Drop for SessionEnd {
    fn drop(&mut self) {
        self.logind.release();
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

async fn inhibit_shutdown(conn: &zbus::Connection) -> zbus::Result<zbus::zvariant::OwnedFd> {
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
    reply.body().deserialize()
}

fn watch_logind_shutdown(logind: &Logind) -> Result<(), zbus::Error> {
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

        let mut lock_fd = Some(inhibit_shutdown(&conn).await?);
        loop {
            if logind.stopped.load(Ordering::SeqCst) {
                break;
            }
            if let Some(lock_fd) = lock_fd.take()
                && let Ok(mut guard) = logind.inhibitor.lock()
            {
                *guard = Some(lock_fd);
            }
            let Some(msg) = stream.next().await else {
                break;
            };
            let Ok(starting) = msg.and_then(|msg| msg.body().deserialize::<bool>()) else {
                continue;
            };
            if !starting {
                // The lock was given up when the shutdown began; without a
                // new one the next shutdown would not wait for the recovery.
                lock_fd = inhibit_shutdown(&conn).await.ok();
            }
            logind.shutting_down.store(starting, Ordering::SeqCst);
            signal_eventfd(logind.changed.as_raw_fd());
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
        let _guard = SessionEnd::install_with(false, move || {
            let _ = std::fs::write(&target, "saved-before-sigterm");
        })
        .unwrap();

        println!("READY");
        let _ = io::Write::flush(&mut io::stdout());
        std::thread::sleep(std::time::Duration::from_secs(10));
    }

    /// This test binary running only `test`, with `variable` set to `value`,
    /// once it has printed `READY`.
    fn ready_child(test: &str, variable: &str, value: &Path) -> std::process::Child {
        use std::io::BufRead;

        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env(variable, value)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let mut reader = io::BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap() > 0 {
            if line.contains("READY") {
                break;
            }
            line.clear();
        }
        child
    }

    #[test]
    fn child_that_ignores_sighup() {
        let Some(path) = std::env::var_os("UGU_SIGHUP_CHILD") else {
            return;
        };
        let target = std::path::PathBuf::from(path);
        // SAFETY: test-only; ignoring a signal has no preconditions.
        unsafe { libc::signal(libc::SIGHUP, libc::SIG_IGN) };
        let _guard = SessionEnd::install_with(false, move || {
            let _ = std::fs::write(&target, "saved");
        })
        .unwrap();

        println!("READY");
        let _ = io::Write::flush(&mut io::stdout());
        std::thread::sleep(std::time::Duration::from_secs(10));
    }

    #[test]
    fn a_sighup_the_process_was_started_to_ignore_stays_ignored() {
        use std::os::unix::process::ExitStatusExt;

        let folder =
            std::env::temp_dir().join(format!("ugu-platform-sighup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let marker = folder.join("recovery-marker.txt");
        let mut child = ready_child(
            "linux::session::tests::child_that_ignores_sighup",
            "UGU_SIGHUP_CHILD",
            &marker,
        );

        // SAFETY: `child.id()` is a live child process spawned above.
        unsafe { libc::kill(child.id() as i32, libc::SIGHUP) };
        std::thread::sleep(std::time::Duration::from_millis(300));
        let after_hangup = (child.try_wait().unwrap(), marker.exists());
        // SAFETY: as above.
        unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
        let status = child.wait().unwrap();
        let _ = std::fs::remove_dir_all(&folder);

        assert_eq!(after_hangup, (None, false));
        assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}");
    }

    #[test]
    fn child_whose_session_end_cannot_start() {
        if std::env::var_os("UGU_NO_FD_CHILD").is_none() {
            return;
        }
        ensure_signal_pipe().unwrap();
        // SAFETY: test-only; a limit of no descriptors makes the next
        // `eventfd` fail, and the old limit is put back at once.
        let installed = unsafe {
            let mut before: libc::rlimit = std::mem::zeroed();
            libc::getrlimit(libc::RLIMIT_NOFILE, &mut before);
            let none = libc::rlimit {
                rlim_cur: 0,
                rlim_max: before.rlim_max,
            };
            libc::setrlimit(libc::RLIMIT_NOFILE, &none);
            let installed = SessionEnd::install_with(false, || {});
            libc::setrlimit(libc::RLIMIT_NOFILE, &before);
            installed
        };
        assert!(installed.is_err());

        println!("READY");
        let _ = io::Write::flush(&mut io::stdout());
        std::thread::sleep(std::time::Duration::from_secs(10));
    }

    #[test]
    fn a_session_end_that_could_not_start_leaves_sigterm_ending_the_process() {
        use std::os::unix::process::ExitStatusExt;

        let mut child = ready_child(
            "linux::session::tests::child_whose_session_end_cannot_start",
            "UGU_NO_FD_CHILD",
            Path::new("1"),
        );
        // SAFETY: `child.id()` is a live child process spawned above.
        unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
        let status = child.wait().unwrap();
        assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}");
    }

    fn logind() -> Logind {
        Logind {
            changed: new_eventfd().unwrap(),
            shutting_down: AtomicBool::new(false),
            inhibitor: Mutex::new(None),
            stopped: AtomicBool::new(false),
        }
    }

    #[test]
    fn a_shutdown_called_off_has_the_next_one_write_the_recovery_again() {
        let logind = logind();
        let stop = new_eventfd().unwrap();
        let signals = new_eventfd().unwrap();
        let (told, written) = std::sync::mpsc::channel();
        let wait = std::time::Duration::from_secs(5);
        let quiet = std::time::Duration::from_millis(200);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                wait_for_the_end(signals.as_raw_fd(), &stop, &logind, || {
                    let _ = told.send(());
                });
            });
            let prepare = |starting: bool| {
                logind.shutting_down.store(starting, Ordering::SeqCst);
                signal_eventfd(logind.changed.as_raw_fd());
            };
            prepare(true);
            assert!(written.recv_timeout(wait).is_ok());
            prepare(true);
            assert!(written.recv_timeout(quiet).is_err(), "once for a shutdown");
            prepare(false);
            assert!(written.recv_timeout(quiet).is_err(), "nothing ends");
            prepare(true);
            assert!(written.recv_timeout(wait).is_ok(), "the next shutdown");
            signal_eventfd(stop.as_raw_fd());
        });
        assert!(logind.stopped.load(Ordering::SeqCst));
    }

    #[test]
    fn the_signal_handler_counts_a_listener_only_while_its_thread_runs() {
        let before = ACTIVE_LISTENERS.load(Ordering::SeqCst);
        let listening = Listening::start();
        assert_eq!(ACTIVE_LISTENERS.load(Ordering::SeqCst), before + 1);
        // As when the thread ends by itself or was never started.
        drop(listening);
        assert_eq!(ACTIVE_LISTENERS.load(Ordering::SeqCst), before);
    }

    #[test]
    fn a_sigterm_runs_session_end_and_exits_with_sigterm() {
        use std::os::unix::process::ExitStatusExt;

        let folder =
            std::env::temp_dir().join(format!("ugu-platform-sigterm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let marker = folder.join("recovery-marker.txt");

        let mut child = ready_child(
            "linux::session::tests::child_that_waits_for_sigterm",
            "UGU_SIGTERM_CHILD",
            &marker,
        );

        // SAFETY: `child.id()` is a live child process spawned above.
        unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
        let status = child.wait().unwrap();
        let saved = std::fs::read_to_string(&marker);
        let _ = std::fs::remove_dir_all(&folder);

        assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}");
        assert_eq!(saved.unwrap(), "saved-before-sigterm");
    }
}
