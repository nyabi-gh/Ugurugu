// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Keyboard shortcuts, as 2.2.13's: every menu action has a default key, a
//! few also answer to aliases, and the user may change the key. Only changed
//! keys are kept in the settings. Keys the canvas holds itself (Space to
//! pan, Alt to pick a colour, Shift and Alt while selecting) are not here.

use std::collections::{BTreeMap, HashMap};

use egui::Key;
use serde_json::{Map, Value};

/// A key pressed with exactly these modifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: Key,
}

const fn plain(key: Key) -> Chord {
    Chord {
        ctrl: false,
        alt: false,
        shift: false,
        key,
    }
}

const fn ctrl(key: Key) -> Chord {
    Chord {
        ctrl: true,
        ..plain(key)
    }
}

const fn ctrl_shift(key: Key) -> Chord {
    Chord {
        shift: true,
        ..ctrl(key)
    }
}

const fn alt(key: Key) -> Chord {
    Chord {
        alt: true,
        ..plain(key)
    }
}

const fn shift(key: Key) -> Chord {
    Chord {
        shift: true,
        ..plain(key)
    }
}

/// Keys that need Shift on some layouts and not on others, so Shift is not
/// part of their chord.
fn shift_is_part_of_key(key: Key) -> bool {
    matches!(
        key,
        Key::Plus
            | Key::Colon
            | Key::Pipe
            | Key::Questionmark
            | Key::Exclamationmark
            | Key::OpenCurlyBracket
            | Key::CloseCurlyBracket
    )
}

impl Chord {
    pub fn new(modifiers: egui::Modifiers, key: Key) -> Self {
        Self {
            // egui-winit sets both on Windows; `COMMAND` alone sets one.
            ctrl: modifiers.ctrl || modifiers.command,
            alt: modifiers.alt,
            shift: modifiers.shift && !shift_is_part_of_key(key),
            key,
        }
    }

    /// Space pans the canvas and Tab moves the focus, with any modifiers.
    /// A modifier alone, which egui also sends as a key, is no shortcut.
    pub fn assignable(self) -> bool {
        !matches!(
            self.key,
            Key::Space
                | Key::Tab
                | Key::ShiftLeft
                | Key::ShiftRight
                | Key::ControlLeft
                | Key::ControlRight
                | Key::AltLeft
                | Key::AltRight
                | Key::SuperLeft
                | Key::SuperRight
        )
    }

    /// As 2.2.13 writes it: "Ctrl+Alt+Shift+Key".
    pub fn text(self) -> String {
        let mut text = String::new();
        for (held, name) in [
            (self.ctrl, "Ctrl+"),
            (self.alt, "Alt+"),
            (self.shift, "Shift+"),
        ] {
            if held {
                text.push_str(name);
            }
        }
        text.push_str(match self.key {
            Key::Plus => "+",
            Key::Minus => "-",
            Key::Equals => "=",
            Key::Escape => "Esc",
            key => key.name(),
        });
        text
    }

    pub fn parse(text: &str) -> Option<Self> {
        let mut chord = plain(Key::A);
        let mut rest = text.trim();
        while let Some((name, after)) = rest.split_once('+').filter(|(name, _)| !name.is_empty()) {
            match name.to_ascii_lowercase().as_str() {
                "ctrl" => chord.ctrl = true,
                "alt" => chord.alt = true,
                "shift" => chord.shift = true,
                _ => return None,
            }
            rest = after;
        }
        chord.key = match rest {
            "Del" => Key::Delete,
            "Ins" => Key::Insert,
            name => Key::from_name(name)?,
        };
        chord.shift &= !shift_is_part_of_key(chord.key);
        Some(chord)
    }
}

/// The chord of a key press that egui-winit turns into a copy, cut or
/// paste event instead of a key, found as it finds them: the key's meaning,
/// else its place on the keyboard.
pub fn clipboard_chord(
    logical: &winit::keyboard::Key,
    physical: winit::keyboard::PhysicalKey,
    modifiers: winit::keyboard::ModifiersState,
) -> Option<Chord> {
    use winit::keyboard::{Key as Logical, KeyCode, NamedKey, PhysicalKey};
    let meant = match logical {
        Logical::Named(NamedKey::Delete) => Some(Key::Delete),
        Logical::Named(NamedKey::Insert) => Some(Key::Insert),
        Logical::Character(text) => Key::from_name(text),
        _ => None,
    };
    let placed = match physical {
        PhysicalKey::Code(KeyCode::KeyX) => Some(Key::X),
        PhysicalKey::Code(KeyCode::KeyC) => Some(Key::C),
        PhysicalKey::Code(KeyCode::KeyV) => Some(Key::V),
        PhysicalKey::Code(KeyCode::Delete) => Some(Key::Delete),
        PhysicalKey::Code(KeyCode::Insert) => Some(Key::Insert),
        _ => None,
    };
    let key = meant.or(placed)?;
    let (ctrl, shift) = (modifiers.control_key(), modifiers.shift_key());
    let clipboard = match key {
        Key::X | Key::C | Key::V => ctrl,
        Key::Delete => shift,
        Key::Insert => ctrl || shift,
        _ => false,
    };
    clipboard.then_some(Chord {
        ctrl,
        alt: modifiers.alt_key(),
        shift,
        key,
    })
}

macro_rules! actions {
    ($($action:ident $id:literal $label:literal [$($default:expr)?] [$($alias:expr),*];)*) => {
        /// Everything the menus do, and the canvas's Escape.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Action {
            $($action,)*
        }

        impl Action {
            pub const ALL: &[Self] = &[$(Self::$action,)*];

            /// Its name in the settings file: 2.2.13's action name without
            /// "Action".
            pub fn id(self) -> &'static str {
                match self {
                    $(Self::$action => $id,)*
                }
            }

            /// The message that names it.
            pub fn label(self) -> &'static str {
                match self {
                    $(Self::$action => $label,)*
                }
            }

            pub fn default_key(self) -> Option<Chord> {
                match self {
                    $(Self::$action => None$(.or(Some($default)))?,)*
                }
            }

            /// Further keys it answers to while no other action's key is
            /// the same.
            pub fn aliases(self) -> &'static [Chord] {
                match self {
                    $(Self::$action => {
                        const ALIASES: &[Chord] = &[$($alias),*];
                        ALIASES
                    })*
                }
            }
        }
    };
}

// 2.2.13's keys with Windows's standard ones as aliases, except where 3.0
// already differs: Save As, Quit and exporting have keys, transforming is
// Ctrl+T and so the animation bar Ctrl+Shift+T.
actions! {
    New "new" "file-new" [ctrl(Key::N)] [];
    Open "open" "file-open" [ctrl(Key::O)] [];
    Save "save" "file-save" [ctrl(Key::S)] [];
    SaveAs "saveAs" "file-save-as" [ctrl_shift(Key::S)] [];
    InsertImage "insertImage" "file-insert-image" [] [];
    Export "exportPng" "file-export-frame" [ctrl_shift(Key::E)] [];
    ExportGif "exportGif" "export-gif" [ctrl(Key::E)] [];
    ExportWebP "exportWebP" "export-webp" [] [];
    Quit "quit" "file-quit" [ctrl(Key::Q)] [];
    Undo "undo" "edit-undo" [ctrl(Key::Z)] [alt(Key::Backspace)];
    Redo "redo" "edit-redo" [ctrl(Key::Y)] [ctrl_shift(Key::Z)];
    Cut "cutSelection" "edit-cut" [ctrl(Key::X)] [shift(Key::Delete)];
    Copy "copySelection" "edit-copy" [ctrl(Key::C)] [ctrl(Key::Insert)];
    Paste "paste" "edit-paste" [ctrl(Key::V)] [shift(Key::Insert)];
    ImageSize "resizeImage" "edit-image-size" [] [];
    CanvasSize "resizeCanvas" "edit-canvas-size" [] [];
    SelectAll "selectAll" "edit-select-all" [ctrl(Key::A)] [];
    InvertSelection "invertSelection" "edit-invert-selection" [ctrl_shift(Key::I)] [];
    Deselect "deselectSelection" "edit-deselect" [ctrl(Key::D)] [];
    FillSelection "fillSelection" "edit-fill-selection" [alt(Key::Delete)] [];
    Restyle "editStrokeProperties" "restyle-selected" [] [];
    DeleteSelected "deleteSelection" "delete-selected" [plain(Key::Delete)] [];
    Transform "transformSelection" "transform-selection" [ctrl(Key::T)] [];
    FlipHorizontal "flipSelectionHorizontal" "flip-horizontal" [] [];
    FlipVertical "flipSelectionVertical" "flip-vertical" [] [];
    ApplyTransform "applySelectionTransform" "transform-apply" [plain(Key::Enter)] [];
    CancelTransform "cancelSelectionTransform" "transform-cancel" [] [];
    Escape "escapeCanvas" "escape-canvas" [plain(Key::Escape)] [];
    Settings "settings" "edit-settings" [] [];
    ZoomIn "zoomIn" "view-zoom-in" [ctrl(Key::Plus)] [ctrl(Key::Equals)];
    ZoomOut "zoomOut" "view-zoom-out" [ctrl(Key::Minus)] [];
    ActualPixels "actualSize" "view-actual-pixels" [ctrl(Key::Num1)] [];
    Fit "fit" "view-fit" [ctrl(Key::Num0)] [];
    Animate "play" "view-animate" [plain(Key::P)] [];
    Brush "brush" "tool-brush" [plain(Key::B)] [];
    Eraser "eraser" "tool-eraser" [plain(Key::E)] [];
    Select "lasso" "tool-select" [plain(Key::L)] [];
    Wand "wand" "tool-wand" [plain(Key::W)] [];
    Fill "bucket" "tool-fill" [plain(Key::G)] [];
    Text "text" "tool-text" [plain(Key::T)] [];
    Eyedropper "eyedropper" "tool-eyedropper" [plain(Key::I)] [];
    ImportTools "importToolPreset" "tools-import" [] [];
    ExportTools "exportToolPreset" "tools-export" [] [];
    AnimationBar "showTimeline" "window-animation-bar" [ctrl_shift(Key::T)] [];
    ToolSettings "toolSettings" "tool-settings" [] [];
    WobbleDock "wobbleDock" "wobble-dock" [] [];
    ColorDock "colorDock" "color-dock" [] [];
    ColorHistory "colorHistory" "color-history" [] [];
    Layers "layers" "layers" [] [];
    ResetLayout "resetPanelLayout" "window-reset-layout" [] [];
}

impl Action {
    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|action| action.id() == id)
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// The keys the user changed, as kept in the settings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shortcuts {
    /// `None` for an action whose key was cleared.
    changed: BTreeMap<Action, Option<Chord>>,
    /// Actions this version does not have, written back as they were.
    unknown: Map<String, Value>,
}

impl Shortcuts {
    pub fn key(&self, action: Action) -> Option<Chord> {
        self.changed
            .get(&action)
            .copied()
            .unwrap_or_else(|| action.default_key())
    }

    /// Gives `action` the key `key`, or nothing for `None`. Refuses a key
    /// another action answers to, returning that action, as 2.2.13 does.
    pub fn assign(&mut self, action: Action, key: Option<Chord>) -> Result<(), Action> {
        if let Some(key) = key
            && let Some(holder) = Keymap::new(self).action(key)
            && holder != action
        {
            return Err(holder);
        }
        if key == action.default_key() {
            self.changed.remove(&action);
        } else {
            self.changed.insert(action, key);
        }
        Ok(())
    }

    /// Every key back to its default; actions this version does not know
    /// stay, as the rest of the settings' unknown keys do.
    pub fn restored(&self) -> Self {
        Self {
            changed: BTreeMap::new(),
            unknown: self.unknown.clone(),
        }
    }

    pub fn parse(value: &Value) -> Self {
        let mut shortcuts = Self::default();
        let Some(object) = value.as_object() else {
            tracing::warn!(%value, "the shortcuts in the settings are not an object");
            return shortcuts;
        };
        for (id, text) in object {
            let Some(action) = Action::from_id(id) else {
                shortcuts.unknown.insert(id.clone(), text.clone());
                continue;
            };
            match text.as_str() {
                Some("") => {
                    shortcuts.changed.insert(action, None);
                }
                Some(text) => match Chord::parse(text).filter(|chord| chord.assignable()) {
                    Some(chord) if Some(chord) != action.default_key() => {
                        shortcuts.changed.insert(action, Some(chord));
                    }
                    Some(_) => {}
                    None => tracing::warn!(id, text, "a shortcut in the settings is not a key"),
                },
                None => tracing::warn!(id, %text, "a shortcut in the settings is not text"),
            }
        }
        shortcuts
    }

    /// `None` when nothing was changed, so the file leaves the key out.
    pub fn to_json(&self) -> Option<Value> {
        if self.changed.is_empty() && self.unknown.is_empty() {
            return None;
        }
        let mut object = self.unknown.clone();
        for (action, key) in &self.changed {
            let text = key.map(Chord::text).unwrap_or_default();
            object.insert(action.id().to_owned(), text.into());
        }
        Some(Value::Object(object))
    }
}

/// Which action each key runs.
pub struct Keymap {
    primary: Vec<Option<Chord>>,
    actions: HashMap<Chord, Action>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::new(&Shortcuts::default())
    }
}

impl Keymap {
    /// A key held by two actions, as a settings file edited by hand may
    /// have, stays with the first in `Action::ALL`. An alias goes to the
    /// first action naming it while no action has it as its key.
    pub fn new(shortcuts: &Shortcuts) -> Self {
        let mut primary = vec![None; Action::ALL.len()];
        let mut actions: HashMap<Chord, Action> = HashMap::new();
        for &action in Action::ALL {
            let Some(key) = shortcuts.key(action) else {
                continue;
            };
            if let Some(holder) = actions.get(&key) {
                tracing::warn!(
                    key = key.text(),
                    kept = holder.id(),
                    dropped = action.id(),
                    "two actions have the same shortcut"
                );
                continue;
            }
            primary[action.index()] = Some(key);
            actions.insert(key, action);
        }
        for &action in Action::ALL {
            for &alias in action.aliases() {
                actions.entry(alias).or_insert(action);
            }
        }
        Self { primary, actions }
    }

    pub fn key(&self, action: Action) -> Option<Chord> {
        self.primary[action.index()]
    }

    /// The key as menus show it, or nothing.
    pub fn text(&self, action: Action) -> String {
        self.key(action).map(Chord::text).unwrap_or_default()
    }

    pub fn action(&self, key: Chord) -> Option<Action> {
        self.actions.get(&key).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_has_its_own_name_and_default_key() {
        let mut ids: Vec<&str> = Action::ALL.iter().map(|action| action.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), Action::ALL.len());
        let keymap = Keymap::default();
        for &action in Action::ALL {
            assert_eq!(keymap.key(action), action.default_key(), "{action:?}");
            assert_eq!(Action::from_id(action.id()), Some(action));
            for &key in action.default_key().iter().chain(action.aliases()) {
                assert_eq!(keymap.action(key), Some(action), "{}", key.text());
            }
        }
    }

    #[test]
    fn keys_are_written_as_2_2_13_writes_them_and_read_back() {
        for (chord, text) in [
            (ctrl_shift(Key::S), "Ctrl+Shift+S"),
            (ctrl(Key::Plus), "Ctrl++"),
            (ctrl(Key::Minus), "Ctrl+-"),
            (ctrl(Key::Equals), "Ctrl+="),
            (alt(Key::Delete), "Alt+Delete"),
            (plain(Key::Escape), "Esc"),
            (ctrl(Key::Num1), "Ctrl+1"),
            (plain(Key::F5), "F5"),
            (alt(Key::Backspace), "Alt+Backspace"),
        ] {
            assert_eq!(chord.text(), text);
            assert_eq!(Chord::parse(text), Some(chord), "{text}");
        }
        assert_eq!(Chord::parse("ctrl+shift+s"), Some(ctrl_shift(Key::S)));
        assert_eq!(Chord::parse("Shift+Del"), Some(shift(Key::Delete)));
        assert_eq!(Chord::parse("Return"), Some(plain(Key::Enter)));
        assert_eq!(Chord::parse("Ctrl+Shift++"), Some(ctrl(Key::Plus)));
        for wrong in ["", "Ctrl+", "Hyper+A", "Ctrl+Nothing", "+A"] {
            assert_eq!(Chord::parse(wrong), None, "{wrong}");
        }
    }

    #[test]
    fn modifiers_must_match_exactly() {
        let keymap = Keymap::default();
        let pressed = |modifiers, key| keymap.action(Chord::new(modifiers, key));
        let command_shift = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
        assert_eq!(
            pressed(egui::Modifiers::COMMAND, Key::Z),
            Some(Action::Undo)
        );
        assert_eq!(pressed(command_shift, Key::Z), Some(Action::Redo));
        assert_eq!(
            pressed(egui::Modifiers::COMMAND, Key::T),
            Some(Action::Transform)
        );
        assert_eq!(pressed(command_shift, Key::T), Some(Action::AnimationBar));
        assert_eq!(pressed(egui::Modifiers::SHIFT, Key::B), None);
        assert_eq!(
            pressed(egui::Modifiers::ALT, Key::Delete),
            Some(Action::FillSelection)
        );
        // `+` takes Shift on a US keyboard and not on others.
        assert_eq!(pressed(command_shift, Key::Plus), Some(Action::ZoomIn));
    }

    #[test]
    fn a_key_another_action_answers_to_is_refused() {
        let mut shortcuts = Shortcuts::default();
        assert_eq!(
            shortcuts.assign(Action::Fit, Some(ctrl(Key::N))),
            Err(Action::New)
        );
        // An alias counts too, as in 2.2.13.
        assert_eq!(
            shortcuts.assign(Action::Fit, Some(ctrl(Key::Equals))),
            Err(Action::ZoomIn)
        );
        assert_eq!(shortcuts, Shortcuts::default());
        // Its own key and alias are not a conflict.
        assert_eq!(
            shortcuts.assign(Action::ZoomIn, Some(ctrl(Key::Equals))),
            Ok(())
        );
        assert_eq!(shortcuts.key(Action::ZoomIn), Some(ctrl(Key::Equals)));
        // Freed by clearing it, a key can be given to another action.
        assert_eq!(shortcuts.assign(Action::New, None), Ok(()));
        assert_eq!(shortcuts.assign(Action::Fit, Some(ctrl(Key::N))), Ok(()));
        let keymap = Keymap::new(&shortcuts);
        assert_eq!(keymap.action(ctrl(Key::N)), Some(Action::Fit));
        assert_eq!(keymap.key(Action::New), None);
        assert_eq!(keymap.text(Action::New), "");
    }

    #[test]
    fn an_action_s_key_wins_over_another_s_alias() {
        let shortcuts = Shortcuts::parse(&serde_json::json!({"play": "Ctrl+Shift+Z"}));
        let keymap = Keymap::new(&shortcuts);
        assert_eq!(keymap.action(ctrl_shift(Key::Z)), Some(Action::Animate));
        assert_eq!(keymap.action(ctrl(Key::Y)), Some(Action::Redo));
        // A cleared key leaves the aliases, as in 2.2.13.
        let mut shortcuts = Shortcuts::default();
        shortcuts.assign(Action::Undo, None).unwrap();
        assert_eq!(
            shortcuts.assign(Action::Fit, Some(alt(Key::Backspace))),
            Err(Action::Undo)
        );
    }

    #[test]
    fn only_changed_keys_are_kept_and_read_back() {
        let mut shortcuts = Shortcuts::default();
        assert_eq!(shortcuts.to_json(), None);
        shortcuts.assign(Action::Fit, Some(plain(Key::F))).unwrap();
        shortcuts.assign(Action::Animate, None).unwrap();
        shortcuts.assign(Action::Undo, Some(ctrl(Key::Z))).unwrap();
        let json = shortcuts.to_json().unwrap();
        assert_eq!(json, serde_json::json!({"fit": "F", "play": ""}));
        assert_eq!(Shortcuts::parse(&json), shortcuts);
        // Back to the default, it is left out again.
        shortcuts
            .assign(Action::Fit, Some(ctrl(Key::Num0)))
            .unwrap();
        shortcuts
            .assign(Action::Animate, Some(plain(Key::P)))
            .unwrap();
        assert_eq!(shortcuts.to_json(), None);
    }

    #[test]
    fn unreadable_keys_fall_back_alone_and_unknown_actions_stay() {
        let shortcuts = Shortcuts::parse(&serde_json::json!({
            "fit": "Ctrl+Nothing",
            "zoomIn": 3,
            "play": "Space",
            "save": "Ctrl+S",
            "brush": "N",
            "morphCanvas": "Ctrl+M",
        }));
        assert_eq!(shortcuts.key(Action::Fit), Some(ctrl(Key::Num0)));
        assert_eq!(shortcuts.key(Action::ZoomIn), Some(ctrl(Key::Plus)));
        assert_eq!(shortcuts.key(Action::Animate), Some(plain(Key::P)));
        assert_eq!(shortcuts.key(Action::Brush), Some(plain(Key::N)));
        assert_eq!(
            shortcuts.to_json().unwrap(),
            serde_json::json!({"brush": "N", "morphCanvas": "Ctrl+M"})
        );
        assert_eq!(
            shortcuts.restored().to_json().unwrap(),
            serde_json::json!({"morphCanvas": "Ctrl+M"})
        );
        assert_eq!(
            Shortcuts::parse(&serde_json::json!([1])),
            Shortcuts::default()
        );
    }

    #[test]
    fn a_key_given_twice_by_hand_stays_with_the_first_action() {
        let shortcuts = Shortcuts::parse(&serde_json::json!({"brush": "Ctrl+N"}));
        let keymap = Keymap::new(&shortcuts);
        assert_eq!(keymap.action(ctrl(Key::N)), Some(Action::New));
        assert_eq!(keymap.key(Action::Brush), None);
        assert_eq!(keymap.action(plain(Key::B)), None);
    }

    #[test]
    fn clipboard_chords_are_found_as_egui_winit_finds_them() {
        use winit::keyboard::{Key as Logical, KeyCode, ModifiersState, NamedKey, PhysicalKey};
        let character = |text: &str| Logical::Character(text.into());
        let code = PhysicalKey::Code;
        let control = ModifiersState::CONTROL;
        let shift_held = ModifiersState::SHIFT;
        let delete = Logical::Named(NamedKey::Delete);
        let insert = Logical::Named(NamedKey::Insert);
        for (logical, physical, modifiers, chord) in [
            (character("c"), KeyCode::KeyC, control, Some(ctrl(Key::C))),
            (
                character("v"),
                KeyCode::KeyV,
                control | shift_held,
                Some(ctrl_shift(Key::V)),
            ),
            (
                delete.clone(),
                KeyCode::Delete,
                shift_held,
                Some(shift(Key::Delete)),
            ),
            (insert, KeyCode::Insert, control, Some(ctrl(Key::Insert))),
            // A layout whose letters are not Latin falls back on the place.
            (
                character("\u{314a}"),
                KeyCode::KeyC,
                control,
                Some(ctrl(Key::C)),
            ),
            // Keys egui-winit passes on as keys.
            (character("c"), KeyCode::KeyC, ModifiersState::empty(), None),
            (delete, KeyCode::Delete, ModifiersState::ALT, None),
            (character("z"), KeyCode::KeyZ, control, None),
        ] {
            assert_eq!(
                clipboard_chord(&logical, code(physical), modifiers),
                chord,
                "{logical:?}"
            );
        }
    }

    #[test]
    fn space_and_tab_cannot_be_given() {
        assert!(!plain(Key::Space).assignable());
        assert!(!ctrl(Key::Space).assignable());
        assert!(!shift(Key::Tab).assignable());
        assert!(!ctrl(Key::ControlLeft).assignable());
        assert!(!shift(Key::ShiftRight).assignable());
        assert!(plain(Key::F2).assignable());
    }
}
