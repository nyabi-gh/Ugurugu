// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The window around the canvas, laid out as 2.2.13: menus and quick access
//! on top, the tool rail and tool settings on the left, wobble, colour and
//! layers on the right, the animation bar under the canvas and the status
//! bar at the bottom. Only what 3.0 can already do is shown.

mod actions;
mod brushes;
mod color;
mod history;
mod layers;
mod resize;
mod restyle;
mod settings;
mod text;

use fluent_bundle::FluentArgs;
use std::sync::Arc;
use ugu_core::document::{LayerKind, limits};
use ugu_core::edit::{EditError, Outcome};
use ugu_core::motion::frame_in_cycle;

use ugu_core::ops::{Affine, MotionStyle, Sampling, Wobble};
use ugu_core::selection::{Combine, Selection};
use ugu_session::{FillSettings, Lasso, Reads, Session, ShapeKind, Tool};

use crate::canvas::{Canvas, Grip, HANDLES, ZOOM_RANGE};
use crate::clipboard::Clipboard;
use crate::files::Files;
use crate::i18n::{tr, tr_with};
use crate::icons::{self, Glyph};
use crate::settings::Store;
use crate::shortcuts::{Action, Keymap};
use crate::theme;
use crate::widgets;

/// What the panels keep between frames: which are open, and edits in
/// progress that are committed as one undo step when they end.
#[derive(Default)]
pub struct Panels {
    pub layers: layers::LayerDock,
    color: color::ColorDock,
    presets: brushes::Presets,
    pub shown: Shown,
    /// Frames and frames per second being edited.
    animation: Option<(f32, f32)>,
    /// Wobble being edited, from a drag or typing.
    wobble: Option<Wobble>,
    /// Whether the wobble panel edits the current layer instead of the
    /// whole drawing.
    wobble_layer: bool,
    /// The document point under the pointer, for the status bar.
    pub pointer: Option<[f64; 2]>,
    /// Why the last edit from the panels was refused.
    refusal: Option<String>,
    /// The canvas or image size dialog, while open.
    resize: Option<resize::Dialog>,
    /// The stroke properties dialog, while open.
    restyle: Option<restyle::Dialog>,
    /// The settings dialog, while open.
    settings: Option<settings::Dialog>,
    /// Which action each key runs.
    pub keys: Keymap,
}

/// Panels the Window menu opens and closes.
#[derive(Clone, Copy)]
pub struct Shown {
    pub tool_settings: bool,
    pub wobble: bool,
    pub color: bool,
    pub color_history: bool,
    pub layers: bool,
    pub animation_bar: bool,
}

impl Default for Shown {
    fn default() -> Self {
        Self {
            tool_settings: true,
            wobble: true,
            color: true,
            color_history: true,
            layers: true,
            animation_bar: true,
        }
    }
}

fn args<const N: usize>(pairs: [(&'static str, String); N]) -> FluentArgs<'static> {
    let mut args = FluentArgs::new();
    for (name, value) in pairs {
        args.set(name, value);
    }
    args
}

impl Panels {
    /// Reports a refused edit until the next edit that goes through.
    fn report(&mut self, result: Result<Outcome, EditError>) {
        match result {
            Ok(Outcome::Committed(_)) => self.refusal = None,
            Ok(Outcome::NoChange) => {}
            Err(error) => {
                tracing::warn!(%error, "edit refused");
                self.refusal = Some(error.to_string());
            }
        }
    }
}

pub use actions::shortcuts;

fn report_bool(result: Result<bool, EditError>) {
    if let Err(error) = result {
        tracing::warn!(%error, "undo or redo failed");
    }
}

pub fn menu_bar(
    ui: &mut egui::Ui,
    canvas: &mut Canvas,
    files: &mut Files,
    clipboard: &mut Clipboard,
    panels: &mut Panels,
) {
    let mut chosen = None;
    egui::MenuBar::new().ui(ui, |ui| {
        for (title, entries) in actions::MENUS {
            ui.menu_button(tr(title), |ui| {
                chosen = chosen.or(actions::items(ui, entries, canvas, panels));
            });
        }
    });
    if let Some(action) = chosen {
        actions::run(action, canvas, files, clipboard, panels);
    }
}

/// The canvas or image size, stroke properties or settings dialog, while one
/// is open.
/// `typed`: chords egui turns into clipboard events, for a shortcut being
/// set in the settings.
pub fn dialogs(
    ctx: &egui::Context,
    canvas: &mut Canvas,
    files: &mut Files,
    store: &mut Store,
    panels: &mut Panels,
    typed: &[crate::shortcuts::Chord],
) {
    resize::show(ctx, canvas, panels);
    restyle::show(ctx, canvas, panels);
    settings::show(ctx, canvas, files, store, panels, typed);
}

/// Puts the settings read at start into effect.
pub fn apply_settings(
    ctx: &egui::Context,
    store: &Store,
    canvas: &mut Canvas,
    files: &mut Files,
    panels: &mut Panels,
) {
    settings::restore_tools(store.get(), canvas);
    settings::apply(ctx, store.get(), canvas, files, &mut panels.keys);
}

/// The quick access bar: panels on the left, undo and redo on the right.
pub fn quick_access(
    ui: &mut egui::Ui,
    canvas: &mut Canvas,
    files: &mut Files,
    clipboard: &mut Clipboard,
    panels: &mut Panels,
) {
    ui.horizontal(|ui| {
        let button = widgets::icon_button(ui, Glyph::Panels, 22.0, tr("panels-tip"), true);
        let chosen = egui::Popup::menu(&button)
            .show(|ui| actions::items(ui, actions::WINDOW, canvas, panels))
            .and_then(|shown| shown.inner);
        if let Some(action) = chosen {
            actions::run(action, canvas, files, clipboard, panels);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let session = canvas.session();
            let (can_undo, can_redo) = (
                session.undo_label().is_some(),
                session.redo_label().is_some(),
            );
            if widgets::icon_button(ui, Glyph::Redo, 22.0, tr("edit-redo"), can_redo).clicked() {
                report_bool(canvas.edit(Session::redo));
            }
            if widgets::icon_button(ui, Glyph::Undo, 22.0, tr("edit-undo"), can_undo).clicked() {
                report_bool(canvas.edit(Session::undo));
            }
        });
    });
}

/// The tool rail.
pub fn rail(ui: &mut egui::Ui, canvas: &mut Canvas, keys: &Keymap) {
    ui.spacing_mut().item_spacing.y = 2.0;
    let tool = canvas.session().tool();
    for (each, action, glyph, key) in [
        (Tool::Pen, Action::Brush, Glyph::Brush, "tool-brush"),
        (Tool::Eraser, Action::Eraser, Glyph::Eraser, "tool-eraser"),
        (Tool::Select, Action::Select, Glyph::Lasso, "tool-select"),
        (Tool::Wand, Action::Wand, Glyph::Wand, "tool-wand"),
        (Tool::Fill, Action::Fill, Glyph::Bucket, "tool-fill"),
        (Tool::Text, Action::Text, Glyph::Text, "tool-text-rail"),
        (
            Tool::Eyedropper,
            Action::Eyedropper,
            Glyph::Eyedropper,
            "tool-eyedropper-rail",
        ),
    ] {
        let shortcut = keys.text(action);
        if widgets::tool_button(ui, glyph, tr(key), &shortcut, tool == each).clicked() {
            canvas.edit(|session| session.set_tool(each));
        }
    }
}

/// A dock's header: its name, and a button that closes it.
fn dock_header(ui: &mut egui::Ui, title: &str, open: &mut bool) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title)
                .size(theme::SMALL)
                .color(theme::MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let close = ui
                .add(egui::Button::new(egui::RichText::new("×").color(theme::MUTED)).frame(false))
                .on_hover_text(tr("dock-close"));
            if close.clicked() {
                *open = false;
            }
        });
    });
}

/// A slider with its number field, on one row under a field label.
fn slider_row(
    ui: &mut egui::Ui,
    label: &str,
    name: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
    decimals: usize,
) -> egui::Response {
    ui.horizontal(|ui| {
        widgets::field_label(ui, label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let field = widgets::number(ui, value, range.clone(), suffix, decimals, 64.0, name);
            let width = ui.available_width().max(40.0);
            let slider = widgets::slider(ui, value, range, name, width);
            slider.union(field)
        })
        .inner
    })
    .inner
}

pub fn tool_settings(ui: &mut egui::Ui, canvas: &mut Canvas, panels: &mut Panels) {
    dock_header(ui, tr("tool-settings"), &mut panels.shown.tool_settings);
    let tool = canvas.session().tool();
    ui.add_space(4.0);
    match tool {
        Tool::Select => return selection_settings(ui, canvas),
        Tool::Wand | Tool::Fill => return fill_settings(ui, canvas, tool == Tool::Fill),
        Tool::Text => return text::settings(ui, canvas),
        Tool::Eyedropper => {
            ui.label(
                egui::RichText::new(tr("eyedropper-hint"))
                    .size(theme::SMALL)
                    .color(theme::MUTED),
            );
            return;
        }
        Tool::Pen | Tool::Eraser => {}
    }
    panels.presets.show(ui, canvas, tool);
    // Read after the presets: choosing one sets its width and stabilizer.
    let mut settings = match tool {
        Tool::Eraser => canvas.session().eraser,
        Tool::Pen | Tool::Select | Tool::Wand | Tool::Fill | Tool::Text | Tool::Eyedropper => {
            canvas.session().pen
        }
    };
    let before = settings;
    ui.add_space(4.0);
    let size_name = match tool {
        Tool::Eraser => tr("eraser-size"),
        Tool::Pen | Tool::Select | Tool::Wand | Tool::Fill | Tool::Text | Tool::Eyedropper => {
            tr("brush-size")
        }
    };
    let range = 1.0..=128.0;
    slider_row(
        ui,
        tr("size"),
        size_name,
        &mut settings.width,
        range,
        " px",
        0,
    );
    let mut stabilization = settings.stabilizer * 100.0;
    slider_row(
        ui,
        tr("stabilization"),
        tr("stabilization-name"),
        &mut stabilization,
        0.0..=100.0,
        "%",
        0,
    );
    settings.stabilizer = stabilization / 100.0;
    if tool == Tool::Pen {
        widgets::check_row(
            ui,
            tr("antialiasing"),
            tr("antialiasing-name"),
            &mut settings.antialias,
        )
        .on_hover_text(tr("antialiasing-tip"));
    }
    if settings != before {
        canvas.edit(|session| match tool {
            Tool::Eraser => session.eraser = settings,
            Tool::Pen | Tool::Select | Tool::Wand | Tool::Fill | Tool::Text | Tool::Eyedropper => {
                session.pen = settings
            }
        });
    }
}

/// The selection tool's mode and shapes, as 2.2.13's area select panel;
/// painting also shows how fills look.
fn selection_settings(ui: &mut egui::Ui, canvas: &mut Canvas) {
    widgets::field_label(ui, tr("lasso-mode"));
    let mut paints = canvas.session().lasso_paints;
    ui.horizontal(|ui| {
        ui.radio_value(&mut paints, false, tr("lasso-select"));
        ui.radio_value(&mut paints, true, tr("lasso-paint"));
    });
    let mut fill = canvas.session().fill;
    if paints {
        widgets::check_row(
            ui,
            tr("fill-antialias"),
            tr("fill-antialias"),
            &mut fill.antialias,
        );
    }
    if paints != canvas.session().lasso_paints || fill != canvas.session().fill {
        canvas.edit(|session| {
            session.lasso_paints = paints;
            session.fill = fill;
        });
    }
    ui.add_space(6.0);
    widgets::field_label(ui, tr("transform-method"));
    let current = canvas.session().transform_sampling;
    for (sampling, title, description) in [
        (Sampling::Smooth, "sampling-smooth", "sampling-smooth-tip"),
        (Sampling::Nearest, "sampling-pixels", "sampling-pixels-tip"),
    ] {
        if ui
            .radio(current == sampling, tr(title))
            .on_hover_text(tr(description))
            .clicked()
        {
            canvas.set_transform_sampling(sampling);
        }
    }
    ui.add_space(6.0);
    widgets::field_label(ui, tr("selection-shape"));
    let current = canvas.session().selection_shape;
    for (kind, title, description) in [
        (ShapeKind::Freehand, "shape-freehand", "shape-freehand-tip"),
        (
            ShapeKind::Rectangle,
            "shape-rectangle",
            "shape-rectangle-tip",
        ),
        (ShapeKind::Ellipse, "shape-ellipse", "shape-ellipse-tip"),
    ] {
        if shape_option(ui, current == kind, tr(title), tr(description)).clicked() {
            canvas.edit(|session| session.selection_shape = kind);
        }
    }
}

/// What the wand and the bucket read and how they compare, as 2.2.13's auto
/// select and paint bucket panels; the bucket also sets how fills look.
/// The wand shows the comparison too, as it uses it.
fn fill_settings(ui: &mut egui::Ui, canvas: &mut Canvas, bucket: bool) {
    let mut fill: FillSettings = canvas.session().fill;
    widgets::field_label(ui, tr("fill-reference"));
    for (reads, title, description) in [
        (Reads::Current, "reads-current", "reads-current-tip"),
        (Reads::Marked, "reads-marked", "reads-marked-tip"),
        (Reads::Visible, "reads-visible", "reads-visible-tip"),
    ] {
        if shape_option(ui, fill.reads == reads, tr(title), tr(description)).clicked() {
            fill.reads = reads;
        }
    }
    ui.add_space(6.0);
    widgets::field_label(ui, tr("fill-comparison"));
    ui.radio_value(&mut fill.by_colour, false, tr("compare-alpha"));
    ui.radio_value(&mut fill.by_colour, true, tr("compare-colour"));
    let mut tolerance = f32::from(fill.tolerance);
    ui.add_enabled_ui(fill.by_colour, |ui| {
        slider_row(
            ui,
            tr("fill-tolerance"),
            tr("fill-tolerance"),
            &mut tolerance,
            0.0..=255.0,
            "",
            0,
        );
    });
    fill.tolerance = tolerance.round().clamp(0.0, 255.0) as u8;
    if bucket {
        ui.add_space(4.0);
        widgets::check_row(
            ui,
            tr("fill-antialias"),
            tr("fill-antialias"),
            &mut fill.antialias,
        );
    }
    if fill != canvas.session().fill {
        canvas.edit(|session| session.fill = fill);
    }
}

/// A choice showing its name over a line saying what it does.
fn shape_option(
    ui: &mut egui::Ui,
    selected: bool,
    title: &str,
    description: &str,
) -> egui::Response {
    let size = egui::vec2(ui.available_width(), 44.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, title)
    });
    let painter = ui.painter();
    let (fill, edge) = match (selected, response.hovered()) {
        (true, _) => (theme::CONTROL, theme::accent()),
        (false, true) => (theme::HOVER, theme::BORDER),
        (false, false) => (egui::Color32::TRANSPARENT, theme::BORDER),
    };
    painter.rect(
        rect,
        egui::CornerRadius::same(7),
        fill,
        egui::Stroke::new(1.0, edge),
        egui::StrokeKind::Inside,
    );
    let at = |y: f32| rect.left_top() + egui::vec2(10.0, y);
    let font = |size: f32| egui::FontId::proportional(size);
    painter.text(
        at(6.0),
        egui::Align2::LEFT_TOP,
        title,
        font(theme::BODY),
        theme::TEXT,
    );
    painter.text(
        at(25.0),
        egui::Align2::LEFT_TOP,
        description,
        font(theme::SMALL),
        theme::MUTED,
    );
    response
}

/// Marching ants around the selection and the shape being dragged, as
/// 2.2.13 draws them: a light line under a dark dashed one that moves every
/// 120 ms. The shape is drawn by egui; the selection, which can be many small
/// pieces, by the GPU (`ugu_render::ants`), so it is returned with the
/// pending transform that moves it and the points its dashes have moved.
/// Neither takes a document render or canvas upload, and both stop while the
/// window is minimized. A pending transform also shows its box.
pub fn selection_overlay(
    ui: &mut egui::Ui,
    canvas: &Canvas,
) -> Option<(Arc<Selection>, Affine, f32)> {
    let session = canvas.session();
    let lasso = session.lasso();
    // A shape that will replace the selection hides it while dragged.
    let replacing = lasso.is_some_and(|lasso| lasso.combine == Combine::Replace);
    let selection = session.selection().filter(|_| !replacing);
    if selection.is_none() && lasso.is_none() {
        return None;
    }
    let phase = ants_phase(ui);
    if let Some(lasso) = lasso {
        ants(ui, canvas, &lasso_path(lasso), phase);
    }
    if let Some(corners) = canvas.transform_box() {
        transform_box(ui, canvas, &corners);
    }
    let moved = session
        .pending()
        .map_or(Affine::IDENTITY, |pending| pending.transform);
    selection.map(|selection| (selection.clone(), moved, phase))
}

/// A crosshair over the canvas while a press would pick a colour.
pub fn pick_cursor(ui: &mut egui::Ui, canvas: &Canvas) {
    let (alt, pointer) = ui.input(|input| (input.modifiers.alt, input.pointer.hover_pos()));
    let over = pointer.is_some_and(|pointer| {
        ui.max_rect().contains(pointer)
            && ui
                .ctx()
                .layer_id_at(pointer)
                .is_none_or(|layer| layer.order == egui::Order::Background)
    });
    if over && canvas.picks_with(alt) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
}

/// A dashed frame around placed text and what to do with it, as 2.2.13
/// draws them.
pub fn text_overlay(ui: &mut egui::Ui, canvas: &Canvas) {
    let Some([left, top, right, bottom]) = canvas.text_box() else {
        return;
    };
    let pad = 6.0;
    let rect = egui::Rect::from_min_max(
        to_screen(ui, canvas, [left, top]) - egui::vec2(pad, pad),
        to_screen(ui, canvas, [right, bottom]) + egui::vec2(pad, pad),
    );
    let painter = ui.painter();
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    for side in corners.windows(2) {
        painter.extend(egui::Shape::dashed_line(
            side,
            egui::Stroke::new(1.0, theme::accent()),
            4.0,
            3.0,
        ));
    }
    let hint = if canvas.session().text_content.trim().is_empty() {
        tr("text-type-hint")
    } else {
        tr("text-drag-hint")
    };
    painter.text(
        rect.left_bottom() + egui::vec2(0.0, 4.0),
        egui::Align2::LEFT_TOP,
        hint,
        egui::FontId::proportional(theme::SMALL),
        theme::MUTED,
    );
}

/// Document pixels to points.
fn to_screen(ui: &egui::Ui, canvas: &Canvas, [x, y]: [f64; 2]) -> egui::Pos2 {
    let ppp = ui.ctx().pixels_per_point();
    let placement = canvas.placement();
    egui::pos2(
        (placement.offset[0] + x as f32 * placement.scale) / ppp,
        (placement.offset[1] + y as f32 * placement.scale) / ppp,
    )
}

/// The box's sides and handles, and the pointer shape for what a drag from
/// under the pointer would do.
fn transform_box(ui: &mut egui::Ui, canvas: &Canvas, corners: &[[f64; 2]; 8]) {
    let points = corners.map(|corner| to_screen(ui, canvas, corner));
    let painter = ui.painter();
    painter.add(egui::Shape::closed_line(
        points[..4].to_vec(),
        egui::Stroke::new(1.0, theme::accent()),
    ));
    for point in points {
        painter.rect(
            egui::Rect::from_center_size(point, egui::vec2(8.0, 8.0)),
            egui::CornerRadius::same(1),
            egui::Color32::WHITE,
            egui::Stroke::new(1.0, theme::accent_text()),
            egui::StrokeKind::Middle,
        );
    }
    let ppp = ui.ctx().pixels_per_point();
    let Some(pointer) = ui
        .input(|input| input.pointer.hover_pos())
        .filter(|pointer| ui.max_rect().contains(*pointer))
    else {
        return;
    };
    let icon = match canvas.grip_at([f64::from(pointer.x * ppp), f64::from(pointer.y * ppp)]) {
        Some(Grip::Move) => egui::CursorIcon::Move,
        Some(Grip::Rotate) => egui::CursorIcon::Alias,
        Some(Grip::Scale(handle)) => {
            // The handle's direction from the middle, as the box is now.
            let middle = points[..4]
                .iter()
                .fold(egui::Vec2::ZERO, |sum, point| sum + point.to_vec2())
                / 4.0;
            let at = HANDLES.iter().position(|each| *each == handle).unwrap_or(0);
            let direction = points[at].to_vec2() - middle;
            let eighths = direction.y.atan2(direction.x) / std::f32::consts::FRAC_PI_4;
            match eighths.round().rem_euclid(4.0) as u8 {
                0 => egui::CursorIcon::ResizeHorizontal,
                1 => egui::CursorIcon::ResizeNwSe,
                2 => egui::CursorIcon::ResizeVertical,
                _ => egui::CursorIcon::ResizeNeSw,
            }
        }
        None => return,
    };
    ui.ctx().set_cursor_icon(icon);
}

/// The bar of selection actions under the selection, as 2.2.13's: transform,
/// flip, apply and cancel, duplicate, delete and deselect. Hidden while a
/// shape or the box is dragged.
pub fn selection_actions(ui: &mut egui::Ui, canvas: &mut Canvas, panels: &mut Panels) {
    const MARGIN: f32 = 8.0;
    const GAP: f32 = 6.0;
    let session = canvas.session();
    if session.selection().is_none() || session.lasso().is_some() || canvas.is_dragging() {
        return;
    }
    let pending = session.pending().map(|pending| pending.keep_source);
    let Some([left, top, right, bottom]) = canvas.selection_bounds() else {
        return;
    };
    let area = ui.max_rect();
    let low = to_screen(ui, canvas, [left, bottom]);
    let high = to_screen(ui, canvas, [right, top]);
    let id = egui::Id::new("selection actions");
    let size = ui
        .ctx()
        .memory(|memory| memory.area_rect(id))
        .map_or(egui::vec2(300.0, 40.0), |rect| rect.size());
    let x = ((low.x + high.x) / 2.0 - size.x / 2.0).clamp(
        area.left() + MARGIN,
        (area.right() - size.x - MARGIN).max(area.left() + MARGIN),
    );
    let mut y = low.y + GAP;
    if y + size.y > area.bottom() - MARGIN {
        y = high.y - size.y - GAP;
    }
    let y = y.clamp(
        area.top() + MARGIN,
        (area.bottom() - size.y - MARGIN).max(area.top() + MARGIN),
    );
    egui::Area::new(id)
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(x, y))
        .show(ui.ctx(), |ui| {
            egui::Frame::new()
                .fill(theme::PANEL)
                .stroke(egui::Stroke::new(1.0, theme::BORDER))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::same(4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| action_buttons(ui, canvas, panels, pending));
                });
        });
}

/// `pending`: whether a transform is pending, and whether it duplicates.
fn action_buttons(
    ui: &mut egui::Ui,
    canvas: &mut Canvas,
    panels: &mut Panels,
    pending: Option<bool>,
) {
    ui.spacing_mut().item_spacing.x = 2.0;
    let keys = &panels.keys;
    let button = |ui: &mut egui::Ui, glyph, name, tip, action| {
        let tip = widgets::with_key(tr(tip), &keys.text(action));
        widgets::icon_button_tip(ui, glyph, 18.0, tr(name), &tip, true).clicked()
    };
    if pending.is_none()
        && button(
            ui,
            Glyph::Scale,
            "transform-selection",
            "transform-selection-tip",
            Action::Transform,
        )
    {
        canvas.begin_transform();
    }
    if button(
        ui,
        Glyph::MirrorHorizontal,
        "flip-horizontal",
        "flip-horizontal",
        Action::FlipHorizontal,
    ) {
        canvas.flip(true);
    }
    if button(
        ui,
        Glyph::MirrorVertical,
        "flip-vertical",
        "flip-vertical",
        Action::FlipVertical,
    ) {
        canvas.flip(false);
    }
    if pending.is_some() {
        ui.separator();
        if button(
            ui,
            Glyph::Confirm,
            "transform-apply",
            "transform-apply-tip",
            Action::ApplyTransform,
        ) {
            canvas.apply_transform();
        }
        if button(
            ui,
            Glyph::Cancel,
            "transform-cancel",
            "transform-cancel-tip",
            // Escape cancels it when it has no key of its own.
            if keys.key(Action::CancelTransform).is_some() {
                Action::CancelTransform
            } else {
                Action::Escape
            },
        ) {
            canvas.cancel_transform();
        }
    }
    ui.separator();
    let duplicate = pending == Some(true);
    let toggled = widgets::icon_toggle(
        ui,
        Glyph::Duplicate,
        18.0,
        tr("transform-duplicate"),
        tr("transform-duplicate-tip"),
        duplicate,
    );
    if toggled.clicked() {
        canvas.set_duplicate(!duplicate);
    }
    let restyle = restyle::available(canvas)
        && button(
            ui,
            Glyph::Brush,
            "restyle-selected",
            "restyle-selected-tip",
            Action::Restyle,
        );
    if button(
        ui,
        Glyph::Delete,
        "delete-selected",
        "delete-selected",
        Action::DeleteSelected,
    ) {
        canvas.delete_selected();
    }
    ui.separator();
    if button(
        ui,
        Glyph::Deselect,
        "edit-deselect",
        "edit-deselect",
        Action::Deselect,
    ) {
        canvas.edit(Session::deselect);
    }
    if restyle {
        restyle::open(canvas, panels);
    }
}

/// Points the dashes have moved: a step every 120 ms over a cycle of 8.
fn ants_phase(ui: &egui::Ui) -> f32 {
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(120));
    let step = (ui.ctx().input(|input| input.time) / 0.12).floor();
    8.0 - (step % 8.0) as f32
}

/// The outline of a shape being dragged, in document pixels.
fn lasso_path(lasso: &Lasso) -> Vec<[f64; 2]> {
    let first = lasso.points[0];
    let last = *lasso.points.last().unwrap_or(&first);
    match lasso.kind {
        ShapeKind::Freehand => lasso.points.clone(),
        ShapeKind::Rectangle => vec![first, [last[0], first[1]], last, [first[0], last[1]], first],
        ShapeKind::Ellipse => {
            let centre = [(first[0] + last[0]) / 2.0, (first[1] + last[1]) / 2.0];
            let radii = [
                (last[0] - first[0]).abs() / 2.0,
                (last[1] - first[1]).abs() / 2.0,
            ];
            (0..=64)
                .map(|step| {
                    let angle = std::f64::consts::TAU * f64::from(step) / 64.0;
                    [
                        centre[0] + radii[0] * angle.cos(),
                        centre[1] + radii[1] * angle.sin(),
                    ]
                })
                .collect()
        }
    }
}

fn ants(ui: &mut egui::Ui, canvas: &Canvas, path: &[[f64; 2]], offset: f32) {
    let light = egui::Stroke::new(
        1.8,
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 235),
    );
    let dark = egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(20, 20, 20, 245));
    let painter = ui.painter();
    let points: Vec<egui::Pos2> = path
        .iter()
        .map(|point| to_screen(ui, canvas, *point))
        .collect();
    painter.add(egui::Shape::line(points.clone(), light));
    painter.extend(egui::Shape::dashed_line_with_offset(
        &points,
        dark,
        &[4.0],
        &[4.0],
        offset,
    ));
}

pub fn wobble(ui: &mut egui::Ui, canvas: &mut Canvas, panels: &mut Panels) {
    dock_header(ui, tr("wobble-dock"), &mut panels.shown.wobble);
    let document = canvas.session().document();
    let current = canvas.session().current_layer();
    let layer_wobble = match document.layer(current).map(|layer| &layer.kind) {
        Some(LayerKind::Paint(paint)) => Some(paint.wobble),
        _ => None,
    };
    let (drawing, frames, fps) = (document.wobble, document.frames, document.frames_per_second);
    let scope = |layer: bool| {
        if layer {
            tr("wobble-layer")
        } else {
            tr("wobble-whole")
        }
    };
    egui::ComboBox::from_id_salt("wobble-scope")
        .width(ui.available_width())
        .selected_text(scope(panels.wobble_layer))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut panels.wobble_layer, false, scope(false));
            ui.add_enabled_ui(layer_wobble.is_some(), |ui| {
                ui.selectable_value(&mut panels.wobble_layer, true, scope(true));
            });
        })
        .response
        .widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, tr("wobble-scope"))
        });
    let on_layer = panels.wobble_layer && layer_wobble.is_some();
    let shown = match layer_wobble {
        Some(Some(own)) if on_layer => own,
        _ => drawing,
    };
    if on_layer {
        let overridden = matches!(layer_wobble, Some(Some(_)));
        let follow = ui
            .add_enabled(overridden, egui::Button::new(tr("wobble-follow")))
            .on_hover_text(tr("wobble-follow-tip"));
        if follow.clicked() {
            panels.report(canvas.edit(|session| {
                session.update_layer(current, "Follow drawing settings", |layer| {
                    if let LayerKind::Paint(paint) = &mut layer.kind {
                        paint.wobble = None;
                    }
                })
            }));
        }
    }
    widgets::field_label(ui, tr("wobble"));
    let mut edited = panels.wobble.unwrap_or(shown);
    // A drag commits when it stops, anything else at once.
    let mut ended = false;
    let mut track = |response: egui::Response| {
        ended |= response.drag_stopped() || (response.changed() && !response.dragged());
        response.changed()
    };
    let (min, max) = (*limits::WOBBLE_AMOUNT.start(), *limits::WOBBLE_AMOUNT.end());
    let mut amount = edited.amount;
    let response = ui
        .horizontal(|ui| {
            let preview = wobble_preview(ui, amount, ui.ctx().input(|input| input.time));
            preview.on_hover_text(tr("wobble-preview-tip"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let field =
                    widgets::number(ui, &mut amount, min..=max, " px", 1, 64.0, tr("wobble"));
                let width = ui.available_width().max(40.0);
                widgets::slider(ui, &mut amount, min..=max, tr("wobble"), width).union(field)
            })
            .inner
        })
        .inner;
    let mut changed = track(response);
    edited.amount = (amount * 10.0).round() / 10.0;

    let motion = &mut edited.motion;
    let style_name = |style: MotionStyle| match style {
        MotionStyle::Classic => tr("motion-classic"),
        MotionStyle::Smooth => tr("motion-smooth"),
        MotionStyle::Stepped => tr("motion-stepped"),
    };
    let before = motion.style;
    form_row(ui, tr("motion-style"), |ui| {
        egui::ComboBox::from_id_salt("motion-style")
            .width(ui.available_width())
            .selected_text(style_name(motion.style))
            .show_ui(ui, |ui| {
                for style in [
                    MotionStyle::Classic,
                    MotionStyle::Smooth,
                    MotionStyle::Stepped,
                ] {
                    ui.selectable_value(&mut motion.style, style, style_name(style));
                }
            })
            .response
            .widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, tr("motion-style"))
            });
    });
    let restyled = motion.style != before;
    // Poses, detail, linking and randomness move Smooth and Stepped only.
    ui.add_enabled_ui(motion.style != MotionStyle::Classic, |ui| {
        let mut poses = motion.poses.min(frames) as f32;
        let response = form_row(ui, tr("motion-poses"), |ui| {
            widgets::number(
                ui,
                &mut poses,
                1.0..=frames as f32,
                "",
                0,
                64.0,
                tr("motion-poses"),
            )
        });
        if track(response) {
            changed = true;
            motion.poses = poses.round() as u32;
        }
        let range = limits::MOTION_DETAIL;
        let mut detail = motion.detail as f32;
        let response = form_row(ui, tr("motion-detail"), |ui| {
            let range = *range.start() as f32..=*range.end() as f32;
            widgets::number(ui, &mut detail, range, "", 0, 64.0, tr("motion-detail"))
        });
        if track(response) {
            changed = true;
            motion.detail = detail.round() as u32;
        }
        changed |= track(percent_row(ui, tr("motion-linked"), &mut motion.linked));
        changed |= track(percent_row(
            ui,
            tr("motion-randomness"),
            &mut motion.randomness,
        ));
    });
    if track(widgets::checkbox(
        ui,
        &mut motion.broken,
        tr("motion-broken"),
    )) {
        changed = true;
    }
    ui.add_enabled_ui(motion.broken, |ui| {
        changed |= track(percent_row(
            ui,
            tr("motion-break-amount"),
            &mut motion.break_amount,
        ));
        let range = limits::BREAK_RANGE;
        let mut length = motion.break_range;
        let response = form_row(ui, tr("motion-break-range"), |ui| {
            widgets::number(
                ui,
                &mut length,
                range,
                " px",
                1,
                64.0,
                tr("motion-break-range"),
            )
        });
        if track(response) {
            changed = true;
            motion.break_range = (length * 10.0).round() / 10.0;
        }
    });

    if restyled {
        changed = true;
        ended = true;
    }
    if changed {
        panels.wobble = Some(edited);
    }
    if ended
        && let Some(value) = panels.wobble.take()
        && value != shown
    {
        let result = if on_layer {
            canvas.edit(|session| {
                session.update_layer(current, "Layer wobble", |layer| {
                    if let LayerKind::Paint(paint) = &mut layer.kind {
                        paint.wobble = Some(value);
                    }
                })
            })
        } else {
            canvas.edit(|session| session.set_animation(frames, fps, value))
        };
        panels.report(result);
    }
}

/// A caption on the left and `control` on the right, as 2.2.13's form rows.
fn form_row<R>(ui: &mut egui::Ui, label: &str, control: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.horizontal(|ui| {
        widgets::field_label(ui, label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control)
            .inner
    })
    .inner
}

/// A share from 0 to 1 as a slider and a field of whole percent.
fn percent_row(ui: &mut egui::Ui, label: &str, share: &mut f32) -> egui::Response {
    let mut percent = *share * 100.0;
    let response = form_row(ui, label, |ui| {
        let field = widgets::number(ui, &mut percent, 0.0..=100.0, "%", 0, 64.0, label);
        let width = ui.available_width().max(40.0);
        widgets::slider(ui, &mut percent, 0.0..=100.0, label, width).union(field)
    });
    if response.changed() {
        *share = percent.round() / 100.0;
    }
    response
}

/// A short line wobbling by `amount`, moving only while the pointer is on
/// it so that an idle window draws nothing.
fn wobble_preview(ui: &mut egui::Ui, amount: f32, time: f64) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(58.0, 24.0), egui::Sense::hover());
    let phase = if response.hovered() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(80));
        (time * 8.0).floor()
    } else {
        0.0
    };
    let points: Vec<egui::Pos2> = (0..=24)
        .map(|step| {
            let position = f64::from(step) / 24.0;
            let turn = std::f64::consts::TAU * position;
            let offset =
                0.62 * (1.25 * turn + phase).sin() + 0.38 * (2.75 * turn + 0.9 + phase * 1.3).sin();
            let x = rect.left() + 4.0 + (rect.width() - 8.0) * position as f32;
            let y = rect.center().y - (f64::from(amount) * 0.9 * offset) as f32;
            egui::pos2(x, y.clamp(rect.top() + 2.0, rect.bottom() - 2.0))
        })
        .collect();
    ui.painter().add(egui::Shape::line(
        points,
        egui::Stroke::new(2.0, theme::accent()),
    ));
    response
}

pub fn color(ui: &mut egui::Ui, canvas: &mut Canvas, panels: &mut Panels) {
    dock_header(ui, tr("color-dock"), &mut panels.shown.color);
    panels.color.show(ui, canvas);
}

pub fn color_history(ui: &mut egui::Ui, canvas: &mut Canvas, panels: &mut Panels) {
    dock_header(ui, tr("color-history"), &mut panels.shown.color_history);
    history::show(ui, canvas);
}

pub fn layers(ui: &mut egui::Ui, canvas: &mut Canvas, panels: &mut Panels) {
    dock_header(ui, tr("layers"), &mut panels.shown.layers);
    let Panels {
        layers, refusal, ..
    } = panels;
    layers.show(ui, canvas, refusal);
}

/// The animation bar: play, the frame and frame count, the scrubber and the
/// playback speed.
pub fn animation_bar(ui: &mut egui::Ui, canvas: &mut Canvas, panels: &mut Panels) {
    // With the wobble animation off in the settings, as 2.2.13's timeline.
    ui.add_enabled_ui(canvas.animation_allowed(), |ui| {
        animation_controls(ui, canvas, panels)
    });
}

fn animation_controls(ui: &mut egui::Ui, canvas: &mut Canvas, panels: &mut Panels) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let playing = canvas.is_playing();
        let (rect, play) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::click());
        let fill = match (playing, play.hovered()) {
            (true, true) => theme::accent_pressed(),
            (true, false) => theme::accent(),
            (false, true) => theme::HOVER,
            (false, false) => theme::CONTROL,
        };
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(7), fill);
        let (glyph, ink) = if playing {
            (Glyph::Pause, theme::accent_text())
        } else {
            (Glyph::Play, theme::TEXT)
        };
        icons::paint(
            ui.painter(),
            egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(20.0)),
            glyph,
            ink,
            0.0,
        );
        play.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, playing, tr("play"))
        });
        if play.on_hover_text(tr("play-tip")).clicked() {
            canvas.toggle_playback();
        }

        let document = canvas.session().document();
        let (frames, fps, wobble) = (document.frames, document.frames_per_second, document.wobble);
        let mut frame = frame_in_cycle(canvas.session().frame(), frames) as f32 + 1.0;
        let field = widgets::number(
            ui,
            &mut frame,
            1.0..=frames as f32,
            "",
            0,
            46.0,
            tr("frame-current"),
        )
        .on_hover_text(tr("frame-current"));
        if field.changed() {
            if canvas.is_playing() {
                canvas.toggle_playback();
            }
            canvas.edit(|session| session.set_frame(frame as i64 - 1));
        }
        ui.label(egui::RichText::new("/").color(theme::MUTED));
        let (mut count, mut speed) = panels.animation.unwrap_or((frames as f32, fps));
        let (low, high) = (*limits::FRAMES.start() as f32, *limits::FRAMES.end() as f32);
        let count_field =
            widgets::number(ui, &mut count, low..=high, "", 0, 46.0, tr("frame-count"))
                .on_hover_text(tr("frame-count"));

        let right = 46.0 + 30.0 + 90.0 + 30.0;
        let width = (ui.available_width() - right).max(160.0);
        let current = frame_in_cycle(canvas.session().frame(), frames);
        if let Some(chosen) = scrubber(ui, current, frames, canvas.is_playing(), width) {
            if canvas.is_playing() {
                canvas.toggle_playback();
            }
            canvas.edit(|session| session.set_frame(i64::from(chosen)));
        }

        ui.label(
            egui::RichText::new(tr("fps"))
                .size(theme::SMALL)
                .color(theme::MUTED),
        );
        let (low, high) = (
            *limits::FRAMES_PER_SECOND.start(),
            *limits::FRAMES_PER_SECOND.end(),
        );
        let speed_field = widgets::number(ui, &mut speed, low..=high, "", 0, 46.0, tr("fps-name"))
            .on_hover_text(tr("fps-tip"));
        let edited = count_field.changed() || speed_field.changed();
        if edited {
            panels.animation = Some((count, speed));
        }
        let ended = [&count_field, &speed_field].iter().any(|field| {
            field.drag_stopped() || (field.changed() && !field.dragged()) || field.lost_focus()
        });
        if ended && let Some((count, speed)) = panels.animation.take() {
            let result =
                canvas.edit(|session| session.set_animation(count.round() as u32, speed, wobble));
            panels.report(result);
        }
        let tip = widgets::with_key(tr("bar-hide-tip"), &panels.keys.text(Action::AnimationBar));
        let hide = widgets::text_icon_button(ui, Glyph::MoveDown, tr("bar-hide"), &tip);
        if hide.clicked() {
            panels.shown.animation_bar = false;
        }
    });
}

/// One slot per frame along a track. Returns the frame clicked or dragged
/// to.
fn scrubber(
    ui: &mut egui::Ui,
    current: u32,
    frames: u32,
    playing: bool,
    width: f32,
) -> Option<u32> {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::click_and_drag());
    let track = rect.shrink2(egui::vec2(0.0, 2.5));
    let slot = track.width() / frames.max(1) as f32;
    let slot_at = |x: f32| {
        (((x - track.left()) / slot).floor() as i64).clamp(0, i64::from(frames) - 1) as u32
    };
    let painter = ui.painter();
    let focused = response.has_focus();
    painter.rect(
        track,
        egui::CornerRadius::same(6),
        theme::STATUS,
        egui::Stroke::new(
            1.0,
            if focused {
                theme::accent()
            } else {
                theme::BORDER
            },
        ),
        egui::StrokeKind::Inside,
    );
    let slot_rect = |index: u32| {
        egui::Rect::from_min_size(
            egui::pos2(track.left() + slot * index as f32, track.top()),
            egui::vec2(slot, track.height()),
        )
        .shrink(2.0)
    };
    if let Some(hover) = response.hover_pos() {
        painter.rect_filled(
            slot_rect(slot_at(hover.x)),
            egui::CornerRadius::same(4),
            theme::CONTROL,
        );
    }
    for index in 0..frames {
        painter.circle_filled(slot_rect(index).center(), 1.4, theme::DISABLED);
    }
    let head = slot_rect(current);
    if playing {
        let bar = egui::Rect::from_min_max(egui::pos2(head.left(), head.bottom() - 3.0), head.max);
        painter.rect_filled(
            bar,
            egui::CornerRadius::same(1),
            theme::accent().gamma_multiply(0.55),
        );
    } else {
        let head = egui::Rect::from_center_size(
            head.center(),
            egui::vec2(head.width().max(6.0), head.height()),
        );
        painter.rect_filled(head, egui::CornerRadius::same(4), theme::accent());
    }
    let name = tr_with(
        "scrubber-frame",
        &args([
            ("frame", (current + 1).to_string()),
            ("count", frames.to_string()),
        ]),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, &name));
    let mut chosen = response
        .interact_pointer_pos()
        .map(|pointer| slot_at(pointer.x));
    if focused {
        let step = ui.input(|input| {
            i64::from(input.key_pressed(egui::Key::ArrowRight))
                - i64::from(input.key_pressed(egui::Key::ArrowLeft))
        });
        if step != 0 {
            chosen = Some((i64::from(current) + step).rem_euclid(i64::from(frames)) as u32);
        }
    }
    chosen.filter(|&frame| frame != current)
}

/// The status bar: the last message on the left; the pointer, zoom and fit
/// on the right.
pub fn status_bar(
    ui: &mut egui::Ui,
    canvas: &mut Canvas,
    message: Option<(&str, bool)>,
    pointer: Option<[f64; 2]>,
) {
    ui.horizontal(|ui| {
        match message {
            Some((text, true)) => ui.colored_label(theme::accent(), text),
            Some((text, false)) => ui.colored_label(theme::MUTED, text),
            None => ui.colored_label(theme::MUTED, tr("status-ready")),
        };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let fit = widgets::text_icon_button(ui, Glyph::FitView, tr("fit"), tr("fit-tip"));
            if fit.clicked() {
                canvas.fit();
            }
            let scale = canvas.scale();
            let mut percent = (scale * 100.0) as f32;
            let (low, high) = (
                *ZOOM_RANGE.start() as f32 * 100.0,
                *ZOOM_RANGE.end() as f32 * 100.0,
            );
            let field = widgets::number(
                ui,
                &mut percent,
                low..=high,
                "%",
                0,
                72.0,
                tr("canvas-zoom-percent"),
            )
            .on_hover_text(tr("canvas-zoom-percent"));
            if field.changed() {
                canvas.zoom_to(f64::from(percent) / 100.0);
            }
            // Logarithmic, so every step reads as the same change in size.
            let mut position = (scale.ln() as f32 - low.ln() + 100f32.ln()) / ((high / low).ln());
            let slider = widgets::slider(ui, &mut position, 0.0..=1.0, tr("canvas-zoom"), 96.0);
            if slider.changed() {
                let percent = low * (high / low).powf(position);
                canvas.zoom_to(f64::from(percent) / 100.0);
            }
            let text = pointer.map_or_else(String::new, |[x, y]| {
                tr_with(
                    "status-pointer",
                    &args([("x", format!("{x:.0}")), ("y", format!("{y:.0}"))]),
                )
            });
            ui.add_sized(
                [150.0, 20.0],
                egui::Label::new(egui::RichText::new(text).color(theme::MUTED)),
            );
        });
    });
}
