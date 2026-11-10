// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The macOS menu bar. It shows the app's actions and their keys, but the
//! keys stay the app's: they reach its shortcuts as they do without a menu,
//! so that a text field being typed in and a key the user changed behave as
//! on every OS. Only the system's own items and, while a native panel has
//! the keyboard, Edit's commands answer keys here.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSControlStateValueOff, NSControlStateValueOn, NSEvent, NSEventModifierFlags,
    NSMenu, NSMenuDelegate, NSMenuItem, NSView,
};
use objc2_foundation::NSString;
use ugu_platform_types::menu::{Action, Bar, EditRole, Entry, Key, MenuRole, SystemItem};

fn edit_selector(role: EditRole) -> Sel {
    match role {
        EditRole::Undo => sel!(undo:),
        EditRole::Redo => sel!(redo:),
        EditRole::Cut => sel!(cut:),
        EditRole::Copy => sel!(copy:),
        EditRole::Paste => sel!(paste:),
        EditRole::SelectAll => sel!(selectAll:),
    }
}

#[derive(Clone, Copy)]
struct ItemState {
    enabled: bool,
    edit: Option<Sel>,
}

struct Ivars {
    /// The app's window view.
    view: isize,
    choose: RefCell<Box<dyn FnMut(u32)>>,
    items: RefCell<HashMap<isize, ItemState>>,
}

define_class!(
    /// The target of the app's items and the delegate of every menu.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct Target;

    unsafe impl NSObjectProtocol for Target {}
    unsafe impl NSMenuDelegate for Target {}

    impl Target {
        #[unsafe(method(chooseAction:))]
        fn choose_action(&self, item: &NSMenuItem) {
            let edit = self.state_of(item).and_then(|state| state.edit);
            match edit.filter(|_| self.panel_has_keyboard()) {
                // SAFETY: an action sent up the responder chain, with the
                // item as the sender AppKit gives its own actions.
                Some(edit) => unsafe {
                    let _ = self.app().sendAction_to_from(edit, None, Some(item));
                },
                None => (self.ivars().choose.borrow_mut())(item.tag() as u32),
            }
        }

        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            let state = self.state_of(item);
            state.is_some_and(|state| {
                state.enabled || (state.edit.is_some() && self.panel_has_keyboard())
            })
        }

        /// Which item a key press belongs to. Answering no leaves the key
        /// to the window, where the app's shortcuts read it.
        #[unsafe(method(menuHasKeyEquivalent:forEvent:target:action:))]
        fn has_key_equivalent(
            &self,
            menu: &NSMenu,
            event: &NSEvent,
            target: *mut *mut AnyObject,
            action: *mut *const c_void,
        ) -> bool {
            self.answer_key(menu, event, target, action)
        }
    }
);

impl Target {
    fn new(mtm: MainThreadMarker, ivars: Ivars) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ivars);
        // SAFETY: NSObject's init takes no arguments.
        unsafe { msg_send![super(this), init] }
    }

    /// See `menuHasKeyEquivalent:forEvent:target:action:`.
    fn answer_key(
        &self,
        menu: &NSMenu,
        event: &NSEvent,
        target: *mut *mut AnyObject,
        action: *mut *const c_void,
    ) -> bool {
        let Some(item) = item_for(menu, event) else {
            return false;
        };
        let (to, selector) = match self.state_of(&item) {
            Some(state) if state.edit.is_some() && self.panel_has_keyboard() => {
                let this: *const Self = self;
                (this.cast_mut().cast(), sel!(chooseAction:))
            }
            Some(_) => return false,
            None => match item.action() {
                Some(selector) => (std::ptr::null_mut(), selector),
                None => return false,
            },
        };
        // SAFETY: AppKit passes where to write the two answers.
        unsafe {
            *target = to;
            *action.cast::<Sel>() = selector;
        }
        true
    }

    fn app(&self) -> Retained<NSApplication> {
        NSApplication::sharedApplication(self.mtm())
    }

    fn state_of(&self, item: &NSMenuItem) -> Option<ItemState> {
        if !item.target().is_some_and(|target| {
            let this: *const AnyObject = (self as *const Self).cast();
            std::ptr::eq(&*target, this)
        }) {
            return None;
        }
        self.ivars().items.borrow().get(&item.tag()).copied()
    }

    /// Whether another window than the app's, as a save panel, has the
    /// keyboard.
    fn panel_has_keyboard(&self) -> bool {
        // SAFETY: the view lives as long as the window, and `Menus`, whose
        // target this is, is dropped before it.
        let view = unsafe { &*(self.ivars().view as *const NSView) };
        let window = view.window();
        self.app().keyWindow().is_some_and(|key| {
            !window
                .as_ref()
                .is_some_and(|window| std::ptr::eq(&**window, &*key))
        })
    }
}

/// The item of `menu` whose key `event` is.
fn item_for(menu: &NSMenu, event: &NSEvent) -> Option<Retained<NSMenuItem>> {
    let typed = event.charactersIgnoringModifiers()?.to_string();
    let modifiers = event.modifierFlags();
    menu.itemArray().iter().find(|item| {
        same_key(
            &item.keyEquivalent().to_string(),
            item.keyEquivalentModifierMask(),
            &typed,
            modifiers,
        )
    })
}

fn same_key(
    item_key: &str,
    item_modifiers: NSEventModifierFlags,
    typed: &str,
    modifiers: NSEventModifierFlags,
) -> bool {
    let relevant = NSEventModifierFlags::Command
        | NSEventModifierFlags::Control
        | NSEventModifierFlags::Option
        | NSEventModifierFlags::Shift
        | NSEventModifierFlags::Function;
    !item_key.is_empty()
        && item_key.to_lowercase() == typed.to_lowercase()
        && (item_modifiers & relevant) == (modifiers & relevant)
}

fn modifier_flags(key: &Key) -> NSEventModifierFlags {
    let mut flags = NSEventModifierFlags::empty();
    for (held, flag) in [
        (key.command, NSEventModifierFlags::Command),
        (key.control, NSEventModifierFlags::Control),
        (key.option, NSEventModifierFlags::Option),
        (key.shift, NSEventModifierFlags::Shift),
    ] {
        if held {
            flags |= flag;
        }
    }
    flags
}

/// The macOS menu bar, for the lifetime of the app's window.
pub struct Menus {
    mtm: MainThreadMarker,
    target: Retained<Target>,
    shown: Option<Bar>,
    /// The items of the app's actions, in the order of the bar.
    items: Vec<Retained<NSMenuItem>>,
}

impl Menus {
    /// `choose` is called on the main thread with the id of the action
    /// chosen.
    ///
    /// # Safety
    /// On the main thread, with `view` the view of the app's window, which
    /// outlives the returned value.
    pub unsafe fn install(view: isize, choose: impl FnMut(u32) + 'static) -> Self {
        let mtm = MainThreadMarker::new().expect("the caller is on the main thread");
        Self {
            mtm,
            target: Target::new(
                mtm,
                Ivars {
                    view,
                    choose: RefCell::new(Box::new(choose)),
                    items: RefCell::default(),
                },
            ),
            shown: None,
            items: Vec::new(),
        }
    }

    pub fn show(&mut self, bar: Bar) {
        if self.shown.as_ref() == Some(&bar) {
            return;
        }
        if self.shown.as_ref().is_some_and(|old| old.same_shape(&bar)) {
            self.update(&bar);
        } else {
            self.build(&bar);
        }
        let states = actions(&bar)
            .map(|action| {
                let edit = action.edit.map(edit_selector);
                let enabled = action.enabled;
                (action.id as isize, ItemState { enabled, edit })
            })
            .collect();
        self.target.ivars().items.replace(states);
        self.shown = Some(bar);
    }

    fn update(&self, bar: &Bar) {
        let old = self.shown.as_ref().expect("only an existing bar updates");
        for ((item, action), before) in self.items.iter().zip(actions(bar)).zip(actions(old)) {
            if action.title != before.title {
                item.setTitle(&NSString::from_str(&action.title));
            }
            if action.checked != before.checked {
                set_checked(item, action.checked);
            }
            if action.key != before.key {
                set_key(item, action.key.as_ref());
            }
        }
    }

    fn build(&mut self, bar: &Bar) {
        let mtm = self.mtm;
        let app = NSApplication::sharedApplication(mtm);
        let root = NSMenu::new(mtm);
        self.items.clear();
        for menu in &bar.menus {
            let title = NSString::from_str(&menu.title);
            let list = NSMenu::initWithTitle(NSMenu::alloc(mtm), &title);
            list.setDelegate(Some(ProtocolObject::from_ref(&*self.target)));
            for entry in &menu.entries {
                list.addItem(&self.entry(entry, &app));
            }
            let holder = NSMenuItem::new(mtm);
            holder.setTitle(&title);
            holder.setSubmenu(Some(&list));
            root.addItem(&holder);
            match menu.role {
                MenuRole::Window => app.setWindowsMenu(Some(&list)),
                MenuRole::Help => app.setHelpMenu(Some(&list)),
                MenuRole::App | MenuRole::Plain => {}
            }
        }
        app.setMainMenu(Some(&root));
    }

    fn entry(&mut self, entry: &Entry, app: &NSApplication) -> Retained<NSMenuItem> {
        let mtm = self.mtm;
        match entry {
            Entry::Gap => NSMenuItem::separatorItem(mtm),
            Entry::Action(action) => {
                let item = new_item(mtm, &action.title, Some(sel!(chooseAction:)));
                // SAFETY: the target answers the action's selector and
                // outlives the menus, which `Menus` replaces first.
                unsafe { item.setTarget(Some(&self.target)) };
                item.setTag(action.id as isize);
                set_checked(&item, action.checked);
                set_key(&item, action.key.as_ref());
                self.items.push(item.clone());
                item
            }
            Entry::System {
                item: SystemItem::Services,
                title,
            } => {
                let item = new_item(mtm, title, None);
                let services =
                    NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
                item.setSubmenu(Some(&services));
                app.setServicesMenu(Some(&services));
                item
            }
            Entry::System { item: which, title } => {
                let (selector, key) = system_item(*which);
                let item = new_item(mtm, title, Some(selector));
                set_key(&item, key.as_ref());
                item
            }
        }
    }
}

fn actions(bar: &Bar) -> impl Iterator<Item = &Action> {
    bar.menus
        .iter()
        .flat_map(|menu| &menu.entries)
        .filter_map(|entry| match entry {
            Entry::Action(action) => Some(action),
            _ => None,
        })
}

/// What a system item asks AppKit for, and macOS's standard key for it.
fn system_item(item: SystemItem) -> (Sel, Option<Key>) {
    let command = |key: &str, option: bool| {
        Some(Key {
            key: key.to_owned(),
            command: true,
            control: false,
            option,
            shift: false,
        })
    };
    match item {
        SystemItem::Hide => (sel!(hide:), command("h", false)),
        SystemItem::HideOthers => (sel!(hideOtherApplications:), command("h", true)),
        SystemItem::ShowAll => (sel!(unhideAllApplications:), None),
        SystemItem::Minimize => (sel!(performMiniaturize:), command("m", false)),
        SystemItem::Zoom => (sel!(performZoom:), None),
        SystemItem::BringAllToFront => (sel!(arrangeInFront:), None),
        SystemItem::Services => unreachable!("the Services item is a submenu"),
    }
}

fn new_item(mtm: MainThreadMarker, title: &str, action: Option<Sel>) -> Retained<NSMenuItem> {
    // SAFETY: the selector is one the item's target or the responder chain
    // answers.
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::new(),
        )
    }
}

fn set_checked(item: &NSMenuItem, checked: bool) {
    item.setState(if checked {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
}

fn set_key(item: &NSMenuItem, key: Option<&Key>) {
    match key {
        Some(key) => {
            item.setKeyEquivalent(&NSString::from_str(&key.key));
            item.setKeyEquivalentModifierMask(modifier_flags(key));
        }
        None => {
            item.setKeyEquivalent(&NSString::new());
            item.setKeyEquivalentModifierMask(NSEventModifierFlags::empty());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_matches_in_either_case_with_exactly_its_modifiers() {
        let command = NSEventModifierFlags::Command;
        let shift_command = NSEventModifierFlags::Command | NSEventModifierFlags::Shift;
        // A key's own bits, as Caps Lock, are not part of the comparison.
        let caps_command = NSEventModifierFlags::Command | NSEventModifierFlags::CapsLock;
        assert!(same_key("v", command, "v", command));
        assert!(same_key("v", command, "v", caps_command));
        assert!(same_key("z", shift_command, "Z", shift_command));
        assert!(!same_key("z", command, "Z", shift_command));
        assert!(!same_key("v", command, "c", command));
        assert!(!same_key("", NSEventModifierFlags::empty(), "", command));
        // macOS adds Fn+E to Edit for its emoji panel; E alone is the eraser.
        let function = NSEventModifierFlags::Function;
        assert!(same_key("e", function, "e", function));
        assert!(!same_key("e", function, "e", NSEventModifierFlags::empty()));
    }

    #[test]
    fn modifiers_become_the_flags_a_menu_item_takes() {
        let key = Key {
            key: "s".to_owned(),
            command: true,
            control: false,
            option: true,
            shift: true,
        };
        assert_eq!(
            modifier_flags(&key),
            NSEventModifierFlags::Command
                | NSEventModifierFlags::Option
                | NSEventModifierFlags::Shift
        );
    }
}
