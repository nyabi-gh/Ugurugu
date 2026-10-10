// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The panels in the areas beside the canvas and floating over it, as
//! 2.2.13's dock widgets. A panel's tab, or a group's handle, is dragged to
//! another group (a tab there), above or below one, to the end of an area,
//! or anywhere else to float. The handle's menu does the same from the
//! keyboard.

use egui::{
    Align2, Color32, CornerRadius, FontId, Frame, Id, Margin, Order, Pos2, Rect, Response, Sense,
    Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};

use std::collections::HashMap;

use super::Panels;
use crate::canvas::Canvas;
use crate::i18n::{args, tr, tr_with};
use crate::layout::{FLOATING_MIN, FLOATING_SIZE, Group, Layout, Panel, Place, Side, WIDTHS};
use crate::theme;

/// What docking keeps between frames.
#[derive(Default)]
pub struct Docks {
    drag: Option<Drag>,
    /// Where a dragged panel can go, found while drawing this frame.
    targets: Vec<Target>,
    /// A move chosen while drawing, made once everything is drawn.
    chosen: Option<(Vec<Panel>, Place)>,
    /// Changed when the layout is replaced, so that egui forgets the sizes
    /// it keeps for the areas and floating panels.
    generation: u32,
    /// How far down each group in an area reached last frame, by the panel
    /// in front, and the extra at the end of the right area.
    heights: HashMap<Option<Panel>, f32>,
}

impl Docks {
    pub fn replaced(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.drag = None;
        self.chosen = None;
    }
}

struct Drag {
    /// The one dragged first, then the rest of its group when the group's
    /// handle is dragged.
    panels: Vec<Panel>,
    /// The pointer from the group's top left.
    grab: Vec2,
    /// A floating group dragged by its handle follows the pointer.
    follows: bool,
}

struct Target {
    /// Where the pointer must be.
    hit: Rect,
    /// What lights up.
    show: Rect,
    place: Place,
    /// The group it belongs to; a group is not dropped on itself.
    group: Vec<Panel>,
    /// Shown faintly through the whole drag, as nothing else marks it.
    hint: bool,
}

impl Target {
    fn end(rect: Rect, side: Side) -> Self {
        Self {
            hit: rect,
            show: rect,
            place: Place::End(side),
            group: Vec::new(),
            hint: true,
        }
    }
}

/// The width of a folded area's strip, and of an empty side's drop zone.
const EDGE: f32 = 30.0;
/// The layers' group is not squeezed below this.
const LAYERS_LEAST: f32 = 280.0;

#[derive(Clone, Copy, PartialEq)]
enum Holder {
    Area { side: Side, first: bool },
    Floating,
}

fn title(panel: Panel) -> &'static str {
    tr(match panel {
        Panel::ToolSettings => "tool-settings",
        Panel::Color => "color-dock",
        Panel::ColorHistory => "color-history",
        Panel::Wobble => "wobble-dock",
        Panel::Layers => "layers",
    })
}

fn body(ui: &mut Ui, panel: Panel, canvas: &mut Canvas, panels: &mut Panels) {
    match panel {
        Panel::ToolSettings => super::tool_settings(ui, canvas, panels),
        Panel::Color => super::color(ui, canvas, panels),
        Panel::ColorHistory => super::color_history(ui, canvas),
        Panel::Wobble => super::wobble(ui, canvas, panels),
        Panel::Layers => super::layers(ui, canvas, panels),
    }
}

/// The left and right areas, with `extra` at the end of the right one.
/// Before the canvas, as they take room from it.
pub fn areas(
    ui: &mut Ui,
    canvas: &mut Canvas,
    panels: &mut Panels,
    mut extra: Option<&mut dyn FnMut(&mut Ui)>,
) {
    panels.docks.targets.clear();
    for side in [Side::Left, Side::Right] {
        let extra = if side == Side::Right {
            extra.take()
        } else {
            None
        };
        area(ui, side, canvas, panels, extra);
    }
}

fn area(
    ui: &mut Ui,
    side: Side,
    canvas: &mut Canvas,
    panels: &mut Panels,
    extra: Option<&mut dyn FnMut(&mut Ui)>,
) {
    let area = panels.layout.area(side).clone();
    let id = Id::new(("dock area", side == Side::Left, panels.docks.generation));
    let bar = |id: Id| match side {
        Side::Left => egui::Panel::left(id),
        Side::Right => egui::Panel::right(id),
    };
    if !panels.layout.has_open(side) && extra.is_none() {
        // While a panel is dragged, the empty side takes it at the canvas
        // edge, without taking room from the canvas.
        if panels.docks.drag.is_some() {
            let free = ui.available_rect_before_wrap();
            let strip = match side {
                Side::Left => free.with_max_x(free.left() + EDGE),
                Side::Right => free.with_min_x(free.right() - EDGE),
            };
            panels.docks.targets.push(Target::end(strip, side));
        }
        return;
    }
    if area.collapsed {
        // A strip that unfolds the area, and takes a dragged panel.
        let strip = bar(id.with("strip"))
            .resizable(false)
            .exact_size(EDGE)
            .frame(
                Frame::new()
                    .fill(theme::PANEL)
                    .inner_margin(Margin::symmetric(3, 8)),
            )
            .show(ui, |ui| {
                let arrow = if side == Side::Left { "›" } else { "‹" };
                if small_button(ui, arrow, tr("dock-expand-area")).clicked() {
                    panels.layout.area_mut(side).collapsed = false;
                }
            });
        panels
            .docks
            .targets
            .push(Target::end(strip.response.rect, side));
        return;
    }
    let shown = bar(id)
        .resizable(true)
        .default_size(area.width)
        .size_range(WIDTHS)
        .frame(
            Frame::new()
                .fill(theme::PANEL)
                .inner_margin(Margin::same(8)),
        )
        .show(ui, |ui| {
            let groups: Vec<(Group, Panel)> = area
                .groups
                .iter()
                .filter_map(|group| Some((group.clone(), panels.layout.front(group)?)))
                .collect();
            let total = ui.available_height();
            let below_extra = extra.is_some();
            for (index, (group, front)) in groups.iter().enumerate() {
                let top = ui.cursor().min.y;
                if index > 0 {
                    ui.separator();
                }
                let holder = Holder::Area {
                    side,
                    first: index == 0,
                };
                let origin = ui.cursor().min;
                let last = index + 1 == groups.len() && !below_extra;
                if *front == Panel::Layers && !last {
                    // The layer list takes the height left, so above other
                    // groups it gets what they took last frame.
                    let others: f32 = groups
                        .iter()
                        .map(|(_, other)| Some(*other))
                        .chain(below_extra.then_some(None))
                        .filter(|other| *other != Some(*front))
                        .filter_map(|other| panels.docks.heights.get(&other))
                        .sum();
                    let height = (total - others - (origin.y - top)).max(LAYERS_LEAST);
                    ui.allocate_ui(vec2(ui.available_width(), height), |ui| {
                        group_ui(ui, holder, origin, group, *front, canvas, panels);
                    });
                } else {
                    group_ui(ui, holder, origin, group, *front, canvas, panels);
                }
                reached(ui, &mut panels.docks.heights, Some(*front), top);
            }
            if let Some(extra) = extra {
                let top = ui.cursor().min.y;
                ui.separator();
                extra(ui);
                reached(ui, &mut panels.docks.heights, None, top);
            }
            let rest = ui.available_rect_before_wrap();
            if rest.height() > 8.0 {
                let mut end = Target::end(rest, side);
                end.hint = false;
                panels.docks.targets.push(end);
            }
        });
    // Only a width the user dragged to: a window too narrow for the area
    // squeezes it without changing what it goes back to.
    if ui.input(|input| input.pointer.primary_down()) {
        panels.layout.area_mut(side).width = shown.response.rect.width();
    }
}

/// Keeps how far down from `top` the group fronted by `front` reached,
/// drawing again if that moved.
fn reached(ui: &Ui, heights: &mut HashMap<Option<Panel>, f32>, front: Option<Panel>, top: f32) {
    let height = ui.cursor().min.y - top;
    let before = heights.insert(front, height);
    if before.is_none_or(|before| (before - height).abs() > 0.5) {
        ui.ctx().request_repaint();
    }
}

/// The floating panels, then what a drag or a menu chose. After the canvas.
pub fn floating(ui: &mut Ui, canvas: &mut Canvas, panels: &mut Panels) {
    let screen = ui.ctx().content_rect();
    if let Some(drag) = &panels.docks.drag
        && drag.follows
        && let Some(pointer) = ui.ctx().pointer_latest_pos()
    {
        let lead = drag.panels[0];
        let at = pointer - drag.grab;
        panels.layout.float_at(lead, at.into());
    }
    for index in 0..panels.layout.floating.len() {
        let floating = panels.layout.floating[index].clone();
        let Some(front) = panels.layout.front(&floating.group) else {
            continue;
        };
        // Kept where its handle can be reached, without moving it for good.
        let at = pos2(
            floating.at[0].clamp(screen.min.x, (screen.max.x - 80.0).max(screen.min.x)),
            floating.at[1].clamp(screen.min.y, (screen.max.y - 40.0).max(screen.min.y)),
        );
        let id = Id::new((
            "floating",
            floating.group.panels[0],
            panels.docks.generation,
        ));
        let shown = egui::Window::new(title(front))
            .id(id)
            .title_bar(false)
            .movable(false)
            .resizable(true)
            .constrain(false)
            .current_pos(at)
            .default_size(floating.size)
            .min_size(FLOATING_MIN)
            .frame(
                Frame::new()
                    .fill(theme::PANEL)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::same(8))
                    .shadow(ui.style().visuals.window_shadow),
            )
            .show(ui.ctx(), |ui| {
                group_ui(
                    ui,
                    Holder::Floating,
                    at,
                    &floating.group,
                    front,
                    canvas,
                    panels,
                );
            });
        let following =
            panels.docks.drag.as_ref().is_some_and(|drag| {
                drag.follows && floating.group.panels.contains(&drag.panels[0])
            });
        if let Some(shown) = shown
            && !following
            && ui.input(|input| input.pointer.primary_down())
            && let Some(each) = panels.layout.floating.get_mut(index)
        {
            each.size = shown.response.rect.size().into();
        }
    }
    finish(ui, panels);
}

/// One group: its header with the handle, tabs or title and buttons, then
/// the panel in front. A drag measures from `origin`: a floating group's
/// corner, so that it follows the pointer from where it was grabbed.
fn group_ui(
    ui: &mut Ui,
    holder: Holder,
    origin: Pos2,
    group: &Group,
    front: Panel,
    canvas: &mut Canvas,
    panels: &mut Panels,
) {
    let open: Vec<Panel> = group
        .panels
        .iter()
        .copied()
        .filter(|panel| panels.layout.is_open(*panel))
        .collect();
    // The handle moves the whole group, the one in front first.
    let mut whole = vec![front];
    whole.extend(group.panels.iter().copied().filter(|panel| *panel != front));
    let header = ui
        .horizontal(|ui| {
            let handle = handle(ui, front);
            if handle.drag_started() {
                start(
                    ui,
                    panels,
                    whole.clone(),
                    origin,
                    holder == Holder::Floating,
                );
            }
            egui::Popup::menu(&handle).show(|ui| move_menu(ui, panels, holder, &whole));
            if open.len() > 1 {
                for panel in &open {
                    let tab = tab(ui, *panel, *panel == front);
                    if tab.clicked() {
                        panels.layout.bring_to_front(*panel);
                    }
                    if tab.drag_started() {
                        start(ui, panels, vec![*panel], origin, false);
                    }
                }
            } else {
                ui.label(
                    egui::RichText::new(title(front))
                        .size(theme::SMALL)
                        .color(theme::MUTED),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if small_button(ui, "×", tr("dock-close")).clicked() {
                    panels.layout.set_open(front, false);
                }
                if let Holder::Area { side, first: true } = holder {
                    let arrow = if side == Side::Left { "‹" } else { "›" };
                    if small_button(ui, arrow, tr("dock-collapse-area")).clicked() {
                        panels.layout.area_mut(side).collapsed = true;
                    }
                }
            });
        })
        .response
        .rect;
    let content = ui.scope(|ui| body(ui, front, canvas, panels)).response.rect;
    let whole_rect = header.union(content);
    let targets = &mut panels.docks.targets;
    targets.push(Target {
        hit: header,
        show: whole_rect,
        place: Place::With(front),
        group: group.panels.clone(),
        hint: false,
    });
    let bar = |y: f32| Rect::from_x_y_ranges(whole_rect.x_range(), y - 2.0..=y + 2.0);
    let (upper, lower) = content.split_top_bottom_at_y(content.center().y);
    targets.push(Target {
        hit: upper,
        show: bar(whole_rect.top() - 3.0),
        place: Place::Before(front),
        group: group.panels.clone(),
        hint: false,
    });
    targets.push(Target {
        hit: lower,
        show: bar(whole_rect.bottom() + 3.0),
        place: Place::After(front),
        group: group.panels.clone(),
        hint: false,
    });
}

fn start(ui: &Ui, panels: &mut Panels, moved: Vec<Panel>, top: Pos2, follows: bool) {
    let pointer = ui
        .input(|input| input.pointer.press_origin())
        .unwrap_or(top);
    panels.docks.drag = Some(Drag {
        panels: moved,
        grab: pointer - top,
        follows,
    });
}

/// The handle's menu: the moves a drag makes, from the keyboard.
fn move_menu(ui: &mut Ui, panels: &mut Panels, holder: Holder, moved: &[Panel]) {
    let screen = ui.ctx().content_rect();
    let middle = screen.center() - Vec2::from(FLOATING_SIZE) / 2.0;
    let side = match holder {
        Holder::Area { side, .. } => Some(side),
        Holder::Floating => None,
    };
    for (key, place, here) in [
        (
            "dock-move-left",
            Place::End(Side::Left),
            side == Some(Side::Left),
        ),
        (
            "dock-move-right",
            Place::End(Side::Right),
            side == Some(Side::Right),
        ),
        ("dock-float", Place::Float(middle.into()), side.is_none()),
    ] {
        if !here && ui.button(tr(key)).clicked() {
            panels.docks.chosen = Some((moved.to_vec(), place));
        }
    }
}

/// Follows a drag, shows where it would go, and makes the move chosen.
fn finish(ui: &mut Ui, panels: &mut Panels) {
    let ctx = ui.ctx().clone();
    let Panels { layout, docks, .. } = panels;
    if let Some((moved, place)) = docks.chosen.take() {
        move_group(layout, &moved, place);
    }
    let Some(drag) = &docks.drag else {
        return;
    };
    let follows = drag.follows;
    let pointer = ctx.pointer_latest_pos();
    let target = pointer.and_then(|pointer| {
        docks.targets.iter().rev().find(|target| {
            target.hit.contains(pointer)
                && !target.group.iter().any(|panel| drag.panels.contains(panel))
        })
    });
    let at = pointer.map(|pointer| pointer - drag.grab);
    let painter = ctx.layer_painter(egui::LayerId::new(Order::Tooltip, Id::new("dock drop")));
    for hint in docks.targets.iter().filter(|target| target.hint) {
        painter.rect_filled(
            hint.show,
            CornerRadius::ZERO,
            theme::accent().gamma_multiply(0.08),
        );
    }
    if let Some(target) = target {
        painter.rect(
            target.show,
            CornerRadius::same(4),
            theme::accent().gamma_multiply(0.18),
            Stroke::new(1.5, theme::accent()),
            StrokeKind::Inside,
        );
    }
    if !follows && let Some(pointer) = pointer {
        let galley = painter.layout_no_wrap(
            title(drag.panels[0]).to_owned(),
            FontId::proportional(theme::SMALL),
            theme::TEXT,
        );
        let rect = Rect::from_min_size(pointer + vec2(14.0, 10.0), galley.size() + vec2(16.0, 8.0));
        painter.rect(
            rect,
            CornerRadius::same(5),
            theme::CONTROL,
            Stroke::new(1.0, theme::BORDER),
            StrokeKind::Inside,
        );
        painter.galley(rect.min + vec2(8.0, 4.0), galley, theme::TEXT);
    }
    if ctx.input(|input| input.pointer.primary_down()) {
        return;
    }
    let place = match (target, at) {
        (Some(target), _) => Some(target.place),
        (None, Some(at)) if !follows => Some(Place::Float(at.into())),
        _ => None,
    };
    if let (Some(drag), Some(place)) = (docks.drag.take(), place) {
        move_group(layout, &drag.panels, place);
    }
}

/// Moves `moved[0]` to `place` and the rest into its group.
fn move_group(layout: &mut Layout, moved: &[Panel], place: Place) {
    let Some((&lead, rest)) = moved.split_first() else {
        return;
    };
    layout.move_to(lead, place);
    for &panel in rest {
        layout.move_to(panel, Place::With(lead));
    }
    layout.bring_to_front(lead);
}

/// Four dots that drag the group, and open its menu when clicked.
fn handle(ui: &mut Ui, front: Panel) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(16.0, 18.0), Sense::click_and_drag());
    let name = tr_with("dock-handle", &args([("panel", title(front).to_owned())]));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
    let ink = if response.hovered() || response.dragged() {
        theme::TEXT
    } else {
        theme::MUTED
    };
    for dx in [-3.0, 3.0] {
        for dy in [-3.0, 3.0] {
            ui.painter()
                .circle_filled(rect.center() + vec2(dx, dy), 1.2, ink);
        }
    }
    focus_ring(ui, &response, rect);
    response
        .on_hover_cursor(egui::CursorIcon::Grab)
        .on_hover_text(name)
}

/// A panel's tab in a group of several.
fn tab(ui: &mut Ui, panel: Panel, front: bool) -> Response {
    let name = title(panel);
    let ink = if front { theme::TEXT } else { theme::MUTED };
    let galley =
        ui.painter()
            .layout_no_wrap(name.to_owned(), FontId::proportional(theme::SMALL), ink);
    let (rect, response) =
        ui.allocate_exact_size(galley.size() + vec2(14.0, 8.0), Sense::click_and_drag());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, front, name)
    });
    let fill = if front {
        theme::CONTROL
    } else if response.hovered() {
        theme::HOVER
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(5), fill);
    ui.painter().galley(
        Align2::CENTER_CENTER
            .anchor_size(rect.center(), galley.size())
            .min,
        galley,
        ink,
    );
    focus_ring(ui, &response, rect);
    response
}

/// A small text button named for screen readers apart from its text.
fn small_button(ui: &mut Ui, text: &str, name: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(5), theme::HOVER);
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(theme::BODY + 2.0),
        theme::MUTED,
    );
    focus_ring(ui, &response, rect);
    response.on_hover_text(name)
}

fn focus_ring(ui: &Ui, response: &Response, rect: Rect) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(5),
            Stroke::new(1.0, theme::accent()),
            StrokeKind::Inside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, PointerButton, RawInput};
    use ugu_core::document::Document;

    struct Window {
        ctx: egui::Context,
        canvas: Canvas,
        panels: Panels,
    }

    impl Window {
        fn new() -> Self {
            let mut window = Self {
                ctx: egui::Context::default(),
                canvas: Canvas::new(Document::new([320, 200]), |_| {}),
                panels: Panels::default(),
            };
            window.frame(Vec::new());
            window
        }

        fn frame(&mut self, events: Vec<Event>) {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 900.0))),
                events,
                ..RawInput::default()
            };
            let Self {
                ctx,
                canvas,
                panels,
            } = self;
            let mut output = ctx.run_ui(input, |ui| {
                areas(ui, canvas, panels, None);
                egui::CentralPanel::default()
                    .frame(Frame::NONE)
                    .show(ui, |_| {});
                floating(ui, canvas, panels);
            });
            // No GPU takes the font atlas here.
            output.textures_delta.clear();
        }

        fn button(pos: Pos2, pressed: bool) -> Event {
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }
        }

        fn drag(&mut self, from: Pos2, to: Pos2) {
            self.frame(vec![Event::PointerMoved(from)]);
            self.frame(vec![Self::button(from, true)]);
            for step in 1..=6 {
                let at = from + (to - from) * (step as f32 / 6.0);
                self.frame(vec![Event::PointerMoved(at)]);
            }
            self.frame(vec![Self::button(to, false)]);
            self.frame(Vec::new());
        }

        fn target(&self, place: Place) -> Rect {
            self.panels
                .docks
                .targets
                .iter()
                .find(|target| target.place == place)
                .unwrap_or_else(|| panic!("no target {place:?}"))
                .hit
        }

        fn handle(&self, front: Panel) -> Pos2 {
            let header = self.target(Place::With(front));
            pos2(header.left() + 8.0, header.center().y)
        }
    }

    #[test]
    fn dragging_handles_tabs_floats_and_docks_panels() {
        let mut window = Window::new();
        let onto = window.target(Place::With(Panel::ToolSettings)).center();
        window.drag(window.handle(Panel::Layers), onto);
        let layout = &window.panels.layout;
        assert_eq!(
            layout.left.groups[0].panels,
            [Panel::ToolSettings, Panel::Layers]
        );
        assert_eq!(layout.left.groups[0].front, Panel::Layers);
        // The layer list above them leaves the other groups room.
        window.frame(Vec::new());
        let history = window.target(Place::After(Panel::ColorHistory));
        assert!(history.bottom() < 900.0, "{history:?}");

        window.drag(window.handle(Panel::Wobble), pos2(800.0, 400.0));
        let layout = &window.panels.layout;
        assert!(layout.right.groups.is_empty());
        assert_eq!(layout.floating[0].group.panels, [Panel::Wobble]);
        let at = layout.floating[0].at;

        // Grabbed by its handle, a floating panel follows the pointer from
        // where it was grabbed.
        let from = window.handle(Panel::Wobble);
        window.drag(from, from + vec2(-100.0, 50.0));
        let moved = window.panels.layout.floating[0].at;
        assert!(
            (moved[0] - (at[0] - 100.0)).abs() < 1.0,
            "{moved:?} from {at:?}"
        );
        assert!(
            (moved[1] - (at[1] + 50.0)).abs() < 1.0,
            "{moved:?} from {at:?}"
        );

        // The empty right side still takes it, at the window's edge.
        window.drag(window.handle(Panel::Wobble), pos2(1590.0, 450.0));
        let layout = &window.panels.layout;
        assert!(layout.floating.is_empty());
        assert_eq!(layout.side_of(Panel::Wobble), Some(Side::Right));
    }

    #[test]
    fn a_tab_dragged_out_of_its_group_leaves_the_rest() {
        let mut window = Window::new();
        window
            .panels
            .layout
            .move_to(Panel::Color, Place::With(Panel::ToolSettings));
        window.frame(Vec::new());
        let header = window.target(Place::With(Panel::Color));
        // The tabs follow the handle: the first is Tool settings.
        let tab = pos2(header.left() + 40.0, header.center().y);
        let below = window.target(Place::After(Panel::Layers));
        window.drag(tab, pos2(below.center().x, below.bottom() - 5.0));
        let layout = &window.panels.layout;
        assert_eq!(layout.left.groups[0].panels, [Panel::Color]);
        assert_eq!(
            layout.right.groups.last().unwrap().panels,
            [Panel::ToolSettings]
        );
    }

    #[test]
    fn the_handle_menu_moves_from_the_keyboard_and_folding_hides_an_area() {
        let mut window = Window::new();
        let handle = window.handle(Panel::Wobble);
        window.frame(vec![Event::PointerMoved(handle)]);
        window.frame(vec![Window::button(handle, true)]);
        window.frame(vec![Window::button(handle, false)]);
        assert!(egui::Popup::is_any_open(&window.ctx), "the menu opens");
        window.panels.docks.chosen = Some((vec![Panel::Wobble], Place::End(Side::Left)));
        window.frame(Vec::new());
        assert_eq!(
            window.panels.layout.side_of(Panel::Wobble),
            Some(Side::Left)
        );

        window.panels.layout.left.collapsed = true;
        window.frame(Vec::new());
        assert!(
            window
                .panels
                .docks
                .targets
                .iter()
                .all(|target| target.group.is_empty()
                    || window.panels.layout.side_of(target.group[0]) != Some(Side::Left)),
            "a folded area shows no panels"
        );
    }
}
