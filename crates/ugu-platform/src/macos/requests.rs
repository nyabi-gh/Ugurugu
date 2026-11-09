// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! What macOS asks of the app as a whole: to open documents, from Finder and
//! the Dock, and to quit, from the Dock or as the user logs out. AppKit asks
//! the application delegate. That one is winit's, which panics if it is
//! replaced and passes neither request on, so `listen` gives its class the
//! two methods. Everything here runs on the main thread.

use std::cell::RefCell;
use std::ffi::CStr;
use std::path::PathBuf;

use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{MainThreadMarker, ffi, msg_send, sel};
use objc2_app_kit::{NSApplication, NSApplicationTerminateReply};
use objc2_foundation::{NSAppleEventManager, NSArray, NSUInteger, NSURL};

type Receive = Box<dyn FnMut(Option<PathBuf>) -> bool>;

#[derive(Default)]
struct Requests {
    /// Asked for before there was a window to take them: AppKit delivers the
    /// documents a launch was for before winit reports the launch.
    documents: Vec<PathBuf>,
    receive: Option<Receive>,
    quit: Option<Box<dyn FnMut()>>,
    ending: Option<Box<dyn FnMut() + Send>>,
}

thread_local! {
    static REQUESTS: RefCell<Requests> = RefCell::default();
}

/// Starts hearing the requests. Call on the main thread once winit's event
/// loop is built, which sets the delegate, and before it runs, which is when
/// the documents a launch was for arrive.
pub fn listen() {
    let mtm = MainThreadMarker::new().expect("the event loop is built on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    let delegate = app
        .delegate()
        .expect("winit sets the delegate when its event loop is built");
    // SAFETY: an object's class is registered, and winit 0.30.13's answers
    // neither selector.
    unsafe { add_methods(AnyObject::class(delegate.as_ref())) };
    // AppKit may note which methods a delegate answers when it is set.
    app.setDelegate(None);
    app.setDelegate(Some(&delegate));
}

/// # Safety
/// `class` is registered.
unsafe fn add_methods(class: &AnyClass) {
    let open: unsafe extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, *const NSArray<NSURL>) =
        open_urls;
    let terminate: extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) -> NSUInteger =
        should_terminate;
    // SAFETY: the signatures are AppKit's for these selectors and the type
    // strings say so. `extern "C"` aborts on a panic instead of unwinding
    // into AppKit.
    unsafe {
        add_method(
            class,
            sel!(application:openURLs:),
            std::mem::transmute::<
                unsafe extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, *const NSArray<NSURL>),
                Imp,
            >(open),
            c"v@:@@",
        );
        add_method(
            class,
            sel!(applicationShouldTerminate:),
            std::mem::transmute::<
                extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) -> NSUInteger,
                Imp,
            >(terminate),
            c"Q@:@",
        );
    }
}

/// # Safety
/// `imp` has the signature `types` describes, and `class` is registered.
unsafe fn add_method(class: &AnyClass, name: Sel, imp: Imp, types: &CStr) {
    let class: *const AnyClass = class;
    // SAFETY: as the caller promises.
    let added = unsafe { ffi::class_addMethod(class.cast_mut(), name, imp, types.as_ptr()) };
    assert!(added.as_bool(), "the delegate already answers {name}");
}

/// # Safety
/// AppKit's call: `urls` is a live array.
unsafe extern "C" fn open_urls(
    _delegate: *mut AnyObject,
    _selector: Sel,
    _app: *mut AnyObject,
    urls: *const NSArray<NSURL>,
) {
    // SAFETY: as the caller promises.
    let urls = unsafe { &*urls };
    open(urls.iter().filter_map(|url| path_of(&url)).collect());
}

/// The file `url` names; `None` when it is not a file.
fn path_of(url: &NSURL) -> Option<PathBuf> {
    if !url.isFileURL() {
        return None;
    }
    url.path().map(|path| PathBuf::from(path.to_string()))
}

fn open(documents: Vec<PathBuf>) {
    let receive = REQUESTS.with_borrow_mut(|requests| requests.receive.take());
    let Some(mut receive) = receive else {
        REQUESTS.with_borrow_mut(|requests| requests.documents.extend(documents));
        return;
    };
    for document in documents {
        receive(Some(document));
    }
    REQUESTS.with_borrow_mut(|requests| {
        requests.receive.get_or_insert(receive);
    });
}

/// Hands the documents waiting and every later one to `receive`.
pub fn receive_documents(receive: Receive) {
    let waiting = REQUESTS.with_borrow_mut(|requests| {
        requests.receive = Some(receive);
        std::mem::take(&mut requests.documents)
    });
    open(waiting);
}

pub fn stop_receiving_documents() {
    REQUESTS.with_borrow_mut(|requests| requests.receive = None);
}

pub fn set_quit(quit: Option<Box<dyn FnMut()>>) {
    REQUESTS.with_borrow_mut(|requests| requests.quit = quit);
}

pub fn set_ending(ending: Option<Box<dyn FnMut() + Send>>) {
    REQUESTS.with_borrow_mut(|requests| requests.ending = ending);
}

extern "C" fn should_terminate(
    _delegate: *mut AnyObject,
    _selector: Sel,
    _app: *mut AnyObject,
) -> NSUInteger {
    answer_to_quit(quit_reason()).0
}

/// Why the Apple event being handled asks the app to quit, as a four-byte
/// code; `None` when no event gives a reason.
fn quit_reason() -> Option<u32> {
    let event = NSAppleEventManager::sharedAppleEventManager().currentAppleEvent()?;
    // SAFETY: AppKit's documented selectors; the keyword is `keyAEQuitReason`.
    unsafe {
        let reason: Option<Retained<AnyObject>> =
            msg_send![&*event, attributeDescriptorForKeyword: code(b"why?")];
        let reason = reason?;
        Some(msg_send![&*reason, typeCodeValue])
    }
}

const fn code(bytes: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*bytes)
}

/// Whether the system is logging out, restarting or shutting down.
fn ends_session(reason: u32) -> bool {
    [b"logo", b"rlgo", b"rrst", b"rsdn", b"rest", b"shut"]
        .iter()
        .any(|&ending| reason == code(ending))
}

/// As the session ends the app keeps its work for recovery without asking,
/// off the main thread, whose window calls the render thread waits on, and
/// then lets the system go on. Asked to quit otherwise it goes the way
/// closing the window does, which may keep it open; the request itself is
/// over either way, so AppKit is told not to end the process.
fn answer_to_quit(reason: Option<u32>) -> NSApplicationTerminateReply {
    if reason.is_some_and(ends_session) {
        let ending = REQUESTS.with_borrow_mut(|requests| requests.ending.take());
        let Some(mut ending) = ending else {
            return NSApplicationTerminateReply::TerminateNow;
        };
        let keeping = std::thread::Builder::new()
            .name("session end".to_owned())
            .spawn(move || {
                ending();
                DispatchQueue::main().exec_async(|| {
                    let mtm =
                        MainThreadMarker::new().expect("the main queue runs on the main thread");
                    NSApplication::sharedApplication(mtm).replyToApplicationShouldTerminate(true);
                });
            });
        return match keeping {
            Ok(_) => NSApplicationTerminateReply::TerminateLater,
            Err(_) => NSApplicationTerminateReply::TerminateNow,
        };
    }
    let quit = REQUESTS.with_borrow_mut(|requests| requests.quit.take());
    let Some(mut quit) = quit else {
        return NSApplicationTerminateReply::TerminateNow;
    };
    quit();
    REQUESTS.with_borrow_mut(|requests| {
        requests.quit.get_or_insert(quit);
    });
    NSApplicationTerminateReply::TerminateCancel
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::mpsc;
    use std::time::Duration;

    use objc2::runtime::{NSObject, NSObjectProtocol};
    use objc2::{AnyThread, define_class};
    use objc2_foundation::NSString;

    use super::super::instance::DocumentReceiver;
    use super::super::session::{QuitRequests, SessionEnd};
    use super::*;

    define_class!(
        /// Answers neither request, as winit's delegate does not.
        #[unsafe(super(NSObject))]
        struct Stand;

        unsafe impl NSObjectProtocol for Stand {}
    );

    fn file_url(path: &str) -> Retained<NSURL> {
        NSURL::fileURLWithPath(&NSString::from_str(path))
    }

    /// Calls `receive` with what it gets collected into the returned list.
    fn collecting() -> (Rc<RefCell<Vec<PathBuf>>>, DocumentReceiver) {
        let opened = Rc::new(RefCell::new(Vec::new()));
        let sink = opened.clone();
        // SAFETY: none needed here; see the function.
        let receiver = unsafe {
            DocumentReceiver::install(0, "", move |path| {
                sink.borrow_mut().extend(path);
                true
            })
        }
        .unwrap();
        (opened, receiver)
    }

    #[test]
    fn a_file_url_becomes_its_path_and_other_urls_nothing() {
        assert_eq!(
            path_of(&file_url("/tmp/図 1.ugurugu")),
            Some(PathBuf::from("/tmp/図 1.ugurugu"))
        );
        let web = NSURL::URLWithString(&NSString::from_str("https://example.com/a")).unwrap();
        assert_eq!(path_of(&web), None);
    }

    #[test]
    fn only_the_system_going_away_ends_the_session() {
        for reason in [b"logo", b"rlgo", b"rrst", b"rsdn", b"rest", b"shut"] {
            assert!(ends_session(code(reason)));
        }
        assert!(!ends_session(0));
        assert!(!ends_session(code(b"quit")));
    }

    #[test]
    fn documents_wait_for_the_window_and_then_go_straight_to_it() {
        open(vec![PathBuf::from("/a"), PathBuf::from("/b")]);
        let (opened, receiver) = collecting();
        assert_eq!(*opened.borrow(), ["/a", "/b"].map(PathBuf::from));
        open(vec![PathBuf::from("/c")]);
        assert_eq!(opened.borrow().len(), 3);
        drop(receiver);
        open(vec![PathBuf::from("/d")]);
        assert_eq!(opened.borrow().len(), 3);
    }

    #[test]
    fn a_quit_goes_to_the_app_and_never_ends_the_process_itself() {
        assert_eq!(
            answer_to_quit(None),
            NSApplicationTerminateReply::TerminateNow
        );
        let asked = Rc::new(Cell::new(0));
        let count = asked.clone();
        let quit = QuitRequests::install(move || count.set(count.get() + 1));
        for reason in [None, Some(0), Some(code(b"quit"))] {
            assert_eq!(
                answer_to_quit(reason),
                NSApplicationTerminateReply::TerminateCancel
            );
        }
        assert_eq!(asked.get(), 3);
        drop(quit);
        assert_eq!(
            answer_to_quit(None),
            NSApplicationTerminateReply::TerminateNow
        );
    }

    #[test]
    fn the_session_ending_keeps_the_work_before_the_process_ends() {
        let asked = Rc::new(Cell::new(false));
        let flag = asked.clone();
        let quit = QuitRequests::install(move || flag.set(true));
        assert_eq!(
            answer_to_quit(Some(code(b"logo"))),
            NSApplicationTerminateReply::TerminateNow
        );
        let (kept, on_disk) = mpsc::channel();
        // SAFETY: none needed here; see the function.
        let ending = unsafe { SessionEnd::install(0, move || kept.send(()).unwrap()) }.unwrap();
        assert_eq!(
            answer_to_quit(Some(code(b"rlgo"))),
            NSApplicationTerminateReply::TerminateLater
        );
        on_disk.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(!asked.get());
        drop((ending, quit));
    }

    #[test]
    fn a_delegate_given_the_methods_answers_appkit() {
        // SAFETY: NSObject's init takes no arguments.
        let delegate: Retained<Stand> = unsafe { msg_send![Stand::alloc(), init] };
        let class = AnyObject::class(&delegate);
        assert!(!class.responds_to(sel!(application:openURLs:)));
        assert!(!class.responds_to(sel!(applicationShouldTerminate:)));
        // SAFETY: an object's class is registered.
        unsafe { add_methods(class) };

        let (opened, _receiver) = collecting();
        let urls = NSArray::from_retained_slice(&[file_url("/tmp/a.ugurugu")]);
        let no_app: *mut AnyObject = std::ptr::null_mut();
        // SAFETY: the selector and argument types are AppKit's; neither
        // method reads the application.
        let reply = unsafe {
            let _: () = msg_send![&*delegate, application: no_app, openURLs: &*urls];
            let reply: NSUInteger = msg_send![&*delegate, applicationShouldTerminate: no_app];
            NSApplicationTerminateReply(reply)
        };
        assert_eq!(*opened.borrow(), [PathBuf::from("/tmp/a.ugurugu")]);
        assert_eq!(reply, NSApplicationTerminateReply::TerminateNow);
    }
}
