// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! What the menus hold, and running their actions from a menu or a key.

use ugu_session::{Session, Tool};

use crate::canvas::Canvas;
use crate::clipboard::Clipboard;
use crate::files::{self, Files};
use crate::i18n::tr;
use crate::shortcuts::{Action, Chord, Keymap};

use super::{Panels, Shown, report_bool, resize, restyle, settings};

pub enum Entry {
    Do(Action),
    Gap,
}

use Action as A;
use Entry::{Do, Gap};

pub const FILE: &[Entry] = &[
    Do(A::New),
    Do(A::Open),
    Gap,
    Do(A::Save),
    Do(A::SaveAs),
    Gap,
    Do(A::InsertImage),
    Do(A::Export),
    Gap,
    Do(A::Quit),
];

pub const EDIT: &[Entry] = &[
    Do(A::Undo),
    Do(A::Redo),
    Gap,
    Do(A::Cut),
    Do(A::Copy),
    Do(A::Paste),
    Gap,
    Do(A::ImageSize),
    Do(A::CanvasSize),
    Gap,
    Do(A::SelectAll),
    Do(A::InvertSelection),
    Do(A::Deselect),
    Do(A::FillSelection),
    Do(A::Restyle),
    Do(A::DeleteSelected),
    Gap,
    Do(A::Transform),
    Do(A::FlipHorizontal),
    Do(A::FlipVertical),
    Do(A::ApplyTransform),
    Do(A::CancelTransform),
    Gap,
    Do(A::Settings),
];

pub const VIEW: &[Entry] = &[
    Do(A::ZoomIn),
    Do(A::ZoomOut),
    Do(A::ActualPixels),
    Do(A::Fit),
    Gap,
    Do(A::Animate),
];

pub const TOOLS: &[Entry] = &[
    Do(A::Brush),
    Do(A::Eraser),
    Do(A::Select),
    Do(A::Wand),
    Do(A::Fill),
    Do(A::Text),
    Do(A::Eyedropper),
];

/// Also behind the quick access Panels button.
pub const WINDOW: &[Entry] = &[
    Do(A::AnimationBar),
    Gap,
    Do(A::ToolSettings),
    Do(A::WobbleDock),
    Do(A::ColorDock),
    Do(A::ColorHistory),
    Do(A::Layers),
    Gap,
    Do(A::ResetLayout),
];

pub const MENUS: [(&str, &[Entry]); 5] = [
    ("menu-file", FILE),
    ("menu-edit", EDIT),
    ("menu-view", VIEW),
    ("menu-tools", TOOLS),
    ("menu-window", WINDOW),
];

pub fn tool(action: Action) -> Option<Tool> {
    Some(match action {
        A::Brush => Tool::Pen,
        A::Eraser => Tool::Eraser,
        A::Select => Tool::Select,
        A::Wand => Tool::Wand,
        A::Fill => Tool::Fill,
        A::Text => Tool::Text,
        A::Eyedropper => Tool::Eyedropper,
        _ => return None,
    })
}

/// A panel the Window menu shows and hides.
fn panel(action: Action, shown: &mut Shown) -> Option<&mut bool> {
    Some(match action {
        A::AnimationBar => &mut shown.animation_bar,
        A::ToolSettings => &mut shown.tool_settings,
        A::WobbleDock => &mut shown.wobble,
        A::ColorDock => &mut shown.color,
        A::ColorHistory => &mut shown.color_history,
        A::Layers => &mut shown.layers,
        _ => return None,
    })
}

/// Whether it can run now. A key runs it only then, as 2.2.13's disabled
/// actions ignore their keys.
fn enabled(action: Action, canvas: &Canvas) -> bool {
    let session = canvas.session();
    let selected = session.selection().is_some();
    let pending = session.pending().is_some();
    match action {
        A::Undo => session.undo_label().is_some(),
        A::Redo => session.redo_label().is_some(),
        A::Cut
        | A::Copy
        | A::InvertSelection
        | A::Deselect
        | A::FillSelection
        | A::DeleteSelected
        | A::FlipHorizontal
        | A::FlipVertical => selected,
        A::Restyle => restyle::available(canvas),
        A::Transform => selected && !pending,
        // Enter also places text, as it did before shortcuts could change.
        A::ApplyTransform => pending || session.placed_text().is_some(),
        A::CancelTransform => pending,
        A::Animate => canvas.animation_allowed(),
        _ => true,
    }
}

/// Whether a menu shows it as on, for those that turn something on.
fn checked(action: Action, canvas: &Canvas, shown: &Shown) -> Option<bool> {
    if let Some(tool) = tool(action) {
        return Some(canvas.session().tool() == tool);
    }
    let mut shown = *shown;
    if let Some(on) = panel(action, &mut shown) {
        return Some(*on);
    }
    (action == A::Animate).then(|| canvas.is_playing())
}

pub fn label(action: Action, canvas: &Canvas) -> &'static str {
    // As 2.2.13: without the animation, there is only the image.
    if action == A::Export && !canvas.animation_allowed() {
        return tr("file-export-image");
    }
    tr(action.label())
}

fn tip(action: Action) -> Option<&'static str> {
    Some(tr(match action {
        A::Copy => "edit-copy-tip",
        A::Paste => "edit-paste-tip",
        A::FillSelection => "edit-fill-selection-tip",
        A::Restyle => "restyle-selected-tip",
        _ => return None,
    }))
}

/// Shows `entries` as menu items. Returns the one chosen.
pub fn items(
    ui: &mut egui::Ui,
    entries: &[Entry],
    canvas: &Canvas,
    panels: &Panels,
) -> Option<Action> {
    let mut chosen = None;
    for entry in entries {
        let &Do(action) = entry else {
            ui.separator();
            continue;
        };
        let text = label(action, canvas);
        let mut button = match checked(action, canvas, &panels.shown) {
            Some(on) => egui::Button::selectable(on, text),
            None => egui::Button::new(text),
        };
        let key = panels.keys.text(action);
        if !key.is_empty() {
            button = button.shortcut_text(key);
        }
        let mut response = ui.add_enabled(enabled(action, canvas), button);
        if let Some(tip) = tip(action) {
            response = response.on_hover_text(tip);
        }
        if response.clicked() {
            chosen = Some(action);
        }
    }
    chosen
}

pub fn run(
    action: Action,
    canvas: &mut Canvas,
    files: &mut Files,
    clipboard: &mut Clipboard,
    panels: &mut Panels,
) {
    if let Some(tool) = tool(action) {
        canvas.edit(|session| session.set_tool(tool));
        return;
    }
    if let Some(on) = panel(action, &mut panels.shown) {
        *on = !*on;
        return;
    }
    let canvas_size = canvas.session().document().canvas;
    match action {
        A::New => files.request(files::Action::New, canvas),
        A::Open => files.request(files::Action::Open, canvas),
        A::Save => files.save(canvas),
        A::SaveAs => files.save_as(),
        A::InsertImage => files.insert_image(),
        A::Export => files.export_image(),
        A::Quit => files.request(files::Action::Close, canvas),
        A::Undo => report_bool(canvas.edit(Session::undo)),
        A::Redo => report_bool(canvas.edit(Session::redo)),
        A::Cut => clipboard.cut(canvas),
        A::Copy => clipboard.copy(canvas),
        A::Paste => clipboard.paste(canvas),
        A::ImageSize => {
            panels.resize = Some(resize::Dialog::Image(resize::ImageSize::new(canvas_size)));
        }
        A::CanvasSize => {
            panels.resize = Some(resize::Dialog::Canvas(resize::CanvasSize::new(canvas_size)));
        }
        A::SelectAll => {
            canvas.edit(Session::select_all);
        }
        A::InvertSelection => {
            canvas.edit(Session::invert_selection);
        }
        A::Deselect => {
            canvas.edit(Session::deselect);
        }
        A::FillSelection => canvas.fill_selection(),
        A::Restyle => restyle::open(canvas, panels),
        A::DeleteSelected => canvas.delete_selected(),
        A::Transform => canvas.begin_transform(),
        A::FlipHorizontal => canvas.flip(true),
        A::FlipVertical => canvas.flip(false),
        A::ApplyTransform => canvas.apply_pending(),
        A::CancelTransform => canvas.cancel_transform(),
        A::Escape => canvas.escape(),
        A::Settings => panels.settings = Some(settings::Dialog::default()),
        A::ZoomIn => canvas.zoom_in_place(1.0),
        A::ZoomOut => canvas.zoom_in_place(-1.0),
        A::ActualPixels => canvas.zoom_to(1.0),
        A::Fit => canvas.fit(),
        A::Animate => canvas.toggle_playback(),
        A::ResetLayout => panels.shown = Shown::default(),
        A::Brush
        | A::Eraser
        | A::Select
        | A::Wand
        | A::Fill
        | A::Text
        | A::Eyedropper
        | A::AnimationBar
        | A::ToolSettings
        | A::WobbleDock
        | A::ColorDock
        | A::ColorHistory
        | A::Layers => unreachable!("handled above"),
    }
}

/// Runs the actions whose keys were pressed in this frame's input, and those
/// of `typed`: the chords egui turns into clipboard events (Ctrl+C, X and V
/// and their Insert and Delete forms), seen before egui. Text fields and
/// dialogs keep their keys, Enter that commits a composition included.
pub fn shortcuts(
    ctx: &egui::Context,
    canvas: &mut Canvas,
    files: &mut Files,
    clipboard: &mut Clipboard,
    panels: &mut Panels,
    typed: &[Chord],
) {
    for (chord, action) in take_pressed(ctx, &panels.keys, typed) {
        // The IME test counts these.
        tracing::debug!("canvas {}", chord.text());
        if enabled(action, canvas) {
            run(action, canvas, files, clipboard, panels);
        }
    }
}

/// Takes the keys of actions out of this frame's input, unless a text field,
/// a dialog or an open menu has the keyboard.
fn take_pressed(ctx: &egui::Context, keys: &Keymap, typed: &[Chord]) -> Vec<(Chord, Action)> {
    // Not `egui_wants_keyboard_input`, which also holds for a focused row or
    // button and would leave the shortcuts dead after clicking a layer. An
    // open menu closes on Escape, which would otherwise be the canvas's.
    if ctx.text_edit_focused()
        || ctx.memory(|memory| memory.top_modal_layer().is_some())
        || egui::Popup::is_any_open(ctx)
    {
        return Vec::new();
    }
    // A whole chord can arrive within one frame, so each key is matched with
    // the modifiers it was pressed with, not the frame's last state.
    let mut pressed = Vec::new();
    ctx.input_mut(|input| {
        input.events.retain(|event| {
            let &egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            else {
                return true;
            };
            let chord = Chord::new(modifiers, key);
            let Some(action) = keys.action(chord) else {
                return true;
            };
            pressed.push((chord, action));
            false
        });
    });
    pressed.extend(
        typed
            .iter()
            .filter_map(|&chord| Some((chord, keys.action(chord)?))),
    );
    pressed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menus_hold_every_action_but_escape_once() {
        let mut held: Vec<Action> = MENUS
            .iter()
            .flat_map(|(_, entries)| entries.iter())
            .filter_map(|entry| match entry {
                Do(action) => Some(*action),
                Gap => None,
            })
            .collect();
        let count = held.len();
        held.sort_unstable();
        held.dedup();
        assert_eq!(held.len(), count, "an action is in the menus twice");
        let missing: Vec<&Action> = Action::ALL
            .iter()
            .filter(|action| !held.contains(action))
            .collect();
        // Escape is the canvas's, as 2.2.13's escapeCanvasAction.
        assert_eq!(missing, [&A::Escape]);
    }

    /// One frame with `keys` pressed, a text field focused or not, and what
    /// the shortcuts took from it.
    fn frame(
        ctx: &egui::Context,
        keys: &[(egui::Modifiers, egui::Key)],
        typed: &[Chord],
        focus_text: bool,
    ) -> Vec<Action> {
        let mut input = egui::RawInput::default();
        for &(modifiers, key) in keys {
            input.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            });
        }
        let mut taken = Vec::new();
        let mut output = ctx.run_ui(input, |ui| {
            taken = take_pressed(ui.ctx(), &Keymap::default(), typed)
                .into_iter()
                .map(|(_, action)| action)
                .collect();
            let mut text = String::new();
            let field = ui.text_edit_singleline(&mut text);
            if focus_text {
                field.request_focus();
            } else {
                field.surrender_focus();
            }
        });
        // No renderer takes the font atlas.
        output.textures_delta.clear();
        taken
    }

    #[test]
    fn a_text_field_keeps_the_keys() {
        let ctx = egui::Context::default();
        let command = egui::Modifiers::COMMAND;
        let none = egui::Modifiers::NONE;
        let keys = [
            (command, egui::Key::Z),
            (none, egui::Key::Enter),
            (none, egui::Key::B),
        ];
        let copy = [Chord::new(command, egui::Key::C)];
        // Focus moves at the end of a frame.
        frame(&ctx, &[], &[], true);
        assert_eq!(frame(&ctx, &keys, &copy, true), []);
        frame(&ctx, &[], &[], false);
        assert_eq!(
            frame(&ctx, &keys, &copy, false),
            [A::Undo, A::ApplyTransform, A::Brush, A::Copy]
        );
        // Nor do they reach the canvas while a menu is open.
        egui::Popup::open_id(&ctx, egui::Id::new("menu"));
        assert_eq!(frame(&ctx, &keys, &copy, false), []);
    }
}
