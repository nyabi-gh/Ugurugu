// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The macOS open and save panels. AppKit shows them only from the main
//! thread, so a call hands the panel to it and waits for the answer; the
//! main thread is not held while the panel is open.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSModalResponse, NSModalResponseOK, NSOpenPanel, NSPopUpButton, NSSavePanel, NSView,
};
use objc2_foundation::{NSArray, NSObject, NSPoint, NSRect, NSSize, NSString, NSURL};
use objc2_uniform_type_identifiers::UTType;
use ugu_platform_types::dialog::{Dialog, FileType};

/// Shows the dialog as a sheet on the window of `owner` (an `NSView`), or on
/// its own without one, and returns the chosen path, or `None` when
/// cancelled. A save panel adds the chosen type's extension itself and asks
/// about replacing the file it returns.
pub fn pick(owner: isize, dialog: &Dialog) -> Result<Option<PathBuf>, String> {
    let (send, receive) = mpsc::channel();
    let dialog = dialog.clone();
    DispatchQueue::main().exec_async(move || {
        let mtm = MainThreadMarker::new().expect("the main queue runs on the main thread");
        // SAFETY: `owner` is the app window's view, which lives as long as
        // the window the dialog belongs to.
        unsafe { begin(mtm, owner, &dialog, send) };
    });
    receive
        .recv()
        .map_err(|_| "the panel closed without an answer".to_owned())
}

/// # Safety
/// `owner` is zero or a live `NSView`.
unsafe fn begin(
    mtm: MainThreadMarker,
    owner: isize,
    dialog: &Dialog,
    send: mpsc::Sender<Option<PathBuf>>,
) {
    let mut chooser = None;
    let panel: Retained<NSSavePanel> = match dialog {
        Dialog::Open(file_type) => {
            let panel = NSOpenPanel::openPanel(mtm);
            panel.setCanChooseFiles(true);
            panel.setCanChooseDirectories(false);
            panel.setAllowsMultipleSelection(false);
            panel.setAllowedContentTypes(&content_types(file_type));
            Retained::into_super(panel)
        }
        Dialog::Save {
            file_types,
            name,
            folder,
        } => {
            let panel = NSSavePanel::savePanel(mtm);
            panel.setCanCreateDirectories(true);
            panel.setAllowedContentTypes(&content_types(&file_types[0]));
            panel.setNameFieldStringValue(&NSString::from_str(name));
            if let Some(folder) = folder {
                start_in(&panel, folder);
            }
            if file_types.len() > 1 {
                chooser = Some(TypeChooser::attach(mtm, &panel, file_types));
            }
            panel
        }
        Dialog::Folder { title, start } => {
            let panel = NSOpenPanel::openPanel(mtm);
            panel.setCanChooseFiles(false);
            panel.setCanChooseDirectories(true);
            panel.setCanCreateDirectories(true);
            panel.setAllowsMultipleSelection(false);
            panel.setMessage(Some(&NSString::from_str(title)));
            if let Some(start) = start {
                start_in(&panel, start);
            }
            Retained::into_super(panel)
        }
    };
    let answered = panel.clone();
    let handler = RcBlock::new(move |response: NSModalResponse| {
        let path = if response == NSModalResponseOK {
            answered.URL().and_then(|url| url.to_file_path())
        } else {
            None
        };
        // The pop-up's target is not retained by the pop-up.
        let _ = &chooser;
        // The receiver waits until this answer comes.
        let _ = send.send(path);
    });
    // SAFETY: the caller passes zero or a live view.
    let window = (owner != 0)
        .then(|| unsafe { &*(owner as *const NSView) })
        .and_then(NSView::window);
    match window {
        Some(window) => panel.beginSheetModalForWindow_completionHandler(&window, &handler),
        None => panel.beginWithCompletionHandler(&handler),
    }
}

/// Opens the panel in `folder`; one that cannot be found is left to macOS,
/// which then starts where it last did.
fn start_in(panel: &NSSavePanel, folder: &Path) {
    if let Some(url) = NSURL::from_directory_path(folder) {
        panel.setDirectoryURL(Some(&url));
    }
}

/// An extension macOS does not know gets a type made from it, which
/// matches files by that extension alone.
fn content_types(file_type: &FileType) -> Retained<NSArray<UTType>> {
    let types: Vec<Retained<UTType>> = file_type
        .extensions
        .iter()
        .filter_map(|extension| UTType::typeWithFilenameExtension(&NSString::from_str(extension)))
        .collect();
    NSArray::from_retained_slice(&types)
}

struct Choices {
    panel: Retained<NSSavePanel>,
    types: Vec<Retained<NSArray<UTType>>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and this class
    // does not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "UguruguFileTypeChooser"]
    #[ivars = Choices]
    /// The save panel's pop-up of file types, which the panel lacks:
    /// choosing one makes the panel save as that type.
    struct TypeChooser;

    impl TypeChooser {
        #[unsafe(method(chooseType:))]
        fn choose_type(&self, sender: &NSPopUpButton) {
            let choices = self.ivars();
            if let Some(types) = usize::try_from(sender.indexOfSelectedItem())
                .ok()
                .and_then(|index| choices.types.get(index))
            {
                choices.panel.setAllowedContentTypes(types);
            }
        }
    }
);

impl TypeChooser {
    fn attach(
        mtm: MainThreadMarker,
        panel: &Retained<NSSavePanel>,
        file_types: &[FileType],
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Choices {
            panel: panel.clone(),
            types: file_types.iter().map(content_types).collect(),
        });
        // SAFETY: NSObject's init takes no arguments.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
        let pop_up =
            NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), frame, false);
        for file_type in file_types {
            pop_up.addItemWithTitle(&NSString::from_str(file_type.name));
        }
        // SAFETY: the target answers the action's selector, and outlives the
        // panel's answer, which keeps it.
        unsafe {
            pop_up.setTarget(Some(&this));
            pop_up.setAction(Some(sel!(chooseType:)));
        }
        pop_up.sizeToFit();
        panel.setAccessoryView(Some(&pop_up));
        this
    }
}
