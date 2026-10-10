// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Using the whole window from the keyboard alone (M5-14), driven as the
//! render thread drives it, without a GPU.

use egui::accesskit::{Role, TreeUpdate};
use egui::{Event, Key, Modifiers, Popup, Pos2, RawInput, Rect, vec2};
use ugu_core::ops::Rgba8;
use ugu_session::Session;

use crate::canvas::Canvas;
use crate::clipboard::Clipboard;
use crate::files::Files;
use crate::i18n::tr;
use crate::settings::Store;

use super::{Panels, Parts};

const NONE: Modifiers = Modifiers::NONE;

struct Window {
    ctx: egui::Context,
    canvas: Canvas,
    files: Files,
    clipboard: Clipboard,
    settings: Store,
    panels: Panels,
    tree: Option<TreeUpdate>,
}

impl Window {
    fn new() -> Self {
        let ctx = egui::Context::default();
        // For the names and roles of what has the focus.
        ctx.enable_accesskit();
        let mut window = Self {
            ctx,
            canvas: Canvas::new(Files::new_canvas(), |_| {}),
            files: Files::new(0, |_| {}),
            clipboard: Clipboard::new(|_| {}),
            settings: Store::open(None),
            panels: Panels::default(),
            tree: None,
        };
        window.frame(Vec::new());
        window.frame(Vec::new());
        window
    }

    fn frame(&mut self, events: Vec<Event>) {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 900.0))),
            events,
            focused: true,
            ..RawInput::default()
        };
        let Self {
            ctx,
            canvas,
            files,
            clipboard,
            settings,
            panels,
            ..
        } = self;
        let mut output = ctx.run_ui(input, |ui| {
            super::window(
                ui,
                Parts {
                    canvas,
                    files,
                    clipboard,
                    settings,
                    panels,
                    typed: &[],
                    warning: None,
                    diagnostics: None,
                    probe: None,
                },
            );
        });
        // No GPU takes the font atlas here.
        output.textures_delta.clear();
        if let Some(tree) = output.platform_output.accesskit_update.take() {
            self.tree = Some(tree);
        }
    }

    /// A key pressed and let go, then a frame for focus and popups to settle.
    fn key(&mut self, modifiers: Modifiers, key: Key) {
        let event = |pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        self.frame(vec![event(true), event(false)]);
        self.frame(Vec::new());
    }

    /// The name and role of what has the focus. A field named by a label
    /// next to it takes the label's text, as UI Automation names it.
    fn focus(&self) -> Option<(String, Role)> {
        let id = self.ctx.memory(|memory| memory.focused())?;
        let nodes = &self.tree.as_ref()?.nodes;
        let node = |id| {
            nodes
                .iter()
                .find(|(node, _)| *node == id)
                .map(|(_, node)| node)
        };
        let focused = node(id.accesskit_id())?;
        let label = focused.label().map(str::to_owned).or_else(|| {
            let by = node(*focused.labelled_by().first()?)?;
            by.label().or(by.value()).map(str::to_owned)
        });
        Some((label.unwrap_or_default(), focused.role()))
    }

    fn label(&self) -> String {
        self.focus().map(|(label, _)| label).unwrap_or_default()
    }

    /// Tabs until `found` holds for the focus.
    fn tab_to(&mut self, found: impl Fn(&str, Role) -> bool) {
        for _ in 0..200 {
            if let Some((label, role)) = self.focus()
                && found(&label, role)
            {
                return;
            }
            self.key(NONE, Key::Tab);
        }
        panic!("Tab did not reach it");
    }

    fn tab_to_label(&mut self, label: &str) {
        self.tab_to(|focused, _| focused.starts_with(label));
    }

    fn popup(&self) -> bool {
        Popup::is_any_open(&self.ctx)
    }

    fn dialog(&self) -> bool {
        self.ctx.memory(|memory| memory.top_modal_layer().is_some())
    }
}

#[test]
fn tab_goes_round_every_control_and_each_has_a_name() {
    let mut window = Window::new();
    let mut walk = Vec::new();
    for _ in 0..200 {
        window.key(NONE, Key::Tab);
        let label = window.label();
        if walk.first() == Some(&label) {
            break;
        }
        walk.push(label);
    }
    assert_eq!(walk[0], tr("menu-file"));
    assert!(walk.len() < 200, "Tab does not come round");
    assert!(walk.iter().all(|label| !label.is_empty()), "{walk:?}");
    for label in [
        tr("menu-window"),
        tr("dock-width-left"),
        tr("dock-width-right"),
        tr("layer-add"),
        tr("motion-style"),
    ] {
        assert!(
            walk.iter().any(|walked| walked == label),
            "{label} in {walk:?}"
        );
    }
}

#[test]
fn the_menus_open_from_the_keyboard() {
    let mut window = Window::new();
    window.key(NONE, Key::F10);
    assert_eq!(window.label(), tr("menu-file"));
    assert!(!window.popup());
    window.key(NONE, Key::ArrowDown);
    assert!(window.popup(), "Down opens the focused menu");
    window.key(NONE, Key::ArrowRight);
    assert_eq!(window.label(), tr("menu-edit"));
    assert!(window.popup(), "Right opens the next menu");
    window.key(NONE, Key::ArrowLeft);
    window.key(NONE, Key::ArrowLeft);
    assert_eq!(window.label(), tr("menu-window"), "Left goes round");
    window.key(NONE, Key::Escape);
    assert!(!window.popup());
    assert_eq!(window.label(), tr("menu-window"), "the focus comes back");

    window.key(Modifiers::ALT, Key::V);
    assert_eq!(window.label(), tr("menu-view"));
    assert!(window.popup());
    window.key(NONE, Key::Escape);
    // Enter presses a focused button while nothing waits to be applied.
    window.key(NONE, Key::Enter);
    assert!(window.popup(), "Enter opens the focused menu");
}

#[test]
fn a_dialog_from_a_menu_closes_on_escape_and_gives_the_focus_back() {
    let mut window = Window::new();
    window.key(Modifiers::ALT, Key::E);
    window.tab_to_label(tr("edit-settings"));
    window.key(NONE, Key::Space);
    assert!(window.dialog());
    assert!(!window.popup(), "the menu closes when an item is chosen");
    window.frame(Vec::new());
    assert_eq!(
        window.label(),
        tr("settings-general"),
        "the dialog takes the focus"
    );
    window.key(NONE, Key::Escape);
    // The dialog asked for the frame that shows it closed.
    assert!(window.ctx.has_requested_repaint());
    window.frame(Vec::new());
    assert!(!window.dialog());
    assert_eq!(window.label(), tr("menu-edit"));
}

#[test]
fn a_list_chosen_from_the_keyboard_closes_and_keeps_the_focus() {
    let mut window = Window::new();
    let before = window.canvas.session().document().wobble.motion.style;
    window.tab_to_label(tr("motion-style"));
    window.key(NONE, Key::Space);
    assert!(window.popup());
    window.key(NONE, Key::ArrowDown);
    window.key(NONE, Key::ArrowDown);
    window.key(NONE, Key::Space);
    assert!(!window.popup());
    assert_ne!(
        window.canvas.session().document().wobble.motion.style,
        before
    );
    assert_eq!(window.label(), tr("motion-style"));
}

#[test]
fn arrows_widen_a_dock_area_from_its_edge() {
    let mut window = Window::new();
    window.tab_to_label(tr("dock-width-left"));
    window.key(NONE, Key::ArrowRight);
    let before = window.panels.layout.left.width;
    window.key(NONE, Key::ArrowRight);
    assert_eq!(window.panels.layout.left.width, before + 16.0);
    assert_eq!(window.label(), tr("dock-width-left"), "the focus stays");
    window.tab_to_label(tr("dock-width-right"));
    window.key(NONE, Key::ArrowLeft);
    let before = window.panels.layout.right.width;
    window.key(NONE, Key::ArrowRight);
    assert_eq!(window.panels.layout.right.width, before - 16.0);
}

#[test]
fn a_colour_can_be_typed() {
    let mut window = Window::new();
    window.tab_to(|_, role| role == Role::TextInput);
    window.key(Modifiers::COMMAND, Key::A);
    window.frame(vec![Event::Text("#8000FF00".to_owned())]);
    window.key(NONE, Key::Enter);
    assert_eq!(window.canvas.session().pen.color, Rgba8([0, 255, 0, 128]));
    // Escape leaves the colour.
    window.tab_to(|_, role| role == Role::TextInput);
    window.key(Modifiers::COMMAND, Key::A);
    window.frame(vec![Event::Text("#FFFF0000".to_owned())]);
    window.key(NONE, Key::Escape);
    assert_eq!(window.canvas.session().pen.color, Rgba8([0, 255, 0, 128]));
}

#[test]
fn unsaved_changes_are_answered_from_the_keyboard() {
    let mut window = Window::new();
    let _ = window.canvas.edit(Session::add_layer);
    window.key(Modifiers::COMMAND, Key::N);
    assert!(window.dialog());
    window.key(NONE, Key::Escape);
    assert!(!window.dialog());
    assert_eq!(window.canvas.session().document().layers.len(), 2);
    window.key(Modifiers::COMMAND, Key::N);
    // N is "Don't save", as in 2.2.13; then the new document's size.
    window.key(NONE, Key::N);
    window.frame(Vec::new());
    assert_eq!(window.label(), tr("size-width"));
    window.key(NONE, Key::Enter);
    window.frame(Vec::new());
    assert!(!window.dialog());
    assert_eq!(window.canvas.session().document().layers.len(), 1);
}

#[test]
fn enter_chooses_a_layer_row() {
    let mut window = Window::new();
    let first = window.canvas.session().current_layer();
    let _ = window.canvas.edit(Session::add_layer);
    assert_ne!(window.canvas.session().current_layer(), first);
    let name = window
        .canvas
        .session()
        .document()
        .layer(first)
        .unwrap()
        .name
        .clone();
    window.tab_to_label(&name);
    window.key(NONE, Key::Enter);
    assert_eq!(window.canvas.session().current_layer(), first);
}

#[test]
fn a_new_document_s_size_is_chosen_from_the_keyboard() {
    let mut window = Window::new();
    window.key(Modifiers::COMMAND, Key::N);
    window.frame(Vec::new());
    assert!(window.dialog());
    assert_eq!(
        window.label(),
        tr("size-width"),
        "the width takes the focus"
    );
    window.tab_to_label("32 × 32");
    window.key(NONE, Key::Space);
    window.tab_to_label(tr("dialog-ok"));
    window.key(NONE, Key::Space);
    window.frame(Vec::new());
    assert!(!window.dialog());
    assert_eq!(window.canvas.session().document().canvas, [32, 32]);

    // Arrows change the field, and Enter in it takes the size.
    window.key(Modifiers::COMMAND, Key::N);
    window.frame(Vec::new());
    window.key(NONE, Key::ArrowDown);
    window.key(NONE, Key::ArrowDown);
    window.key(NONE, Key::Enter);
    window.frame(Vec::new());
    assert!(!window.dialog());
    assert_eq!(window.canvas.session().document().canvas, [30, 32]);
}
