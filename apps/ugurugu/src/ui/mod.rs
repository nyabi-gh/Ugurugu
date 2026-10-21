// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The window around the canvas, laid out as 2.2.13: menus on top, with
//! 2.2.13's quick access buttons at their right end, the tool rail and tool
//! settings on the left, wobble, colour and layers on the right, the
//! animation bar under the canvas and the status bar at the bottom. Only
//! what 3.0 can already do is shown.

mod actions;
mod brushes;
mod color;
mod dock;
mod focus;
mod help;
mod history;
#[cfg(test)]
mod keyboard;
mod layers;
mod motion;
mod resize;
mod restyle;
mod settings;
mod tabs;
mod text;
mod timeline;
mod window;

use crate::canvas::OnionSettings;
use crate::i18n::args;
use std::sync::Arc;
use ugu_core::document::{LayerKind, limits};
use ugu_core::edit::{EditError, Outcome};
use ugu_core::motion::frame_in_cycle;

use ugu_core::ops::{Affine, MotionStyle, Sampling, Wobble};
use ugu_core::selection::{Combine, Selection};
use ugu_session::{FillSettings, Lasso, Reads, Session, ShapeKind, Tool};

use crate::canvas::{BrushCursor, Canvas, Grip, HANDLES, RulerGrip, ZOOM_RANGE};
use crate::clipboard::Clipboard;
use crate::files::Files;
use crate::i18n::{tr, tr_with};
use crate::icons::{self, Glyph};
use crate::layout::Layout;
use crate::settings::Store;
use crate::shortcuts::{Action, Keymap};
use crate::theme;
use crate::widgets;

/// What the panels keep between frames: which are open, and edits in
/// progress that are committed as one undo step when they end.
#[derive(Default)]
pub struct Panels {
    pub layers: layers::LayerDock,
    pub color: color::ColorDock,
    presets: brushes::Presets,
    /// Where the panels are, and whether they and the animation bar are
    /// open.
    pub layout: Layout,
    docks: dock::Docks,
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
    resize: Option<resize::ImageSize>,
    /// The stroke properties dialog, while open.
    restyle: Option<restyle::Dialog>,
    /// The settings dialog, while open.
    settings: Option<settings::Dialog>,
    /// Whether the help window is open.
    help: bool,
    /// Which action each key runs.
    pub keys: Keymap,
    focus: focus::Return,
    /// The menu titles' ids, from the last frame.
    menus: [Option<egui::Id>; actions::MENUS.len()],
    /// The generation of the document the panels last showed.
    document: u64,
    /// The document tab being dragged, by where it is now.
    tab_drag: Option<usize>,
}

impl Panels {
    /// Drops edits under way in the panels when another document is shown:
    /// another tab, or one made, opened or recovered.
    pub fn follow_document(&mut self, canvas: &Canvas) {
        let shown = canvas.generation();
        if shown == self.document {
            return;
        }
        self.layers.switch_document(self.document, shown);
        self.layers.keep_documents(&canvas.tab_generations());
        self.document = shown;
        self.animation = None;
        self.wobble = None;
        self.refusal = None;
    }

    /// Puts `layout` in place of the one shown.
    pub fn set_layout(&mut self, layout: Layout) {
        self.layout = layout;
        self.docks.replaced();
    }

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
use dock::{areas as dock_areas, floating as dock_floating};
pub use window::{Parts, Shown, window};

fn report_bool(step: &'static str, result: Result<bool, EditError>) {
    match result {
        Ok(done) => tracing::debug!(step, done, "history step"),
        Err(error) => tracing::warn!(%error, step, "undo or redo failed"),
    }
}

/// The menus, with Panels, undo and redo at the bar's right end. From the
/// keyboard, as in Windows: F10 or Alt alone goes to the first title and,
/// from a title, back, Alt and a title's letter opens that menu, Down opens
/// the focused one, and Left and Right go to the next menu while one is open.
pub fn menu_bar(
    ui: &mut egui::Ui,
    canvas: &mut Canvas,
    files: &mut Files,
    clipboard: &mut Clipboard,
    panels: &mut Panels,
) {
    let (go_to, open) = menu_keys(ui.ctx(), &panels.menus);
    let mut chosen = None;
    egui::MenuBar::new().ui(ui, |ui| {
        for (index, (title, _, entries)) in actions::MENUS.iter().enumerate() {
            let title = ui.menu_button(tr(title), |ui| {
                chosen = chosen.or(actions::items(ui, entries, canvas, panels));
            });
            let id = title.response.id;
            panels.menus[index] = Some(id);
            if go_to == Some(index) {
                title.response.request_focus();
                // Not also to the control the arrow points at.
                ui.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
                let popup = egui::Popup::default_response_id(&title.response);
                if open && !egui::Popup::is_id_open(ui.ctx(), popup) {
                    egui::Popup::open_id(ui.ctx(), popup);
                } else if !open {
                    egui::Popup::close_all(ui.ctx());
                }
            }
        }
        quick_buttons(ui, canvas, files, clipboard, panels);
    });
    if let Some(action) = chosen {
        actions::run(action, canvas, files, clipboard, panels);
    }
}

/// The menu the keys of this frame go to, if any, and whether to open it.
/// Takes those keys.
fn menu_keys(
    ctx: &egui::Context,
    menus: &[Option<egui::Id>; actions::MENUS.len()],
) -> (Option<usize>, bool) {
    if ctx.text_edit_focused() || ctx.memory(|memory| memory.top_modal_layer().is_some()) {
        return (None, false);
    }
    let popup_of = |id: egui::Id| egui::Popup::is_id_open(ctx, id.with("popup"));
    let open_menu = menus.iter().position(|id| id.is_some_and(popup_of));
    let focused = ctx.memory(|memory| memory.focused());
    let focused_menu = menus.iter().position(|id| id.is_some() && *id == focused);
    let count = menus.len();
    let mut found = (None, false);
    let mut leave = false;
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
            if modifiers.is_none()
                && key == egui::Key::F10
                && open_menu.is_none()
                && focused_menu.is_some()
            {
                leave = true;
                return false;
            }
            let to = if modifiers.is_none() {
                match (key, open_menu, focused_menu) {
                    (egui::Key::F10, ..) => Some((0, false)),
                    (egui::Key::ArrowRight, Some(menu), _) => Some(((menu + 1) % count, true)),
                    (egui::Key::ArrowLeft, Some(menu), _) => {
                        Some(((menu + count - 1) % count, true))
                    }
                    (egui::Key::ArrowDown, None, Some(menu)) => Some((menu, true)),
                    _ => None,
                }
            } else if modifiers.alt && !modifiers.ctrl && !modifiers.shift && !modifiers.mac_cmd {
                actions::MENUS
                    .iter()
                    .position(|(_, letter, _)| *letter == key)
                    .map(|menu| (menu, true))
            } else {
                None
            };
            let Some((menu, open)) = to else {
                return true;
            };
            found = (Some(menu), open);
            false
        });
    });
    if leave && found.0.is_none() {
        ctx.memory_mut(|memory| memory.surrender_focus(focused.expect("a menu")));
    }
    found
}

/// The canvas or image size, stroke properties or settings dialog and the
/// help window, while one is open.
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
    help::show(ctx, &mut panels.help);
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
    panels.set_layout(store.get().layout.clone());
    settings::apply(ctx, store.get(), canvas, files, &mut panels.keys);
}

/// Panels, undo and redo at the right end of the menu bar, which 2.2.13 put
/// in a quick access bar of their own. Laid out left to right, so that Tab
/// reaches them in the order they are seen.
fn quick_buttons(
    ui: &mut egui::Ui,
    canvas: &mut Canvas,
    files: &mut Files,
    clipboard: &mut Clipboard,
    panels: &mut Panels,
) {
    // As tall as a menu title, so the bar keeps its height.
    const GLYPH: f32 = 20.0;
    const GROUP_GAP: f32 = 12.0;
    let side = GLYPH + 8.0;
    let width = 3.0 * side + 2.0 * ui.spacing().item_spacing.x + GROUP_GAP;
    ui.add_space((ui.available_width() - width).max(0.0));
    let button = widgets::icon_button(ui, Glyph::Panels, GLYPH, tr("panels-tip"), true);
    let chosen = egui::Popup::menu(&button)
        .show(|ui| actions::items(ui, actions::WINDOW, canvas, panels))
        .and_then(|shown| shown.inner);
    if let Some(action) = chosen {
        actions::run(action, canvas, files, clipboard, panels);
    }
    ui.add_space(GROUP_GAP);
    let session = canvas.session();
    let (can_undo, can_redo) = (
        session.undo_label().is_some(),
        session.redo_label().is_some(),
    );
    let undo = actions::button_glyph(Action::Undo);
    if widgets::icon_button(ui, undo, GLYPH, tr("edit-undo"), can_undo).clicked() {
        report_bool("undo", canvas.edit(Session::undo));
    }
    let redo = actions::button_glyph(Action::Redo);
    if widgets::icon_button(ui, redo, GLYPH, tr("edit-redo"), can_redo).clicked() {
        report_bool("redo", canvas.edit(Session::redo));
    }
}

/// The tool rail; the move layer tool only with the animation tools.
pub fn rail(ui: &mut egui::Ui, canvas: &mut Canvas, keys: &Keymap, animation_tools: bool) {
    ui.spacing_mut().item_spacing.y = 2.0;
    let tool = canvas.tool();
    for (each, action, key) in [
        (Tool::Pen, Action::Brush, "tool-brush"),
        (Tool::Eraser, Action::Eraser, "tool-eraser"),
        (Tool::Select, Action::Select, "tool-select"),
        (Tool::Wand, Action::Wand, "tool-wand"),
        (Tool::Fill, Action::Fill, "tool-fill"),
        (Tool::Text, Action::Text, "tool-text-rail"),
        (Tool::Shape, Action::Shape, "tool-shape-rail"),
        (Tool::Symmetry, Action::SymmetryRuler, "tool-symmetry-rail"),
        (Tool::Move, Action::MoveLayer, "tool-move-rail"),
        (Tool::Eyedropper, Action::Eyedropper, "tool-eyedropper-rail"),
    ] {
        if each == Tool::Move && !animation_tools {
            continue;
        }
        let shortcut = keys.text(action);
        let glyph = actions::button_glyph(action);
        if widgets::tool_button(ui, glyph, tr(key), &shortcut, tool == each).clicked() {
            canvas.choose_tool(each);
        }
    }
}

/// A slider with its number field, on one row after its name.
fn slider_row(
    ui: &mut egui::Ui,
    label: &str,
    name: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
    decimals: usize,
) -> egui::Response {
    widgets::form_row(ui, label, |ui| {
        let field = widgets::number(ui, value, range.clone(), suffix, decimals, 64.0, name);
        let width = ui.available_width().max(40.0);
        widgets::slider(ui, value, range, name, width).union(field)
    })
}

pub fn tool_settings(ui: &mut egui::Ui, canvas: &mut Canvas, panels: &mut Panels) {
    let tool = canvas.tool();
    ui.add_space(theme::space::XS);
    // A placed image or pasted pixels wait with another tool too, and their
    // resampling is chosen here, pixel art's whole pixels included.
    if tool != Tool::Select && canvas.session().pending().is_some() {
        sampling_choice(ui, canvas);
        ui.add_space(theme::space::S);
    }
    match tool {
        Tool::Select => return selection_settings(ui, canvas),
        Tool::Wand | Tool::Fill => return fill_settings(ui, canvas, tool == Tool::Fill),
        Tool::Text => return text::settings(ui, canvas),
        Tool::Move => return motion::settings(ui, canvas, &panels.keys),
        Tool::Symmetry => return symmetry_settings(ui, canvas),
        Tool::Eyedropper => {
            ui.label(
                egui::RichText::new(tr("eyedropper-hint"))
                    .size(theme::SMALL)
                    .color(theme::muted()),
            );
            return;
        }
        // Drawn with the pen, whose settings follow.
        Tool::Shape => shape_settings(ui, canvas),
        Tool::Pen | Tool::Eraser => {}
    }
    panels.presets.show(ui, canvas, tool);
    // Read after the presets: choosing one sets its width and stabilizer.
    let mut settings = *canvas.brushes().get(tool);
    let before = settings;
    ui.add_space(theme::space::XS);
    let size_name = match tool {
        Tool::Eraser => tr("eraser-size"),
        Tool::Pen
        | Tool::Select
        | Tool::Wand
        | Tool::Fill
        | Tool::Text
        | Tool::Eyedropper
        | Tool::Shape
        | Tool::Move
        | Tool::Symmetry => tr("brush-size"),
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
    let pixel = settings.preset.brush.engine == ugu_core::store::BrushEngine::Pixel;
    if pixel {
        settings.width = settings.width.round().max(1.0);
    } else {
        // The whole stroke at once, as Clip Studio Paint's opacity: a pen
        // does not build up where it crosses itself. Pixel brushes draw
        // opaque.
        let opacity_name = match tool {
            Tool::Eraser => tr("eraser-opacity"),
            _ => tr("brush-opacity"),
        };
        let mut opacity = settings.opacity * 100.0;
        slider_row(
            ui,
            tr("opacity"),
            opacity_name,
            &mut opacity,
            1.0..=100.0,
            "%",
            0,
        );
        settings.opacity = opacity / 100.0;
    }
    // A shape is not drawn by hand, so nothing steadies it.
    if tool != Tool::Shape {
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
    }
    // A pixel brush never antialiases. Unlike 2.2.13's, whose erasers
    // always antialias, an eraser has the choice too.
    if matches!(tool, Tool::Pen | Tool::Eraser | Tool::Shape) && !pixel {
        widgets::check_row(
            ui,
            tr("antialiasing"),
            tr("antialiasing-name"),
            &mut settings.antialias,
        )
        .on_hover_text(tr("antialiasing-tip"));
    }
    if settings != before {
        *canvas.brushes_mut().get_mut(tool) = settings;
    }
}

/// The symmetry ruler of the document shown.
fn symmetry_settings(ui: &mut egui::Ui, canvas: &mut Canvas) {
    use ugu_session::Symmetry;
    let mut symmetry = canvas.symmetry();
    let before = symmetry;
    widgets::panel_heading(ui, tr("symmetry"));
    ui.label(
        egui::RichText::new(tr("symmetry-hint"))
            .size(theme::SMALL)
            .color(theme::muted()),
    );
    ui.add_space(theme::space::S);
    widgets::check_row(
        ui,
        tr("symmetry-on"),
        tr("symmetry-on-name"),
        &mut symmetry.on,
    )
    .on_hover_text(tr("view-symmetry-tip"));
    ui.add_enabled_ui(symmetry.on, |ui| {
        let mut lines = symmetry.lines as f32;
        let range = Symmetry::LINES;
        slider_row(
            ui,
            tr("symmetry-lines"),
            tr("symmetry-lines-name"),
            &mut lines,
            *range.start() as f32..=*range.end() as f32,
            "",
            0,
        );
        symmetry.lines = (lines.round() as u32).clamp(*range.start(), *range.end());
        widgets::check_row(
            ui,
            tr("symmetry-mirror"),
            tr("symmetry-mirror-name"),
            &mut symmetry.mirror,
        )
        .on_hover_text(tr("symmetry-mirror-tip"));
        let mut angle = symmetry.angle as f32;
        slider_row(
            ui,
            tr("symmetry-angle"),
            tr("symmetry-angle-name"),
            &mut angle,
            -180.0..=180.0,
            "°",
            0,
        );
        if angle != symmetry.angle as f32 {
            symmetry.angle = f64::from(angle);
        }
        let canvas_size = canvas.session().document().canvas;
        let [mut x, mut y] = symmetry.centre.map(|value| value as f32);
        for (label, value, edge) in [
            (tr("symmetry-x"), &mut x, canvas_size[0]),
            (tr("symmetry-y"), &mut y, canvas_size[1]),
        ] {
            widgets::form_row(ui, label, |ui| {
                widgets::number(ui, value, 0.0..=edge as f32, " px", 1, 96.0, label);
            });
        }
        if [x, y] != symmetry.centre.map(|value| value as f32) {
            symmetry.centre = [f64::from(x), f64::from(y)];
        }
        if ui
            .button(tr("symmetry-centre"))
            .on_hover_text(tr("symmetry-centre-tip"))
            .clicked()
        {
            symmetry.centre = canvas_size.map(|edge| f64::from(edge) / 2.0);
        }
    });
    if symmetry != before {
        canvas.set_symmetry(symmetry);
    }
}

/// Which shape the shape tool drags out, and its corners.
fn shape_settings(ui: &mut egui::Ui, canvas: &mut Canvas) {
    use ugu_session::{Shape, ShapePaint, ShapeSettings};
    let mut settings = canvas.tools().shape;
    widgets::panel_heading(ui, tr("shape-kind"));
    widgets::segmented(
        ui,
        &mut settings.shape,
        &[
            (Shape::Line, tr("shape-line")),
            (Shape::Rectangle, tr("shape-rectangle")),
            (Shape::Ellipse, tr("shape-ellipse")),
            (Shape::Polygon, tr("shape-polygon")),
            (Shape::Star, tr("shape-star")),
            (Shape::Parallelogram, tr("shape-parallelogram")),
            (Shape::Trapezoid, tr("shape-trapezoid")),
            (Shape::Semicircle, tr("shape-semicircle")),
            (Shape::Moon, tr("shape-moon")),
        ],
        3,
    );
    let whole = |ui: &mut egui::Ui,
                 count: &mut u32,
                 name: &str,
                 range: std::ops::RangeInclusive<u32>,
                 unit: &str| {
        let mut value = *count as f32;
        slider_row(
            ui,
            name,
            name,
            &mut value,
            *range.start() as f32..=*range.end() as f32,
            unit,
            0,
        );
        *count = (value.round() as u32).clamp(*range.start(), *range.end());
    };
    match settings.shape {
        Shape::Polygon => whole(
            ui,
            &mut settings.sides,
            tr("shape-sides"),
            ShapeSettings::CORNERS,
            "",
        ),
        Shape::Star => {
            whole(
                ui,
                &mut settings.points,
                tr("shape-points"),
                ShapeSettings::CORNERS,
                "",
            );
            whole(
                ui,
                &mut settings.inner,
                tr("shape-inner"),
                ShapeSettings::INNER,
                "%",
            );
        }
        _ => {}
    }
    if settings.shape.closed() {
        widgets::panel_heading(ui, tr("shape-paint"));
        widgets::segmented(
            ui,
            &mut settings.paint,
            &[
                (ShapePaint::Outline, tr("shape-paint-outline")),
                (ShapePaint::Fill, tr("shape-paint-fill")),
                (ShapePaint::Both, tr("shape-paint-both")),
            ],
            3,
        );
    }
    ui.label(
        egui::RichText::new(tr("shape-hint"))
            .size(theme::SMALL)
            .color(theme::muted()),
    );
    if settings != canvas.tools().shape {
        canvas.tools_mut().shape = settings;
    }
    ui.add_space(theme::space::S);
}

/// The selection tool's mode and shapes, as 2.2.13's area select panel;
/// painting also shows how fills look.
fn selection_settings(ui: &mut egui::Ui, canvas: &mut Canvas) {
    widgets::panel_heading(ui, tr("lasso-mode"));
    let mut paints = canvas.tools().lasso_paints;
    widgets::segmented(
        ui,
        &mut paints,
        &[(false, tr("lasso-select")), (true, tr("lasso-paint"))],
        2,
    );
    let mut fill = canvas.tools().fill;
    // Pixel mode never blends edges.
    if paints && !canvas.session().pixel_mode() {
        widgets::check_row(
            ui,
            tr("fill-antialias"),
            tr("fill-antialias"),
            &mut fill.antialias,
        );
    }
    if paints != canvas.tools().lasso_paints || fill != canvas.tools().fill {
        let tools = canvas.tools_mut();
        tools.lasso_paints = paints;
        tools.fill = fill;
    }
    ui.add_space(theme::space::S);
    sampling_choice(ui, canvas);
    ui.add_space(theme::space::S);
    widgets::panel_heading(ui, tr("selection-shape"));
    let current = canvas.tools().selection_shape;
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
            canvas.tools_mut().selection_shape = kind;
        }
    }
}

/// What the wand and the bucket read and how they compare, as 2.2.13's auto
/// select and paint bucket panels; the bucket also sets how fills look.
/// The wand shows the comparison too, as it uses it.
fn fill_settings(ui: &mut egui::Ui, canvas: &mut Canvas, bucket: bool) {
    let mut fill: FillSettings = canvas.tools().fill;
    widgets::panel_heading(ui, tr("fill-reference"));
    for (reads, title, description) in [
        (Reads::Current, "reads-current", "reads-current-tip"),
        (Reads::Marked, "reads-marked", "reads-marked-tip"),
        (Reads::Visible, "reads-visible", "reads-visible-tip"),
    ] {
        if shape_option(ui, fill.reads == reads, tr(title), tr(description)).clicked() {
            fill.reads = reads;
        }
    }
    ui.add_space(theme::space::S);
    widgets::panel_heading(ui, tr("fill-comparison"));
    widgets::segmented(
        ui,
        &mut fill.by_colour,
        &[(false, tr("compare-alpha")), (true, tr("compare-colour"))],
        2,
    );
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
    if bucket && !canvas.session().pixel_mode() {
        ui.add_space(theme::space::XS);
        widgets::check_row(
            ui,
            tr("fill-antialias"),
            tr("fill-antialias"),
            &mut fill.antialias,
        );
    }
    if fill != canvas.tools().fill {
        canvas.tools_mut().fill = fill;
    }
}

/// How a transform resamples; always whole pixels in pixel mode.
fn sampling_choice(ui: &mut egui::Ui, canvas: &mut Canvas) {
    if canvas.session().pixel_mode() {
        return;
    }
    widgets::panel_heading(ui, tr("transform-method"));
    let current = canvas.tools().transform_sampling;
    for (sampling, title, description) in [
        (Sampling::Smooth, "sampling-smooth", "sampling-smooth-tip"),
        (Sampling::Nearest, "sampling-pixels", "sampling-pixels-tip"),
    ] {
        if shape_option(ui, current == sampling, tr(title), tr(description)).clicked() {
            canvas.set_transform_sampling(sampling);
        }
    }
}

/// A choice showing its name over what it does, both wrapped to the panel's
/// width, which the card grows to hold.
fn shape_option(
    ui: &mut egui::Ui,
    selected: bool,
    title: &str,
    description: &str,
) -> egui::Response {
    const SIDE: f32 = 10.0;
    let width = ui.available_width();
    let wrap = (width - 2.0 * SIDE).max(1.0);
    let layout = |text: &str, size: f32, color| {
        ui.painter().layout(
            text.to_owned(),
            egui::FontId::proportional(size),
            color,
            wrap,
        )
    };
    let title_text = layout(title, theme::BODY, theme::text());
    let description_text = layout(description, theme::SMALL, theme::muted());
    let below_title = 6.0 + title_text.size().y + 4.0;
    let height = (below_title + description_text.size().y + 5.0).max(44.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, title)
    });
    let painter = ui.painter();
    let (fill, edge) = match (selected, response.hovered()) {
        (true, _) => (theme::control(), theme::accent_ink()),
        (false, true) => (theme::hover(), theme::border()),
        (false, false) => (egui::Color32::TRANSPARENT, theme::border()),
    };
    painter.rect(
        rect,
        egui::CornerRadius::same(theme::radius::CONTROL),
        fill,
        egui::Stroke::new(1.0, edge),
        egui::StrokeKind::Inside,
    );
    let at = |y: f32| rect.left_top() + egui::vec2(SIDE, y);
    painter.galley(at(6.0), title_text, theme::text());
    painter.galley(at(below_title), description_text, theme::muted());
    if response.has_focus() {
        widgets::focus_ring(painter, rect, theme::radius::CONTROL);
    }
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
        transform_box(ui, canvas, &corners, |at| canvas.grip_at(at));
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

/// 2.2.13's brush ring in place of the pointer while a press would draw:
/// a dark line under a light one, dashed for erasing.
pub fn brush_cursor(ui: &mut egui::Ui, canvas: &Canvas) {
    let (pointer, space) =
        ui.input(|input| (input.pointer.hover_pos(), input.key_down(egui::Key::Space)));
    let over = pointer.filter(|pointer| {
        ui.max_rect().contains(*pointer)
            && ui
                .ctx()
                .layer_id_at(*pointer)
                .is_none_or(|layer| layer.order == egui::Order::Background)
    });
    let (Some(pointer), false) = (over, space) else {
        return;
    };
    let ppp = ui.ctx().pixels_per_point();
    let Some(cursor) =
        canvas.brush_cursor([f64::from(pointer.x * ppp), f64::from(pointer.y * ppp)])
    else {
        return;
    };
    let to_points = |[x, y]: [f64; 2]| egui::pos2(x as f32 / ppp, y as f32 / ppp);
    let (points, eraser) = match cursor {
        BrushCursor::Circle {
            centre,
            radius,
            eraser,
        } => {
            let centre = to_points(centre);
            let radius = radius as f32 / ppp;
            let steps = (radius * 1.5).clamp(16.0, 256.0) as usize;
            let points = (0..steps)
                .map(|step| {
                    let angle = step as f32 / steps as f32 * std::f32::consts::TAU;
                    centre + radius * egui::vec2(angle.cos(), angle.sin())
                })
                .collect::<Vec<_>>();
            (points, eraser)
        }
        BrushCursor::Pixels { outline, eraser } => {
            (outline.into_iter().map(to_points).collect(), eraser)
        }
    };
    ui.ctx().set_cursor_icon(egui::CursorIcon::None);
    let painter = ui.painter();
    let dark = egui::Color32::from_rgba_unmultiplied(20, 20, 20, 220);
    let light = egui::Color32::from_rgba_unmultiplied(250, 250, 250, 235);
    painter.add(egui::Shape::closed_line(
        points.clone(),
        egui::Stroke::new(3.0, dark),
    ));
    if eraser {
        let mut closed = points;
        closed.push(closed[0]);
        painter.extend(egui::Shape::dashed_line(
            &closed,
            egui::Stroke::new(1.0, light),
            4.0,
            3.0,
        ));
    } else {
        painter.add(egui::Shape::closed_line(
            points,
            egui::Stroke::new(1.0, light),
        ));
    }
}

/// A dashed frame around placed text and what to do with it, as 2.2.13
/// draws them.
pub fn text_overlay(ui: &mut egui::Ui, canvas: &Canvas) {
    let Some([left, top, right, bottom]) = canvas.text_box() else {
        return;
    };
    // 6 points outside the text, on the turned document.
    let pad = 6.0 * f64::from(ui.ctx().pixels_per_point()) / canvas.scale();
    let [a, b, c, d] = corners_on_screen(
        ui,
        canvas,
        [left - pad, top - pad, right + pad, bottom + pad],
    );
    let painter = ui.painter();
    let corners = [a, b, c, d, a];
    let rect = egui::Rect::from_points(&corners);
    for side in corners.windows(2) {
        painter.extend(egui::Shape::dashed_line(
            side,
            egui::Stroke::new(1.0, theme::accent_ink()),
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
        theme::muted(),
    );
}

/// As 2.2.13's status bar: turn left, the angle, turn right, reset and
/// mirror.
fn view_turn(ui: &mut egui::Ui, canvas: &mut Canvas, keys: &Keymap) {
    let named = |action: Action| {
        let name = tr(action.label());
        (
            actions::button_glyph(action),
            name,
            widgets::with_key(name, &keys.text(action)),
        )
    };
    let (glyph, name, tip) = named(Action::TurnLeft);
    if widgets::icon_button_tip(ui, glyph, 16.0, name, &tip, true).clicked() {
        canvas.turn_by(-crate::canvas::TURN_STEP);
    }
    let mut degrees = canvas.angle() as f32;
    let field = widgets::number(
        ui,
        &mut degrees,
        -180.0..=180.0,
        "°",
        1,
        64.0,
        tr("canvas-rotation"),
    )
    .on_hover_text(tr("canvas-rotation"));
    if field.changed() {
        canvas.turn_to(f64::from(degrees));
    }
    let (glyph, name, tip) = named(Action::TurnRight);
    if widgets::icon_button_tip(ui, glyph, 16.0, name, &tip, true).clicked() {
        canvas.turn_by(crate::canvas::TURN_STEP);
    }
    let (glyph, name, tip) = named(Action::ResetTurn);
    if widgets::icon_button_tip(ui, glyph, 16.0, name, &tip, true).clicked() {
        canvas.turn_to(0.0);
    }
    let (glyph, name, tip) = named(Action::Mirror);
    if widgets::icon_toggle(ui, glyph, 16.0, name, &tip, canvas.is_mirrored()).clicked() {
        canvas.set_mirrored(!canvas.is_mirrored());
    }
}

/// The pointer's place, zoom, turn and fit at the right of the status bar,
/// left to right.
fn view_controls(ui: &mut egui::Ui, canvas: &mut Canvas, pointer: Option<[f64; 2]>, keys: &Keymap) {
    let text = pointer.map_or_else(String::new, |[x, y]| {
        tr_with(
            "status-pointer",
            &args([("x", format!("{x:.0}")), ("y", format!("{y:.0}"))]),
        )
    });
    ui.add_sized(
        [150.0, 20.0],
        egui::Label::new(egui::RichText::new(text).color(theme::muted())),
    );
    let scale = canvas.scale();
    let (low, high) = (
        *ZOOM_RANGE.start() as f32 * 100.0,
        *ZOOM_RANGE.end() as f32 * 100.0,
    );
    // Logarithmic, so every step reads as the same change in size.
    let mut position = (scale.ln() as f32 - low.ln() + 100f32.ln()) / ((high / low).ln());
    let slider = widgets::slider(ui, &mut position, 0.0..=1.0, tr("canvas-zoom"), 96.0);
    if slider.changed() {
        let percent = low * (high / low).powf(position);
        canvas.zoom_to(f64::from(percent) / 100.0);
    }
    let mut percent = (canvas.scale() * 100.0) as f32;
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
    view_turn(ui, canvas, keys);
    let fit = widgets::text_icon_button(
        ui,
        actions::button_glyph(Action::Fit),
        tr("fit"),
        tr("fit-tip"),
    );
    if fit.clicked() {
        canvas.fit();
    }
}

/// Document pixels to points.
fn to_screen(ui: &egui::Ui, canvas: &Canvas, point: [f64; 2]) -> egui::Pos2 {
    let ppp = ui.ctx().pixels_per_point();
    let [x, y] = canvas.placement().to_target(point);
    egui::pos2(x as f32 / ppp, y as f32 / ppp)
}

/// The corners of document rectangle `[left, top, right, bottom]` on
/// screen, clockwise from the top left as the document is turned.
fn corners_on_screen(ui: &egui::Ui, canvas: &Canvas, rect: [f64; 4]) -> [egui::Pos2; 4] {
    let [left, top, right, bottom] = rect;
    [[left, top], [right, top], [right, bottom], [left, bottom]]
        .map(|corner| to_screen(ui, canvas, corner))
}

/// The symmetry ruler's lines and centre, when it is on.
pub fn symmetry_overlay(ui: &mut egui::Ui, canvas: &Canvas) {
    let lines = canvas.symmetry_lines();
    let Some(&[centre, _]) = lines.first() else {
        return;
    };
    let painter = ui.painter();
    // The accent, edged with its text colour to show on light and dark
    // drawings alike.
    let edge = theme::accent_text().gamma_multiply(0.6);
    for (stroke, dot) in [
        (egui::Stroke::new(3.0, edge), 4.0),
        (egui::Stroke::new(1.0, theme::accent()), 3.0),
    ] {
        for [from, to] in &lines {
            painter.line_segment(
                [to_screen(ui, canvas, *from), to_screen(ui, canvas, *to)],
                stroke,
            );
        }
        painter.circle_filled(to_screen(ui, canvas, centre), dot, stroke.color);
    }
    if canvas.tool() == Tool::Symmetry {
        let at = to_screen(ui, canvas, centre);
        painter.circle_stroke(at, 8.0, egui::Stroke::new(3.0, egui::Color32::WHITE));
        painter.circle_stroke(at, 8.0, egui::Stroke::new(1.0, theme::accent_text()));
    }
}

/// The pointer over the canvas for what a drag with the ruler tool does.
pub fn ruler_cursor(ui: &mut egui::Ui, canvas: &Canvas) {
    let ppp = ui.ctx().pixels_per_point();
    let Some(pointer) = ui
        .input(|input| input.pointer.hover_pos())
        .filter(|pointer| ui.max_rect().contains(*pointer))
    else {
        return;
    };
    let icon = match canvas.ruler_grip_at([f64::from(pointer.x * ppp), f64::from(pointer.y * ppp)])
    {
        Some(RulerGrip::Centre) => egui::CursorIcon::Move,
        Some(RulerGrip::Line) => egui::CursorIcon::Crosshair,
        None => return,
    };
    ui.ctx().set_cursor_icon(icon);
}

/// The move tool's box around the current layer as the frame shows it.
pub fn pose_overlay(ui: &mut egui::Ui, canvas: &Canvas) {
    if let Some(corners) = canvas.pose_box() {
        transform_box(ui, canvas, &corners, |at| canvas.pose_grip_at(at));
    }
    if let Some(pivot) = canvas.pivot_shown() {
        let at = to_screen(ui, canvas, pivot);
        let painter = ui.painter();
        for (stroke, reach) in [
            (egui::Stroke::new(3.0, egui::Color32::WHITE), 7.0),
            (egui::Stroke::new(1.0, theme::accent_text()), 6.0),
        ] {
            painter.circle_stroke(at, 4.0, stroke);
            painter.line_segment(
                [at - egui::vec2(reach, 0.0), at + egui::vec2(reach, 0.0)],
                stroke,
            );
            painter.line_segment(
                [at - egui::vec2(0.0, reach), at + egui::vec2(0.0, reach)],
                stroke,
            );
        }
    }
}

/// The box's sides and handles, and the pointer shape for what a drag from
/// under the pointer would do, as `grip_at` says for a client point.
fn transform_box(
    ui: &mut egui::Ui,
    canvas: &Canvas,
    corners: &[[f64; 2]; 8],
    grip_at: impl Fn([f64; 2]) -> Option<Grip>,
) {
    let points = corners.map(|corner| to_screen(ui, canvas, corner));
    let painter = ui.painter();
    painter.add(egui::Shape::closed_line(
        points[..4].to_vec(),
        egui::Stroke::new(1.0, theme::accent_ink()),
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
    let icon = match grip_at([f64::from(pointer.x * ppp), f64::from(pointer.y * ppp)]) {
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
    const MARGIN: f32 = theme::space::M;
    const GAP: f32 = theme::space::S;
    let session = canvas.session();
    if session.selection().is_none() || session.lasso().is_some() || canvas.is_dragging() {
        return;
    }
    let pending = session.pending().map(|pending| pending.keep_source);
    let Some([left, top, right, bottom]) = canvas.selection_bounds() else {
        return;
    };
    let area = ui.max_rect();
    // Below the selection as it shows on screen, or above it.
    let bounds =
        egui::Rect::from_points(&corners_on_screen(ui, canvas, [left, top, right, bottom]));
    let (low, high) = (bounds.left_bottom(), bounds.right_top());
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
                .fill(theme::panel())
                .stroke(egui::Stroke::new(1.0, theme::border()))
                .corner_radius(egui::CornerRadius::same(theme::radius::SURFACE))
                .inner_margin(egui::Margin::same(theme::space::XS as i8))
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
    // `key` is the action whose key the tip names.
    let keyed = |ui: &mut egui::Ui, action, name, tip, key| {
        let tip = widgets::with_key(tr(tip), &keys.text(key));
        let glyph = actions::button_glyph(action);
        widgets::icon_button_tip(ui, glyph, 18.0, tr(name), &tip, true).clicked()
    };
    let button = |ui: &mut egui::Ui, action, name, tip| keyed(ui, action, name, tip, action);
    if pending.is_none()
        && button(
            ui,
            Action::Transform,
            "transform-selection",
            "transform-selection-tip",
        )
    {
        canvas.begin_transform();
    }
    if button(
        ui,
        Action::FlipHorizontal,
        "flip-horizontal",
        "flip-horizontal",
    ) {
        canvas.flip(true);
    }
    if button(ui, Action::FlipVertical, "flip-vertical", "flip-vertical") {
        canvas.flip(false);
    }
    if pending.is_some() {
        ui.separator();
        if button(
            ui,
            Action::ApplyTransform,
            "transform-apply",
            "transform-apply-tip",
        ) {
            canvas.apply_transform();
        }
        if keyed(
            ui,
            Action::CancelTransform,
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
            Action::Restyle,
            "restyle-selected",
            "restyle-selected-tip",
        );
    if button(
        ui,
        Action::DeleteSelected,
        "delete-selected",
        "delete-selected",
    ) {
        canvas.delete_selected();
    }
    ui.separator();
    if button(ui, Action::Deselect, "edit-deselect", "edit-deselect") {
        canvas.edit_settled(Session::deselect);
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
            widgets::choice(ui, &mut panels.wobble_layer, false, scope(false));
            ui.add_enabled_ui(layer_wobble.is_some(), |ui| {
                widgets::choice(ui, &mut panels.wobble_layer, true, scope(true));
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
        let follow = widgets::button(
            ui,
            tr("wobble-follow"),
            widgets::Emphasis::Secondary,
            overridden,
        )
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
    widgets::panel_heading(ui, tr("wobble"));
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
    widgets::form_row(ui, tr("motion-style"), |ui| {
        egui::ComboBox::from_id_salt("motion-style")
            .width(ui.available_width())
            .selected_text(style_name(motion.style))
            .show_ui(ui, |ui| {
                for style in [
                    MotionStyle::Classic,
                    MotionStyle::Smooth,
                    MotionStyle::Stepped,
                ] {
                    widgets::choice(ui, &mut motion.style, style, style_name(style));
                }
            })
            .response
            .widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, tr("motion-style"))
            });
    });
    let restyled = motion.style != before;
    // Poses, detail, linking and randomness move Smooth and Stepped only, so
    // Classic leaves them out, as a whole line leaves out the break's rows.
    if motion.style != MotionStyle::Classic {
        let mut poses = motion.poses.min(frames) as f32;
        let response = widgets::form_row(ui, tr("motion-poses"), |ui| {
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
        let response = widgets::form_row(ui, tr("motion-detail"), |ui| {
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
    }
    if track(widgets::checkbox(
        ui,
        &mut motion.broken,
        tr("motion-broken"),
    )) {
        changed = true;
    }
    if motion.broken {
        changed |= track(percent_row(
            ui,
            tr("motion-break-amount"),
            &mut motion.break_amount,
        ));
        let range = limits::BREAK_RANGE;
        let mut length = motion.break_range;
        let response = widgets::form_row(ui, tr("motion-break-range"), |ui| {
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
    }

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

/// A share from 0 to 1 as a slider and a field of whole percent.
fn percent_row(ui: &mut egui::Ui, label: &str, share: &mut f32) -> egui::Response {
    let mut percent = *share * 100.0;
    let response = widgets::form_row(ui, label, |ui| {
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
        egui::Stroke::new(2.0, theme::accent_ink()),
    ));
    response
}

pub fn color(ui: &mut egui::Ui, canvas: &mut Canvas, panels: &mut Panels) {
    panels
        .color
        .show(ui, canvas, &mut panels.layout.color_field);
}

pub fn color_history(ui: &mut egui::Ui, canvas: &mut Canvas) {
    history::show(ui, canvas);
}

pub fn layers(ui: &mut egui::Ui, canvas: &mut Canvas, panels: &mut Panels) {
    let Panels {
        layers, refusal, ..
    } = panels;
    layers.show(ui, canvas, refusal);
}

/// The animation bar: play, the frame and frame count, the animation
/// buttons, the scrubber and the playback speed, over the timeline's rows.
pub fn animation_bar(ui: &mut egui::Ui, canvas: &mut Canvas, panels: &mut Panels) {
    // With the wobble animation off in the settings, as 2.2.13's timeline.
    ui.add_enabled_ui(canvas.animation_allowed(), |ui| {
        let track = animation_controls(ui, canvas, panels);
        if !canvas.is_playing() && panels.layout.animation_tools {
            ui.add_space(theme::space::XS);
            timeline::rows(ui, canvas, panels, track);
        }
    });
}

/// The bar's controls; returns the scrubber's span.
fn animation_controls(ui: &mut egui::Ui, canvas: &mut Canvas, panels: &mut Panels) -> egui::Rangef {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let playing = canvas.is_playing();
        let (rect, play) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::click());
        let fill = match (playing, play.hovered()) {
            (true, true) => theme::accent_pressed(),
            (true, false) => theme::accent(),
            (false, true) => theme::hover(),
            (false, false) => theme::control(),
        };
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(theme::radius::CONTROL), fill);
        let (glyph, ink) = if playing {
            (Glyph::Pause, theme::accent_text())
        } else {
            (actions::button_glyph(Action::Animate), theme::text())
        };
        icons::paint(
            ui.painter(),
            egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(20.0)),
            glyph,
            ink,
            0.0,
        );
        if play.has_focus() {
            widgets::focus_ring(ui.painter(), rect, theme::radius::CONTROL);
        }
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
            canvas.edit_settled(|session| session.set_frame(frame as i64 - 1));
        }
        ui.label(egui::RichText::new("/").color(theme::muted()));
        let (mut count, mut speed) = panels.animation.unwrap_or((frames as f32, fps));
        let (low, high) = (*limits::FRAMES.start() as f32, *limits::FRAMES.end() as f32);
        let count_field =
            widgets::number(ui, &mut count, low..=high, "", 0, 46.0, tr("frame-count"))
                .on_hover_text(tr("frame-count"));

        if panels.layout.animation_tools {
            animation_buttons(ui, canvas, panels);
        }
        let right = 46.0 + 30.0 + 90.0 + 30.0;
        let width = (ui.available_width() - right).max(160.0);
        let current = frame_in_cycle(canvas.session().frame(), frames);
        let (chosen, track) = scrubber(ui, current, frames, canvas.is_playing(), width);
        if let Some(chosen) = chosen {
            if canvas.is_playing() {
                canvas.toggle_playback();
            }
            canvas.edit_settled(|session| session.set_frame(i64::from(chosen)));
        }

        ui.label(
            egui::RichText::new(tr("fps"))
                .size(theme::SMALL)
                .color(theme::muted()),
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
        let glyph = actions::button_glyph(Action::AnimationBar);
        let hide = widgets::text_icon_button(ui, glyph, tr("bar-hide"), &tip);
        if hide.clicked() {
            panels.layout.animation_bar = false;
        }
        track
    })
    .inner
}

/// Onion skin, the key on this frame, a new animation folder and a new cel.
fn animation_buttons(ui: &mut egui::Ui, canvas: &mut Canvas, panels: &mut Panels) {
    let keys = &panels.keys;
    let onion = canvas.onion_skin();
    let tip = widgets::with_key(tr("onion-skin-tip"), &keys.text(Action::OnionSkin));
    let glyph = actions::button_glyph(Action::OnionSkin);
    let response = widgets::icon_button_tip(ui, glyph, 20.0, tr("onion-skin"), &tip, true);
    egui::Popup::context_menu(&response).show(|ui| {
        ui.set_min_width(240.0);
        ui.set_max_width(280.0);
        widgets::panel_heading(ui, tr("onion-settings"));
        let settings = &mut panels.layout.onion;
        let frames = OnionSettings::FRAMES;
        let frames = *frames.start() as f32..=*frames.end() as f32;
        for (count, key) in [
            (&mut settings.before, "onion-before"),
            (&mut settings.after, "onion-after"),
        ] {
            let mut value = *count as f32;
            slider_row(ui, tr(key), tr(key), &mut value, frames.clone(), "", 0);
            *count = value.round() as u32;
        }
        let opacity = OnionSettings::OPACITY;
        let mut value = settings.opacity as f32;
        slider_row(
            ui,
            tr("onion-opacity"),
            tr("onion-opacity"),
            &mut value,
            *opacity.start() as f32..=*opacity.end() as f32,
            "%",
            0,
        );
        settings.opacity = value.round() as u32;
        *settings = settings.clamped();
    });
    if onion {
        ui.painter().rect_stroke(
            response.rect,
            egui::CornerRadius::same(theme::radius::CONTROL),
            egui::Stroke::new(1.0, theme::accent_ink()),
            egui::StrokeKind::Inside,
        );
    }
    if response.clicked() {
        canvas.set_onion_skin(!onion);
    }
    let keyed = canvas
        .session()
        .key_here(canvas.session().current_layer())
        .is_some();
    let text = if keyed {
        tr("key-remove")
    } else {
        tr("key-add")
    };
    let tip = widgets::with_key(text, &keys.text(Action::ToggleKey));
    let glyph = actions::button_glyph(Action::ToggleKey);
    if widgets::icon_button_tip(ui, glyph, 20.0, text, &tip, true).clicked() {
        canvas.toggle_key();
    }
    let tip = widgets::with_key(
        tr("animation-folder-make"),
        &keys.text(Action::AnimationFolder),
    );
    let folder = widgets::icon_button_tip(
        ui,
        actions::button_glyph(Action::AnimationFolder),
        20.0,
        tr("animation-folder-make"),
        &tip,
        canvas.session().pending().is_none(),
    );
    if folder.clicked() {
        let result = canvas.edit(Session::make_animation_folder);
        panels.report(result);
    }
    let in_folder = canvas.session().folder().is_some();
    let tip = widgets::with_key(tr("cel-new"), &panels.keys.text(Action::NewCel));
    let glyph = actions::button_glyph(Action::NewCel);
    if widgets::icon_button_tip(ui, glyph, 20.0, tr("cel-new"), &tip, in_folder).clicked() {
        let result = canvas.edit(Session::new_cel);
        panels.report(result);
    }
}

/// One slot per frame along a track. Returns the frame clicked or dragged
/// to.
fn scrubber(
    ui: &mut egui::Ui,
    current: u32,
    frames: u32,
    playing: bool,
    width: f32,
) -> (Option<u32>, egui::Rangef) {
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
        egui::CornerRadius::same(theme::radius::CONTROL),
        theme::status(),
        egui::Stroke::new(
            1.0,
            if focused {
                theme::accent_ink()
            } else {
                theme::border()
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
            egui::CornerRadius::same(theme::radius::SMALL),
            theme::control(),
        );
    }
    for index in 0..frames {
        painter.circle_filled(slot_rect(index).center(), 1.4, theme::disabled());
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
        painter.rect_filled(
            head,
            egui::CornerRadius::same(theme::radius::SMALL),
            theme::accent(),
        );
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
    (chosen.filter(|&frame| frame != current), track.x_range())
}

/// The status bar: the last message on the left; the pointer, zoom and fit
/// on the right.
/// Returns whether the export under way, `export` naming it, is to be
/// cancelled.
pub fn status_bar(
    ui: &mut egui::Ui,
    canvas: &mut Canvas,
    message: Option<(&str, bool)>,
    lasting: Option<&str>,
    export: Option<&str>,
    pointer: Option<[f64; 2]>,
    keys: &Keymap,
) -> bool {
    let mut cancel = false;
    ui.horizontal(|ui| {
        if let Some(lasting) = lasting {
            ui.colored_label(theme::accent_ink(), lasting);
            ui.separator();
        }
        if let Some(export) = export {
            // No spinner: it asks for a frame every frame, which kept the
            // render thread drawing throughout an export.
            ui.label(export);
            cancel = widgets::text_icon_button(
                ui,
                Glyph::Cancel,
                tr("export-cancel"),
                tr("export-cancel"),
            )
            .clicked();
            ui.separator();
        }
        match message {
            Some((text, true)) => ui.colored_label(theme::accent_ink(), text),
            Some((text, false)) => ui.colored_label(theme::muted(), text),
            None => ui.colored_label(theme::muted(), tr("status-ready")),
        };
        // The view controls sit at the right end but are made left to right,
        // so that Tab goes through them in the order they are seen; their
        // width is the one measured last frame.
        let width_id = ui.id().with("view controls width");
        let width = ui
            .data(|data| data.get_temp::<f32>(width_id))
            .unwrap_or(0.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let size = egui::vec2(width, ui.available_height());
            let layout = egui::Layout::left_to_right(egui::Align::Center);
            let used = ui
                .allocate_ui_with_layout(size, layout, |ui| {
                    view_controls(ui, canvas, pointer, keys)
                })
                .response
                .rect
                .width();
            if (used - width).abs() > 0.5 {
                ui.data_mut(|data| data.insert_temp(width_id, used));
                ui.ctx().request_discard("status bar width");
            }
        });
    });
    cancel
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the text in `shape` shows.
    fn text_bounds(shape: &egui::Shape, found: &mut Vec<egui::Rect>) {
        match shape {
            egui::Shape::Text(text) => found.push(text.visual_bounding_rect()),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    text_bounds(shape, found);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn an_options_text_wraps_inside_its_card() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut card = egui::Rect::NOTHING;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.allocate_ui(egui::vec2(180.0, 400.0), |ui| {
                card = shape_option(
                    ui,
                    false,
                    "Pixel-based",
                    "Skip interpolation without removing existing transparency; edges may look stepped",
                )
                .rect;
            });
        });
        output.textures_delta.clear();
        let mut found = Vec::new();
        for clipped in &output.shapes {
            text_bounds(&clipped.shape, &mut found);
        }
        assert_eq!(found.len(), 2, "the name and what it does");
        assert!(card.width() <= 180.0, "{card:?}");
        for text in found {
            assert!(card.contains_rect(text), "{text:?} outside {card:?}");
        }
    }
}
