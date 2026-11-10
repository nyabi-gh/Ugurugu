// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The app's menus, for an OS whose own menu bar shows them.

/// Everything the menu bar holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bar {
    pub menus: Vec<Menu>,
}

impl Bar {
    /// Whether `other` has the same menus and entries in the same order, so
    /// that only titles, states and keys differ.
    pub fn same_shape(&self, other: &Self) -> bool {
        self.menus.len() == other.menus.len()
            && self.menus.iter().zip(&other.menus).all(|(one, two)| {
                one.role == two.role
                    && one.entries.len() == two.entries.len()
                    && one.entries.iter().zip(&two.entries).all(same_entry)
            })
    }
}

fn same_entry((one, two): (&Entry, &Entry)) -> bool {
    match (one, two) {
        (Entry::Gap, Entry::Gap) => true,
        (Entry::Action(one), Entry::Action(two)) => one.id == two.id,
        (Entry::System { item: one, .. }, Entry::System { item: two, .. }) => one == two,
        _ => false,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    pub title: String,
    pub role: MenuRole,
    pub entries: Vec<Entry>,
}

/// What the OS does with a menu besides showing it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuRole {
    App,
    Plain,
    Window,
    Help,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Gap,
    Action(Action),
    /// Something the OS does itself, titled in the app's language.
    System {
        item: SystemItem,
        title: String,
    },
}

/// One of the app's actions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub id: u32,
    pub title: String,
    pub enabled: bool,
    pub checked: bool,
    pub key: Option<Key>,
    pub edit: Option<EditRole>,
}

/// The key a menu shows beside an action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    /// As the OS's menus take it: lower case, or a function key's character.
    pub key: String,
    pub command: bool,
    pub control: bool,
    pub option: bool,
    pub shift: bool,
}

/// What an action is to a native text field, which takes it instead of the
/// app while it has the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditRole {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemItem {
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Minimize,
    Zoom,
    BringAllToFront,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(id: u32, title: &str, enabled: bool) -> Entry {
        Entry::Action(Action {
            id,
            title: title.to_owned(),
            enabled,
            checked: false,
            key: None,
            edit: None,
        })
    }

    fn bar(entries: Vec<Entry>) -> Bar {
        Bar {
            menus: vec![Menu {
                title: "File".to_owned(),
                role: MenuRole::Plain,
                entries,
            }],
        }
    }

    #[test]
    fn only_titles_and_states_keep_the_shape() {
        let one = bar(vec![action(1, "Undo", true), Entry::Gap]);
        assert!(one.same_shape(&bar(vec![action(1, "다시", false), Entry::Gap])));
        assert!(!one.same_shape(&bar(vec![action(2, "Undo", true), Entry::Gap])));
        assert!(!one.same_shape(&bar(vec![action(1, "Undo", true)])));
        assert!(!one.same_shape(&bar(vec![Entry::Gap, action(1, "Undo", true)])));
    }
}
