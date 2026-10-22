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
use crate::icons::Glyph;
use crate::layout::{FLOATING_MIN, FLOATING_SIZE, Group, Layout, Panel, Place, Side, WIDTHS};
use crate::{theme, widgets};

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
    /// How tall each panel's group would be to show all of it, and the
    /// least it can take, from the last frame it was in front.
    naturals: HashMap<Panel, f32>,
    leasts: HashMap<Panel, f32>,
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
/// How tall a group's head is.
const HEAD: f32 = 24.0;
/// The layers' group is not squeezed below this in an area that scrolls.
const LAYERS_LEAST: f32 = 280.0;
/// The split between two groups of an area, which drags the height from
/// one to the other, and how far Up or Down moves it.
const SPLIT: f32 = 9.0;
const SPLIT_STEP: f32 = 16.0;
/// Tool settings when another group of its area takes what is left: as
/// tall whatever the tool shows, which then scrolls.
const TOOL_SETTINGS_HEIGHT: f32 = 300.0;
/// Under its head, a panel that scrolls shows at least this: Tool
/// settings its size, opacity, stabilization and antialiasing rows, the
/// others a row or two.
const TOOL_ROWS: f32 = 144.0;
const ROWS_LEAST: f32 = 60.0;
/// A panel's group before it was first measured.
const GUESS: f32 = 200.0;

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

/// The panel's content; `room` is the height it has, for the colour wheel.
fn body(ui: &mut Ui, panel: Panel, canvas: &mut Canvas, panels: &mut Panels, room: Option<f32>) {
    match panel {
        Panel::ToolSettings => super::tool_settings(ui, canvas, panels),
        Panel::Color => super::color(ui, canvas, panels, room),
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
                    .fill(theme::panel())
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
    // egui's id for the panel's resize handle, which Tab stops at.
    let handle = id.with("__resize");
    resize_from_keys(ui.ctx(), panels, side, id, handle);
    let shown = bar(id)
        .resizable(true)
        .default_size(area.width)
        .size_range(WIDTHS)
        .frame(
            Frame::new()
                .fill(theme::panel())
                .inner_margin(Margin::same(theme::space::M as i8)),
        )
        .show(ui, |ui| {
            // A window too low for every panel's least height scrolls the
            // whole area, so that none is out of reach.
            egui::ScrollArea::vertical()
                .id_salt("dock scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let groups: Vec<Placed> = area
                        .groups
                        .iter()
                        .enumerate()
                        .filter_map(|(index, group)| {
                            Some((index, group.clone(), panels.layout.front(group)?))
                        })
                        .collect();
                    let total = ui.available_height();
                    let below_extra = extra.is_some();
                    let probe = if below_extra {
                        panels.docks.heights.get(&None).copied().unwrap_or(0.0)
                    } else {
                        0.0
                    };
                    let spacing = ui.spacing().item_spacing.y;
                    match fit(panels, &groups, total - probe, spacing) {
                        Some(fit) => stack(ui, side, &groups, &fit, canvas, panels),
                        None => scroll(ui, side, &groups, total, below_extra, canvas, panels),
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
        });
    // Only a width the user dragged to: a window too narrow for the area
    // squeezes it without changing what it goes back to.
    if ui.ctx().is_being_dragged(handle) {
        panels.layout.area_mut(side).width = shown.response.rect.width();
    }
    if let Some(response) = ui.ctx().read_response(handle) {
        let width = shown.response.rect.width();
        response.widget_info(|| {
            egui::WidgetInfo::slider(
                true,
                f64::from(width.round()),
                tr(match side {
                    Side::Left => "dock-width-left",
                    Side::Right => "dock-width-right",
                }),
            )
        });
    }
}

/// Widens or narrows the area by a step for each Left or Right pressed while
/// its resize handle has the focus: from the keyboard, what dragging the
/// handle does.
fn resize_from_keys(ctx: &egui::Context, panels: &mut Panels, side: Side, id: Id, handle: Id) {
    const STEP: f32 = 16.0;
    if !ctx.memory(|memory| memory.has_focus(handle)) {
        return;
    }
    let mut growth = 0.0;
    ctx.input_mut(|input| {
        input.events.retain(|event| {
            let &egui::Event::Key {
                key,
                pressed,
                modifiers,
                ..
            } = event
            else {
                return true;
            };
            let outwards = match (key, side) {
                (egui::Key::ArrowRight, Side::Left) | (egui::Key::ArrowLeft, Side::Right) => 1.0,
                (egui::Key::ArrowLeft, Side::Left) | (egui::Key::ArrowRight, Side::Right) => -1.0,
                _ => return true,
            };
            if !modifiers.is_none() {
                return true;
            }
            if pressed {
                growth += outwards * STEP;
            }
            false
        });
    });
    if growth == 0.0 {
        return;
    }
    // The arrow moved the focus too before this frame's widgets.
    ctx.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
    let Some(mut state) = egui::containers::panel::PanelState::load(ctx, id) else {
        return;
    };
    let width = (state.outer_rect.width() + growth).clamp(*WIDTHS.start(), *WIDTHS.end());
    match side {
        Side::Left => state.outer_rect.max.x = state.outer_rect.min.x + width,
        Side::Right => state.outer_rect.min.x = state.outer_rect.max.x - width,
    }
    ctx.data_mut(|data| data.insert_persisted(id, state));
    panels.layout.area_mut(side).width = width;
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

/// Keeps a height measured for `panel`, drawing again if it changed.
fn remember(ui: &Ui, heights: &mut HashMap<Panel, f32>, panel: Panel, height: f32) {
    let before = heights.insert(panel, height);
    if before.is_none_or(|before| (before - height).abs() > 0.5) {
        ui.ctx().request_repaint();
    }
}

/// A group shown in an area: its index there, the group and the panel in
/// front.
type Placed = (usize, Group, Panel);

/// What a group asks of its area's height.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Ask {
    wanted: f32,
    least: f32,
    /// The least it may be dragged to.
    floor: f32,
}

/// The heights of an area's groups, and what they came from.
struct Fit {
    heights: Vec<f32>,
    asks: Vec<Ask>,
    /// The group that takes what the others leave.
    filler: Option<usize>,
    /// The height the groups share, without the splits between them.
    room: f32,
}

/// How tall `panel`'s group would be to show all of it.
fn natural_of(panels: &Panels, panel: Panel) -> f32 {
    match panel {
        // Not what the tool shows, so that changing tools moves nothing.
        Panel::ToolSettings => TOOL_SETTINGS_HEIGHT,
        _ => panels.docks.naturals.get(&panel).copied().unwrap_or(GUESS),
    }
}

/// The least `panel`'s group can take: a group of wobble's rows, which
/// are used alike, scrolls only once the artist made it shorter.
fn least_of(panels: &Panels, panel: Panel, set: bool) -> f32 {
    let floor = panels
        .docks
        .leasts
        .get(&panel)
        .copied()
        .unwrap_or(HEAD + ROWS_LEAST);
    match panel {
        Panel::Wobble if !set => natural_of(panels, panel).max(floor),
        _ => floor,
    }
}

/// The heights of `groups` in `total`, or None when even the least of
/// each does not fit: the layer list takes what the others leave, else
/// Tool settings, so that what is under it stays where it is.
fn fit(panels: &Panels, groups: &[Placed], total: f32, spacing: f32) -> Option<Fit> {
    let shown = |group: &Group| -> Vec<Panel> {
        group
            .panels
            .iter()
            .copied()
            .filter(|panel| panels.layout.is_open(*panel))
            .collect()
    };
    let holding = |panel: Panel| {
        groups
            .iter()
            .position(|(_, group, _)| shown(group).contains(&panel))
    };
    let filler = holding(Panel::Layers).or_else(|| holding(Panel::ToolSettings));
    let asks: Vec<Ask> = groups
        .iter()
        .enumerate()
        .map(|(at, (_, group, _))| {
            let panels_shown = shown(group);
            let most = |each: &dyn Fn(Panel) -> f32| {
                panels_shown
                    .iter()
                    .map(|panel| each(*panel))
                    .fold(0.0, f32::max)
            };
            let set = group.height.is_some();
            let least = most(&|panel| least_of(panels, panel, set));
            let floor = most(&|panel| least_of(panels, panel, true));
            let wanted = if Some(at) == filler {
                least
            } else {
                group
                    .height
                    .unwrap_or_else(|| most(&|panel| natural_of(panels, panel)))
            };
            Ask {
                wanted: wanted.max(least),
                least,
                floor,
            }
        })
        .collect();
    // Each group and each split is followed by the spacing.
    let count = groups.len() as f32;
    let room = total - count * spacing - (count - 1.0).max(0.0) * (SPLIT + spacing);
    let heights = share(&asks, filler, room)?;
    Some(Fit {
        heights,
        asks,
        filler,
        room,
    })
}

/// Gives each group what it wants out of `room`, the filler taking what is
/// left. When they want more, each gives up the same share of what it has
/// over its least. None when even the least of each does not fit.
fn share(asks: &[Ask], filler: Option<usize>, room: f32) -> Option<Vec<f32>> {
    let least: f32 = asks.iter().map(|ask| ask.least).sum();
    if least > room {
        return None;
    }
    let mut heights: Vec<f32> = asks.iter().map(|ask| ask.wanted).collect();
    let wanted: f32 = heights.iter().sum();
    if wanted <= room {
        if let Some(filler) = filler {
            heights[filler] += room - wanted;
        }
    } else {
        let over = wanted - room;
        let slack = wanted - least;
        for (height, ask) in heights.iter_mut().zip(asks) {
            *height -= (*height - ask.least) * over / slack;
        }
    }
    Some(heights)
}

/// The groups of an area at the heights `fit` gave them, each scrolling
/// what it cannot show, with a split between two that moves the height.
fn stack(
    ui: &mut Ui,
    side: Side,
    groups: &[Placed],
    fit: &Fit,
    canvas: &mut Canvas,
    panels: &mut Panels,
) {
    for (at, (_, group, front)) in groups.iter().enumerate() {
        if at > 0 {
            let (_, above, above_front) = &groups[at - 1];
            let change = split(ui, side, above, *above_front, fit.heights[at - 1]);
            if change != 0.0 {
                resize(panels, side, groups, fit, at - 1, change);
                ui.ctx().request_repaint();
            }
        }
        let holder = Holder::Area {
            side,
            first: at == 0,
        };
        let origin = ui.cursor().min;
        let height = fit.heights[at];
        ui.allocate_ui(vec2(ui.available_width(), height), |ui| {
            ui.set_height(height);
            group_ui(
                ui,
                holder,
                origin,
                group,
                *front,
                canvas,
                panels,
                Some(height),
            );
        });
    }
}

/// The groups of an area too low for them, as tall as what they show; the
/// layer list takes the height left, so above other groups it gets what
/// they took last frame.
fn scroll(
    ui: &mut Ui,
    side: Side,
    groups: &[Placed],
    total: f32,
    below_extra: bool,
    canvas: &mut Canvas,
    panels: &mut Panels,
) {
    for (index, (_, group, front)) in groups.iter().enumerate() {
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
            let others: f32 = groups
                .iter()
                .map(|(_, _, other)| Some(*other))
                .chain(below_extra.then_some(None))
                .filter(|other| *other != Some(*front))
                .filter_map(|other| panels.docks.heights.get(&other))
                .sum();
            let height = (total - others - (origin.y - top)).max(LAYERS_LEAST);
            ui.allocate_ui(vec2(ui.available_width(), height), |ui| {
                group_ui(ui, holder, origin, group, *front, canvas, panels, None);
            });
        } else {
            group_ui(ui, holder, origin, group, *front, canvas, panels, None);
        }
        reached(ui, &mut panels.docks.heights, Some(*front), top);
    }
}

/// The split under `above`: dragged, or moved by Up and Down while it has
/// the focus, it changes the height above it. Gives that change.
fn split(ui: &mut Ui, side: Side, above: &Group, front: Panel, height: f32) -> f32 {
    let id = Id::new(("dock split", side == Side::Left, above.panels[0]));
    let (_, rect) = ui.allocate_space(vec2(ui.available_width(), SPLIT));
    // An open list keeps the arrow keys: egui moves the focus to the
    // nearest widget it has seen in any layer, which a split under the list
    // would be. Only drawn, the split drops out of what egui has seen.
    if egui::Popup::is_any_open(ui.ctx()) {
        ui.painter().hline(
            rect.x_range(),
            rect.center().y,
            Stroke::new(1.0, theme::border()),
        );
        return 0.0;
    }
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let name = tr_with("dock-height", &args([("panel", title(front).to_owned())]));
    response.widget_info(|| egui::WidgetInfo::slider(true, f64::from(height.round()), &name));
    let active = response.hovered() || response.dragged() || response.has_focus();
    let ink = if active {
        theme::muted()
    } else {
        theme::border()
    };
    ui.painter()
        .hline(rect.x_range(), rect.center().y, Stroke::new(1.0, ink));
    focus_ring(ui, &response, rect);
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    // egui starts a drag some way from the press and leaves that way out of
    // the first delta, so the first frame takes all of it from the press.
    let mut change = if response.drag_started() {
        ui.input(|input| {
            input
                .pointer
                .interact_pos()
                .zip(input.pointer.press_origin())
                .map_or(0.0, |(at, origin)| at.y - origin.y)
        })
    } else {
        response.drag_delta().y
    };
    if response.has_focus() {
        change += split_keys(ui.ctx());
    }
    change
}

/// Takes Up and Down pressed for a focused split, as a change of height.
fn split_keys(ctx: &egui::Context) -> f32 {
    let mut change = 0.0;
    ctx.input_mut(|input| {
        input.events.retain(|event| {
            let &egui::Event::Key {
                key,
                pressed,
                modifiers,
                ..
            } = event
            else {
                return true;
            };
            let step = match key {
                egui::Key::ArrowUp => -SPLIT_STEP,
                egui::Key::ArrowDown => SPLIT_STEP,
                _ => return true,
            };
            if !modifiers.is_none() {
                return true;
            }
            if pressed {
                change += step;
            }
            false
        });
    });
    if change != 0.0 {
        // The arrow would move the focus too.
        ctx.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
    }
    change
}

/// Moves the split under group `above` by `change`. The group that keeps
/// the new height is the one above, or the one below when the one above
/// takes what is left.
fn resize(
    panels: &mut Panels,
    side: Side,
    groups: &[Placed],
    fit: &Fit,
    above: usize,
    change: f32,
) {
    let (at, change) = if fit.filler == Some(above) {
        (above + 1, -change)
    } else {
        (above, change)
    };
    let others: f32 = fit
        .asks
        .iter()
        .enumerate()
        .filter(|(each, _)| *each != at)
        .map(|(_, ask)| ask.least)
        .sum();
    let floor = fit.asks[at].floor;
    let height = (fit.heights[at] + change).clamp(floor, (fit.room - others).max(floor));
    panels.layout.area_mut(side).groups[groups[at].0].height = Some(height);
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
                    .fill(theme::panel())
                    .stroke(Stroke::new(1.0, theme::border()))
                    .corner_radius(CornerRadius::same(theme::radius::SURFACE))
                    .inner_margin(Margin::same(theme::space::M as i8))
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
                    None,
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
/// corner, so that it follows the pointer from where it was grabbed. Given
/// the `room` it has, the panel scrolls what it cannot show, except the
/// colour and the layers, which fit themselves to it.
#[expect(clippy::too_many_arguments)]
fn group_ui(
    ui: &mut Ui,
    holder: Holder,
    origin: Pos2,
    group: &Group,
    front: Panel,
    canvas: &mut Canvas,
    panels: &mut Panels,
    room: Option<f32>,
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
            // As tall with tabs as with a title alone.
            ui.set_min_height(HEAD);
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
                        .color(theme::muted()),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::icon_button(ui, Glyph::Cancel, 12.0, tr("dock-close"), true).clicked() {
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
    let head = header.height() + ui.spacing().item_spacing.y;
    let width = ui.available_width();
    let fits = matches!(front, Panel::Color | Panel::Layers);
    let content = match room {
        Some(room) if !fits => {
            let inner = (room - head).max(0.0);
            let shown = egui::ScrollArea::vertical()
                .id_salt(("dock panel", front))
                .max_height(inner)
                .min_scrolled_height(inner)
                .auto_shrink([false, false])
                .show(ui, |ui| body(ui, front, canvas, panels, None));
            remember(
                ui,
                &mut panels.docks.naturals,
                front,
                head + shown.content_size.y,
            );
            shown.inner_rect
        }
        Some(room) => {
            let inner = (room - head).max(0.0);
            ui.allocate_ui(vec2(width, inner), |ui| {
                ui.set_max_height(inner);
                body(ui, front, canvas, panels, Some(inner));
            })
            .response
            .rect
        }
        None => {
            let rect = ui
                .scope(|ui| body(ui, front, canvas, panels, None))
                .response
                .rect;
            if !fits {
                remember(ui, &mut panels.docks.naturals, front, head + rect.height());
            }
            rect
        }
    };
    let least = match front {
        Panel::ToolSettings => TOOL_ROWS,
        Panel::Color => {
            let natural = head + panels.color.natural_height(width);
            remember(ui, &mut panels.docks.naturals, front, natural);
            panels.color.least_height(width)
        }
        Panel::Layers => panels.layers.least_height(),
        Panel::ColorHistory | Panel::Wobble => ROWS_LEAST,
    };
    remember(ui, &mut panels.docks.leasts, front, head + least);
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
    let moves = [
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
    ];
    let width = moves
        .iter()
        .map(|(key, ..)| widgets::menu_item_width(ui, tr(key), ""))
        .fold(widgets::MENU_WIDTH, f32::max);
    for (key, place, here) in moves {
        if !here && widgets::menu_item(ui, width, tr(key), "", None, true).clicked() {
            panels.docks.chosen = Some((moved.to_vec(), place));
            // Chosen with Space or Enter too, which egui does not close on.
            ui.close();
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
            CornerRadius::same(theme::radius::SMALL),
            theme::accent().gamma_multiply(0.18),
            Stroke::new(1.5, theme::accent_ink()),
            StrokeKind::Inside,
        );
    }
    if !follows && let Some(pointer) = pointer {
        let galley = painter.layout_no_wrap(
            title(drag.panels[0]).to_owned(),
            FontId::proportional(theme::SMALL),
            theme::text(),
        );
        let rect = Rect::from_min_size(pointer + vec2(14.0, 10.0), galley.size() + vec2(16.0, 8.0));
        painter.rect(
            rect,
            CornerRadius::same(theme::radius::CONTROL),
            theme::control(),
            Stroke::new(1.0, theme::border()),
            StrokeKind::Inside,
        );
        painter.galley(rect.min + vec2(8.0, 4.0), galley, theme::text());
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
        theme::text()
    } else {
        theme::muted()
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
    let ink = if front { theme::text() } else { theme::muted() };
    let galley =
        ui.painter()
            .layout_no_wrap(name.to_owned(), FontId::proportional(theme::SMALL), ink);
    let (rect, response) = ui.allocate_exact_size(
        vec2(galley.size().x + 14.0, HEAD - 2.0),
        Sense::click_and_drag(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, front, name)
    });
    let fill = if front {
        theme::control()
    } else if response.hovered() {
        theme::hover()
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(theme::radius::CONTROL), fill);
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
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(theme::radius::CONTROL),
            theme::hover(),
        );
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(theme::HEADING),
        theme::muted(),
    );
    focus_ring(ui, &response, rect);
    response.on_hover_text(name)
}

fn focus_ring(ui: &Ui, response: &Response, rect: Rect) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(theme::radius::CONTROL),
            Stroke::new(1.0, theme::accent_ink()),
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
        size: Vec2,
    }

    impl Window {
        fn new() -> Self {
            let mut window = Self {
                ctx: egui::Context::default(),
                canvas: Canvas::new(Document::new([320, 200]), |_| {}),
                panels: Panels::default(),
                size: vec2(1600.0, 900.0),
            };
            window.settle();
            window
        }

        /// Frames until the panels have measured themselves and nothing
        /// asks for another.
        fn settle(&mut self) {
            for _ in 0..8 {
                self.frame(Vec::new());
                if !self.ctx.has_requested_repaint() {
                    break;
                }
            }
        }

        fn frame(&mut self, events: Vec<Event>) {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
                events,
                ..RawInput::default()
            };
            let Self {
                ctx,
                canvas,
                panels,
                ..
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
        let colors = window.target(Place::After(Panel::Color));
        assert!(colors.bottom() < 900.0, "{colors:?}");

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
    fn pressing_in_a_panel_squeezed_by_a_narrow_window_keeps_its_width() {
        let mut window = Window::new();
        let widths = |window: &Window| {
            let layout = &window.panels.layout;
            [layout.left.width, layout.right.width]
        };
        let before = widths(&window);
        window.size = vec2(500.0, 900.0);
        window.frame(Vec::new());
        let inside = window.target(Place::With(Panel::Wobble)).center();
        window.drag(inside, inside + vec2(0.0, 1.0));
        assert_eq!(widths(&window), before);

        // Dragging the edge still sets it.
        window.size = vec2(1600.0, 900.0);
        window.frame(Vec::new());
        let edge = window.target(Place::With(Panel::Wobble)).left() - theme::space::M;
        window.drag(pos2(edge, 450.0), pos2(edge - 40.0, 450.0));
        let squeezed = 1600.0 - edge;
        assert!(
            widths(&window)[1] > squeezed + 20.0,
            "{:?}",
            widths(&window)
        );
    }

    #[test]
    fn a_tab_dragged_out_of_its_group_leaves_the_rest() {
        let mut window = Window::new();
        window
            .panels
            .layout
            .move_to(Panel::Color, Place::With(Panel::ToolSettings));
        window.settle();
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
    fn the_filler_takes_what_is_left_and_the_others_give_up_alike() {
        let ask = |wanted, least| Ask {
            wanted,
            least,
            floor: least,
        };
        let asks = [ask(100.0, 100.0), ask(300.0, 200.0)];
        assert_eq!(share(&asks, Some(0), 500.0), Some(vec![200.0, 300.0]));
        assert_eq!(share(&asks, None, 500.0), Some(vec![100.0, 300.0]));
        // Short of room, each gives up the same share of what it has over
        // its least.
        let asks = [ask(150.0, 100.0), ask(300.0, 200.0)];
        assert_eq!(share(&asks, None, 375.0), Some(vec![125.0, 250.0]));
        assert_eq!(share(&asks, None, 299.0), None);
    }

    #[test]
    fn the_split_between_two_groups_moves_with_a_drag_and_the_arrow_keys() {
        let mut window = Window::new();
        let colors = |window: &Window| window.panels.layout.left.groups[1].height;
        assert_eq!(colors(&window), None);
        let header = window.target(Place::With(Panel::Color));
        let spacing = window.ctx.global_style().spacing.item_spacing.y;
        let split = pos2(header.center().x, header.top() - spacing - SPLIT / 2.0);
        // Tool settings above takes what is left, so the colour keeps the
        // height the drag leaves it. In steps of 2 points, as a mouse moves,
        // so egui sees a drag only some way from the press.
        window.frame(vec![Event::PointerMoved(split)]);
        window.frame(vec![Window::button(split, true)]);
        for step in 1..=30 {
            window.frame(vec![Event::PointerMoved(
                split + vec2(0.0, 2.0 * step as f32),
            )]);
        }
        window.frame(vec![Window::button(split + vec2(0.0, 60.0), false)]);
        window.settle();
        let dragged = colors(&window).expect("a height for the colour group");
        let moved = window.target(Place::With(Panel::Color));
        assert!(
            (moved.top() - (header.top() + 60.0)).abs() < 0.5,
            "{header:?} went to {moved:?}, not with the pointer"
        );

        let id = Id::new(("dock split", true, Panel::ToolSettings));
        window.ctx.memory_mut(|memory| memory.request_focus(id));
        window.frame(Vec::new());
        let down = |pressed| Event::Key {
            key: egui::Key::ArrowDown,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        window.frame(vec![down(true), down(false)]);
        window.settle();
        assert_eq!(colors(&window), Some(dragged - SPLIT_STEP));
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
