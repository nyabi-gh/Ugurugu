// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Where the panels are: in the left or right area, stacked or tabbed with
//! others, or floating over the canvas, as 2.2.13's dock widgets; and how it
//! is kept in the settings file.

use serde_json::{Map, Value, json};

/// The panels that can be moved, in the Window menu's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Panel {
    ToolSettings,
    Color,
    ColorHistory,
    Wobble,
    Layers,
}

impl Panel {
    pub const ALL: [Self; 5] = [
        Self::ToolSettings,
        Self::Color,
        Self::ColorHistory,
        Self::Wobble,
        Self::Layers,
    ];

    fn key(self) -> &'static str {
        match self {
            Self::ToolSettings => "toolSettings",
            Self::Color => "color",
            Self::ColorHistory => "colorHistory",
            Self::Wobble => "wobble",
            Self::Layers => "layers",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|panel| panel.key() == key)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// Panels sharing one place, shown one at a time behind tabs.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub panels: Vec<Panel>,
    /// The panel in front.
    pub front: Panel,
}

impl Group {
    fn of(panel: Panel) -> Self {
        Self {
            panels: vec![panel],
            front: panel,
        }
    }
}

/// A side of the canvas: groups stacked from the top.
#[derive(Clone, Debug, PartialEq)]
pub struct Area {
    pub groups: Vec<Group>,
    /// In points.
    pub width: f32,
    /// Folded to a narrow strip with a button that unfolds it.
    pub collapsed: bool,
}

/// A group over the canvas.
#[derive(Clone, Debug, PartialEq)]
pub struct Floating {
    pub group: Group,
    /// Top left, in points from the window's.
    pub at: [f32; 2],
    pub size: [f32; 2],
}

/// Where a panel is moved to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Place {
    /// Into the group holding this panel, as a tab.
    With(Panel),
    /// A group of its own above the one holding this panel.
    Before(Panel),
    /// A group of its own below the one holding this panel.
    After(Panel),
    /// A group of its own at the bottom of a side.
    End(Side),
    /// Floating with its top left here.
    Float([f32; 2]),
}

pub const WIDTHS: std::ops::RangeInclusive<f32> = 150.0..=460.0;
pub const FLOATING_SIZE: [f32; 2] = [280.0, 420.0];
/// Floating panels are kept at least this size, so they can be found again.
pub const FLOATING_MIN: [f32; 2] = [150.0, 80.0];

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub left: Area,
    pub right: Area,
    pub floating: Vec<Floating>,
    /// Panels closed from the Window menu or their close button. They keep
    /// their place and come back to it.
    pub closed: Vec<Panel>,
    pub animation_bar: bool,
}

impl Default for Layout {
    /// 3.0's layout since M3-9: tools and colour on the left, wobble and
    /// layers on the right, each panel in its own group.
    fn default() -> Self {
        let area = |panels: &[Panel], width| Area {
            groups: panels.iter().copied().map(Group::of).collect(),
            width,
            collapsed: false,
        };
        Self {
            left: area(
                &[Panel::ToolSettings, Panel::Color, Panel::ColorHistory],
                260.0,
            ),
            right: area(&[Panel::Wobble, Panel::Layers], 300.0),
            floating: Vec::new(),
            closed: Vec::new(),
            animation_bar: true,
        }
    }
}

impl Layout {
    pub fn area(&self, side: Side) -> &Area {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    pub fn area_mut(&mut self, side: Side) -> &mut Area {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }

    pub fn is_open(&self, panel: Panel) -> bool {
        !self.closed.contains(&panel)
    }

    /// Opens or closes `panel`. An opened panel comes to the front of its
    /// group, and its side unfolds.
    pub fn set_open(&mut self, panel: Panel, open: bool) {
        self.closed.retain(|each| *each != panel);
        if open {
            self.bring_to_front(panel);
            if let Some(side) = self.side_of(panel) {
                self.area_mut(side).collapsed = false;
            }
        } else {
            self.closed.push(panel);
        }
    }

    pub fn side_of(&self, panel: Panel) -> Option<Side> {
        [Side::Left, Side::Right].into_iter().find(|side| {
            self.area(*side)
                .groups
                .iter()
                .any(|group| group.panels.contains(&panel))
        })
    }

    fn group_mut(&mut self, panel: Panel) -> Option<&mut Group> {
        self.left
            .groups
            .iter_mut()
            .chain(self.right.groups.iter_mut())
            .chain(self.floating.iter_mut().map(|floating| &mut floating.group))
            .find(|group| group.panels.contains(&panel))
    }

    pub fn bring_to_front(&mut self, panel: Panel) {
        if let Some(group) = self.group_mut(panel) {
            group.front = panel;
        }
    }

    /// Of `group`'s open panels, the one in front: its own front if open,
    /// else the first open one.
    pub fn front(&self, group: &Group) -> Option<Panel> {
        if self.is_open(group.front) {
            return Some(group.front);
        }
        group
            .panels
            .iter()
            .copied()
            .find(|panel| self.is_open(*panel))
    }

    /// Whether `side` shows anything.
    pub fn has_open(&self, side: Side) -> bool {
        self.area(side)
            .groups
            .iter()
            .any(|group| self.front(group).is_some())
    }

    /// Moves `panel` to `place`. Moving a panel next to or into its own
    /// group alone changes nothing.
    pub fn move_to(&mut self, panel: Panel, place: Place) {
        let target = match place {
            Place::With(other) | Place::Before(other) | Place::After(other) => Some(other),
            _ => None,
        };
        if target == Some(panel) {
            return;
        }
        // A floating group moved as a whole keeps its size.
        let mut size = FLOATING_SIZE;
        self.take(panel, &mut size);
        match place {
            Place::With(other) => {
                if let Some(group) = self.group_mut(other) {
                    group.panels.push(panel);
                    group.front = panel;
                }
            }
            Place::Before(other) | Place::After(other) => {
                let after = matches!(place, Place::After(_));
                let found = [Side::Left, Side::Right].into_iter().find_map(|side| {
                    let index = self
                        .area(side)
                        .groups
                        .iter()
                        .position(|group| group.panels.contains(&other))?;
                    Some((side, index))
                });
                match found {
                    Some((side, index)) => self
                        .area_mut(side)
                        .groups
                        .insert(index + usize::from(after), Group::of(panel)),
                    // Next to a floating panel is floating with it.
                    None => {
                        if let Some(group) = self.group_mut(other) {
                            group.panels.push(panel);
                            group.front = panel;
                        }
                    }
                }
            }
            Place::End(side) => {
                let area = self.area_mut(side);
                area.groups.push(Group::of(panel));
                area.collapsed = false;
            }
            Place::Float(at) => self.floating.push(Floating {
                group: Group::of(panel),
                at,
                size,
            }),
        }
    }

    /// Moves a floating group's top left.
    pub fn float_at(&mut self, panel: Panel, at: [f32; 2]) {
        if let Some(floating) = self
            .floating
            .iter_mut()
            .find(|floating| floating.group.panels.contains(&panel))
        {
            floating.at = at;
        }
    }

    /// Takes `panel` out of its group, dropping a group left empty. `size`
    /// becomes the floating group's if it was alone in one.
    fn take(&mut self, panel: Panel, size: &mut [f32; 2]) {
        for area in [&mut self.left, &mut self.right] {
            for group in &mut area.groups {
                remove(group, panel);
            }
            area.groups.retain(|group| !group.panels.is_empty());
        }
        for floating in &mut self.floating {
            if floating.group.panels == [panel] {
                *size = floating.size;
            }
            remove(&mut floating.group, panel);
        }
        self.floating
            .retain(|floating| !floating.group.panels.is_empty());
    }

    /// Reads a layout written by `to_json`. One that does not hold each
    /// panel exactly once is broken, and the default is used.
    pub fn parse(value: &Value) -> Self {
        match read(value) {
            Some(layout) => layout,
            None => {
                tracing::warn!(%value, "the panel layout in the settings is broken; using the default");
                Self::default()
            }
        }
    }

    pub fn to_json(&self) -> Value {
        let area = |area: &Area| {
            json!({
                "width": area.width,
                "collapsed": area.collapsed,
                "groups": area.groups.iter().map(group_json).collect::<Vec<_>>(),
            })
        };
        let floating: Vec<Value> = self
            .floating
            .iter()
            .map(|floating| {
                let mut object = group_json(&floating.group);
                object["at"] = json!(floating.at);
                object["size"] = json!(floating.size);
                object
            })
            .collect();
        json!({
            "left": area(&self.left),
            "right": area(&self.right),
            "floating": floating,
            "closed": self.closed.iter().map(|panel| panel.key()).collect::<Vec<_>>(),
            "animationBar": self.animation_bar,
        })
    }
}

fn remove(group: &mut Group, panel: Panel) {
    group.panels.retain(|each| *each != panel);
    if group.front == panel
        && let Some(first) = group.panels.first()
    {
        group.front = *first;
    }
}

fn group_json(group: &Group) -> Value {
    json!({
        "panels": group.panels.iter().map(|panel| panel.key()).collect::<Vec<_>>(),
        "front": group.front.key(),
    })
}

fn read(value: &Value) -> Option<Layout> {
    let object = value.as_object()?;
    let mut seen = Vec::new();
    let mut group = |value: &Value| -> Option<Group> {
        let object = value.as_object()?;
        let panels = object
            .get("panels")?
            .as_array()?
            .iter()
            .map(|key| Panel::from_key(key.as_str()?))
            .collect::<Option<Vec<_>>>()?;
        if panels.is_empty() || panels.iter().any(|panel| seen.contains(panel)) {
            return None;
        }
        seen.extend(&panels);
        let front = object
            .get("front")
            .and_then(Value::as_str)
            .and_then(Panel::from_key)
            .filter(|front| panels.contains(front))
            .unwrap_or(panels[0]);
        Some(Group { panels, front })
    };
    let mut area = |key: &str| -> Option<Area> {
        let object = object.get(key)?.as_object()?;
        let groups = object
            .get("groups")?
            .as_array()?
            .iter()
            .map(&mut group)
            .collect::<Option<Vec<_>>>()?;
        Some(Area {
            groups,
            width: number(object, "width")?.clamp(*WIDTHS.start(), *WIDTHS.end()),
            collapsed: object.get("collapsed")?.as_bool()?,
        })
    };
    let left = area("left")?;
    let right = area("right")?;
    let floating = object
        .get("floating")?
        .as_array()?
        .iter()
        .map(|value| {
            let fields = value.as_object()?;
            let pair = |key| -> Option<[f32; 2]> {
                match fields.get(key)?.as_array()?.as_slice() {
                    [x, y] => Some([finite(x)?, finite(y)?]),
                    _ => None,
                }
            };
            let [width, height] = pair("size")?;
            Some(Floating {
                group: group(value)?,
                at: pair("at")?,
                size: [width.max(FLOATING_MIN[0]), height.max(FLOATING_MIN[1])],
            })
        })
        .collect::<Option<Vec<_>>>()?;
    if seen.len() != Panel::ALL.len() {
        return None;
    }
    let closed = object
        .get("closed")?
        .as_array()?
        .iter()
        .map(|key| Panel::from_key(key.as_str()?))
        .collect::<Option<Vec<_>>>()?;
    Some(Layout {
        left,
        right,
        floating,
        closed,
        animation_bar: object.get("animationBar")?.as_bool()?,
    })
}

fn finite(value: &Value) -> Option<f32> {
    Some(value.as_f64()? as f32).filter(|value| value.is_finite())
}

fn number(object: &Map<String, Value>, key: &str) -> Option<f32> {
    finite(object.get(key)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_panel_once(layout: &Layout) -> bool {
        let mut all: Vec<Panel> = [&layout.left, &layout.right]
            .into_iter()
            .flat_map(|area| area.groups.iter())
            .chain(layout.floating.iter().map(|floating| &floating.group))
            .flat_map(|group| group.panels.iter().copied())
            .collect();
        all.sort_by_key(|panel| *panel as u8);
        all == Panel::ALL
    }

    #[test]
    fn a_layout_comes_back_from_the_settings_as_it_was() {
        let mut layout = Layout::default();
        layout.move_to(Panel::Wobble, Place::With(Panel::ToolSettings));
        layout.move_to(Panel::ColorHistory, Place::Float([40.0, 60.5]));
        layout.set_open(Panel::Color, false);
        layout.left.width = 222.0;
        layout.right.collapsed = true;
        layout.animation_bar = false;
        let text = layout.to_json().to_string();
        let read = Layout::parse(&serde_json::from_str(&text).unwrap());
        assert_eq!(read, layout);
        assert!(every_panel_once(&read));
    }

    #[test]
    fn a_broken_layout_is_the_default() {
        let good = Layout::default().to_json();
        let mut twice = good.clone();
        twice["right"]["groups"][0]["panels"] = json!(["wobble", "layers"]);
        let mut missing = good.clone();
        missing["right"]["groups"] = json!([{"panels": ["wobble"], "front": "wobble"}]);
        let mut unknown = good.clone();
        unknown["left"]["groups"][0]["panels"] = json!(["brushes"]);
        let mut empty = good.clone();
        empty["floating"] = json!([{"panels": [], "at": [0, 0], "size": [1, 1]}]);
        for broken in [twice, missing, unknown, empty, json!("left"), json!({})] {
            assert_eq!(Layout::parse(&broken), Layout::default(), "{broken}");
        }
    }

    #[test]
    fn values_out_of_range_are_brought_into_it() {
        let mut value = Layout::default().to_json();
        value["left"]["width"] = json!(9000);
        value["floating"] = json!([]);
        let mut layout = Layout::default();
        layout.move_to(Panel::Layers, Place::Float([0.0, 0.0]));
        let mut floating = layout.to_json();
        floating["floating"][0]["size"] = json!([1, -5]);
        assert_eq!(Layout::parse(&value).left.width, *WIDTHS.end());
        assert_eq!(Layout::parse(&floating).floating[0].size, FLOATING_MIN);
    }

    #[test]
    fn panels_move_between_sides_tabs_and_floating() {
        let mut layout = Layout::default();
        layout.move_to(Panel::Layers, Place::Before(Panel::ToolSettings));
        assert_eq!(layout.left.groups[0].panels, [Panel::Layers]);
        assert_eq!(layout.right.groups.len(), 1);
        layout.move_to(Panel::Wobble, Place::With(Panel::Layers));
        assert!(layout.right.groups.is_empty());
        assert_eq!(layout.left.groups[0].panels, [Panel::Layers, Panel::Wobble]);
        assert_eq!(layout.left.groups[0].front, Panel::Wobble);
        layout.move_to(Panel::Wobble, Place::Float([10.0, 20.0]));
        assert_eq!(layout.left.groups[0].front, Panel::Layers);
        assert_eq!(layout.floating[0].at, [10.0, 20.0]);
        layout.move_to(Panel::Color, Place::After(Panel::Wobble));
        assert_eq!(
            layout.floating[0].group.panels,
            [Panel::Wobble, Panel::Color]
        );
        layout.move_to(Panel::Wobble, Place::End(Side::Right));
        layout.move_to(Panel::Color, Place::End(Side::Right));
        assert!(layout.floating.is_empty());
        assert_eq!(layout.side_of(Panel::Color), Some(Side::Right));
        // Next to or into itself changes nothing.
        let before = layout.clone();
        layout.move_to(Panel::Color, Place::With(Panel::Color));
        layout.move_to(Panel::Color, Place::After(Panel::Color));
        assert_eq!(layout, before);
        assert!(every_panel_once(&layout));
    }

    #[test]
    fn a_floating_panel_moved_again_keeps_its_size() {
        let mut layout = Layout::default();
        layout.move_to(Panel::Layers, Place::Float([0.0, 0.0]));
        layout.floating[0].size = [333.0, 444.0];
        layout.move_to(Panel::Layers, Place::Float([5.0, 5.0]));
        assert_eq!(layout.floating[0].size, [333.0, 444.0]);
    }

    #[test]
    fn closed_panels_keep_their_place_and_open_in_front() {
        let mut layout = Layout::default();
        layout.move_to(Panel::Wobble, Place::With(Panel::Layers));
        layout.set_open(Panel::Wobble, false);
        let group = layout.right.groups[0].clone();
        assert_eq!(layout.front(&group), Some(Panel::Layers));
        layout.set_open(Panel::Layers, false);
        assert!(!layout.has_open(Side::Right));
        layout.right.collapsed = true;
        layout.set_open(Panel::Wobble, true);
        assert_eq!(layout.right.groups[0].front, Panel::Wobble);
        assert!(!layout.right.collapsed);
        assert!(layout.is_open(Panel::Wobble));
    }
}
