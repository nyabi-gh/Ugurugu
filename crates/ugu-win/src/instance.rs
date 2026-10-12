// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Hands a document named on the command line to a window already open, so
//! that opening a file from the shell adds a tab there instead of a window.
//!
//! The document goes with `WM_COPYDATA` to the topmost window of the class,
//! which is the one used last. Starts that carry a document are serialized
//! by a named mutex held until the window can receive, so files opened
//! together from the shell all reach the first window instead of each
//! making their own.

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, HWND, LPARAM, LRESULT, WAIT_ABANDONED, WAIT_OBJECT_0, WPARAM,
};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, FindWindowExW, GetWindowThreadProcessId, IsIconic, SMTO_ABORTIFHUNG,
    SMTO_BLOCK, SW_RESTORE, SendMessageTimeoutW, SetForegroundWindow, ShowWindow, WM_COPYDATA,
    WM_NCDESTROY,
};

const SUBCLASS_ID: usize = 0x5547_4F50;
/// Marks a `WM_COPYDATA` as a document for this app.
const DOCUMENT: usize = 0x5547_4F50;
const ACCEPTED: LRESULT = 1;
const SEND_TIMEOUT_MS: u32 = 5_000;
/// A start that holds the lock longer than this is taken as stuck.
const START_TIMEOUT_MS: u32 = 15_000;
/// Longer messages are not documents of this app.
const LONGEST_MESSAGE: usize = 1 << 16;

/// Held from a start with a document until its window receives documents.
pub struct StartLock(HANDLE);

impl StartLock {
    /// Waits for other starts with a document. `None` when the lock cannot
    /// be made or is held too long, and the start goes on without it.
    pub fn take(name: &str) -> Option<Self> {
        let name = wide(name.as_ref());
        // SAFETY: `name` is NUL-terminated and outlives the call.
        let mutex = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if mutex.is_null() {
            return None;
        }
        // SAFETY: `mutex` is a handle this function owns.
        match unsafe { WaitForSingleObject(mutex, START_TIMEOUT_MS) } {
            // An abandoned lock belonged to a start that crashed.
            WAIT_OBJECT_0 | WAIT_ABANDONED => Some(Self(mutex)),
            _ => {
                // SAFETY: as above, and not used again.
                unsafe { CloseHandle(mutex) };
                None
            }
        }
    }
}

impl Drop for StartLock {
    fn drop(&mut self) {
        // SAFETY: the mutex is owned by this thread, which took it.
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}

/// Gives `document` to the topmost window of `class` that takes documents
/// for `key`, which names the installation and data folders so that test
/// runs and other installations keep to their own windows. Lets that window
/// come to the front. `false` when no window took it.
pub fn hand_over(class: &str, key: &str, document: &Path) -> bool {
    let class = wide(class.as_ref());
    let mut message: Vec<u16> = key.encode_utf16().collect();
    message.push(0);
    message.extend(document.as_os_str().encode_wide());
    let data = COPYDATASTRUCT {
        dwData: DOCUMENT,
        cbData: (message.len() * 2) as u32,
        lpData: message.as_ptr() as *mut _,
    };
    let mut after: HWND = std::ptr::null_mut();
    loop {
        // SAFETY: `class` is NUL-terminated; a null parent searches the
        // top-level windows in z-order, after `after`.
        let window = unsafe {
            FindWindowExW(
                std::ptr::null_mut(),
                after,
                class.as_ptr(),
                std::ptr::null(),
            )
        };
        if window.is_null() {
            return false;
        }
        after = window;
        let mut process = 0;
        // SAFETY: `window` came from `FindWindowExW`; a closed one fails.
        unsafe { GetWindowThreadProcessId(window, &mut process) };
        // Only the process the shell started may give the foreground away.
        // SAFETY: plain call without pointers.
        unsafe { AllowSetForegroundWindow(process) };
        let mut result = 0;
        // SAFETY: `data` and `message` outlive the call, which copies them
        // into the receiving process.
        let sent = unsafe {
            SendMessageTimeoutW(
                window,
                WM_COPYDATA,
                0,
                &data as *const COPYDATASTRUCT as LPARAM,
                SMTO_ABORTIFHUNG | SMTO_BLOCK,
                SEND_TIMEOUT_MS,
                &mut result,
            )
        };
        if sent != 0 && result as LRESULT == ACCEPTED {
            return true;
        }
    }
}

/// Restores `hwnd` if minimized and brings it to the front, which Windows
/// allows after [`hand_over`] let it.
///
/// # Safety
/// `hwnd` must be a window owned by the calling thread.
pub unsafe fn bring_to_front(hwnd: isize) {
    let hwnd = hwnd as HWND;
    // SAFETY: the caller owns the window.
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd);
    }
}

type Receive = dyn FnMut(PathBuf) -> bool;

/// Receives documents handed over to one window until dropped.
pub struct DocumentReceiver {
    hwnd: HWND,
    /// Read by the subclass until `Drop` removes it.
    _shared: Box<Shared>,
}

struct Shared {
    key: Vec<u16>,
    receive: std::cell::RefCell<Box<Receive>>,
}

impl DocumentReceiver {
    /// `receive` is called on the window's thread with each document for
    /// `key` and says whether it took it.
    ///
    /// # Safety
    /// `hwnd` must be a live window owned by the calling thread, which must
    /// also be the thread that drops the returned value.
    pub unsafe fn install(
        hwnd: isize,
        key: &str,
        receive: impl FnMut(PathBuf) -> bool + 'static,
    ) -> Result<Self, &'static str> {
        let hwnd = hwnd as HWND;
        let shared = Box::new(Shared {
            key: key.encode_utf16().collect(),
            receive: std::cell::RefCell::new(Box::new(receive)),
        });
        let data = &*shared as *const Shared as usize;
        // SAFETY: `shared` is boxed and outlives the subclass, which `Drop` removes.
        if unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, data) } == 0 {
            return Err("SetWindowSubclass failed");
        }
        Ok(Self {
            hwnd,
            _shared: shared,
        })
    }
}

impl Drop for DocumentReceiver {
    fn drop(&mut self) {
        // SAFETY: removing a subclass from a destroyed window fails harmlessly.
        unsafe { RemoveWindowSubclass(self.hwnd, Some(subclass_proc), SUBCLASS_ID) };
    }
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    match message {
        WM_COPYDATA => {
            // SAFETY: `data` is the `Shared` installed with this subclass,
            // and `lparam` of `WM_COPYDATA` points at a `COPYDATASTRUCT`.
            let (shared, copied) = unsafe {
                (
                    &*(data as *const Shared),
                    &*(lparam as *const COPYDATASTRUCT),
                )
            };
            if copied.dwData == DOCUMENT {
                let length = copied.cbData as usize;
                let message: Vec<u16> = if length > LONGEST_MESSAGE * 2 || copied.lpData.is_null() {
                    Vec::new()
                } else {
                    // SAFETY: Windows copied `cbData` bytes to `lpData`,
                    // with no promise of alignment.
                    unsafe { std::slice::from_raw_parts(copied.lpData as *const u8, length) }
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|&pair| u16::from_le_bytes(pair))
                        .collect()
                };
                let taken = document_for(&message, &shared.key).is_some_and(|document| {
                    shared
                        .receive
                        .try_borrow_mut()
                        .is_ok_and(|mut receive| receive(document))
                });
                return if taken { ACCEPTED } else { 0 };
            }
        }
        WM_NCDESTROY => {
            // SAFETY: removal during WM_NCDESTROY is the documented clean-up.
            unsafe { RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID) };
        }
        _ => {}
    }
    // SAFETY: forwards the original arguments down the subclass chain.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

/// The document in a message, if the message is for `key`.
fn document_for(message: &[u16], key: &[u16]) -> Option<PathBuf> {
    let (sent_key, document) = message.split_at(message.iter().position(|&unit| unit == 0)?);
    let document = &document[1..];
    (sent_key == key && !document.is_empty()).then(|| PathBuf::from(OsString::from_wide(document)))
}

fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, WNDCLASSW,
    };

    /// A hidden top-level window of a class only this test uses.
    fn hidden_window(class: &str) -> HWND {
        let class = wide(class.as_ref());
        // SAFETY: the class name outlives both calls; the window stays
        // hidden and is destroyed by the test.
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let description = WNDCLASSW {
                lpfnWndProc: Some(DefWindowProcW),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassW(&description);
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                0,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            )
        }
    }

    #[test]
    fn a_document_reaches_the_window_with_the_same_key_only() {
        let class = format!("UguruguInstanceTest{}", std::process::id());
        let window = hidden_window(&class);
        assert!(!window.is_null());
        let received = Rc::new(RefCell::new(Vec::new()));
        let sink = received.clone();
        // SAFETY: the window belongs to this thread, which drops the receiver.
        let receiver = unsafe {
            DocumentReceiver::install(window as isize, "data", move |path| {
                sink.borrow_mut().push(path);
                true
            })
        }
        .unwrap();

        let document = Path::new(r"C:\그림\고양이 1.ugurugu");
        assert!(hand_over(&class, "data", document));
        assert!(!hand_over(&class, "other data", document));
        assert_eq!(*received.borrow(), [document.to_path_buf()]);

        drop(receiver);
        assert!(!hand_over(&class, "data", document));
        // SAFETY: made above on this thread.
        unsafe { DestroyWindow(window) };
        assert!(!hand_over(&class, "data", document));
    }

    #[test]
    fn a_message_without_a_document_is_refused() {
        let key: Vec<u16> = "k".encode_utf16().collect();
        assert_eq!(document_for(&[], &key), None);
        assert_eq!(document_for(&[107], &key), None);
        assert_eq!(document_for(&[107, 0], &key), None);
        assert_eq!(document_for(&[107, 0, 97], &key), Some(PathBuf::from("a")));
    }

    #[test]
    fn starts_with_a_document_take_turns() {
        let name = format!("Local\\UguruguInstanceTest{}", std::process::id());
        let first = StartLock::take(&name).unwrap();
        let waited = std::thread::spawn({
            let name = name.clone();
            move || {
                let started = std::time::Instant::now();
                drop(StartLock::take(&name).unwrap());
                started.elapsed()
            }
        });
        std::thread::sleep(std::time::Duration::from_millis(200));
        drop(first);
        assert!(waited.join().unwrap() >= std::time::Duration::from_millis(150));
    }
}
