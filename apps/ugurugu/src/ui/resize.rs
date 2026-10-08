// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The canvas size and image size dialogs, as 2.2.13's: a new size,
//! absolute or relative, with a reference point and offset for the canvas,
//! and a uniform scale and kept aspect ratio for the image. The image
//! dialog also chooses how pixels are resampled.

use ugu_core::document::limits;
use ugu_core::ops::Sampling;

use crate::canvas::Canvas;
use crate::i18n::{tr, tr_with};
use crate::theme;
use crate::widgets;

use super::{Panels, args};

/// The dialog open, if any.
pub enum Dialog {
    Canvas(CanvasSize),
    Image(ImageSize),
}

const EDGE: std::ops::RangeInclusive<u32> = limits::CANVAS_EDGE;

fn clamp_edge(edge: i64) -> u32 {
    edge.clamp(i64::from(*EDGE.start()), i64::from(*EDGE.end())) as u32
}

/// Where the reference point puts the artwork along one axis when the
/// canvas grows by `delta`: 0 start, 1 middle, 2 end. The middle rounds
/// toward zero, as 2.2.13 does.
fn aligned(delta: i32, place: usize) -> i32 {
    match place {
        0 => 0,
        1 => delta / 2,
        _ => delta,
    }
}

/// A new canvas size and where the artwork lands on it.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasSize {
    pub current: [u32; 2],
    pub size: [u32; 2],
    /// Whether the size is shown as a change from the current one.
    pub relative: bool,
    /// The reference point, row-major from the top left; `None` when the
    /// offset was typed and matches none.
    pub anchor: Option<usize>,
    /// Where the artwork's top left lands on the new canvas.
    pub offset: [i32; 2],
}

impl CanvasSize {
    pub fn new(current: [u32; 2]) -> Self {
        Self {
            current,
            size: current,
            relative: false,
            anchor: Some(4),
            offset: [0, 0],
        }
    }

    fn anchored(&self, anchor: usize) -> [i32; 2] {
        let delta = |axis: usize| self.size[axis] as i32 - self.current[axis] as i32;
        [aligned(delta(0), anchor % 3), aligned(delta(1), anchor / 3)]
    }

    pub fn set_size(&mut self, size: [u32; 2]) {
        self.size = size.map(|edge| clamp_edge(i64::from(edge)));
        if let Some(anchor) = self.anchor {
            self.offset = self.anchored(anchor);
        }
    }

    pub fn set_anchor(&mut self, anchor: usize) {
        self.anchor = Some(anchor);
        self.offset = self.anchored(anchor);
    }

    /// Takes a typed offset; the reference point becomes the one that
    /// gives it, keeping the current one when it still does.
    pub fn set_offset(&mut self, offset: [i32; 2]) {
        let reach = *EDGE.end() as i32;
        self.offset = offset.map(|value| value.clamp(-reach, reach));
        let matches = |anchor: &usize| self.anchored(*anchor) == self.offset;
        self.anchor = self.anchor.filter(matches).or_else(|| (0..9).find(matches));
    }

    /// Whether applying changes anything: a new size, or the same size
    /// with the artwork moved.
    pub fn changes(&self) -> bool {
        self.size != self.current || self.offset != [0, 0]
    }

    fn hint(&self) -> &'static str {
        let [x, y] = self.offset;
        let [width, height] = self.current.map(|edge| edge as i32);
        let [new_width, new_height] = self.size.map(|edge| edge as i32);
        if x >= new_width || y >= new_height || x + width <= 0 || y + height <= 0 {
            "canvas-size-outside"
        } else if x < 0 || y < 0 || x + width > new_width || y + height > new_height {
            "canvas-size-clipped"
        } else if self.changes() {
            "canvas-size-expanded"
        } else {
            "canvas-size-unchanged"
        }
    }
}

/// A new image size, its aspect ratio kept or not.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageSize {
    pub current: [u32; 2],
    pub size: [u32; 2],
    pub keep_aspect: bool,
    pub sampling: Sampling,
}

impl ImageSize {
    pub fn new(current: [u32; 2]) -> Self {
        let mut dialog = Self {
            current,
            size: current,
            keep_aspect: true,
            sampling: Sampling::Smooth,
        };
        dialog.keep_aspect = dialog.aspect_resizable();
        dialog
    }

    fn ratio(&self) -> f64 {
        f64::from(self.current[0]) / f64::from(self.current[1])
    }

    /// The widths and heights that keep the aspect ratio within the limits.
    fn aspect_ranges(&self) -> [[u32; 2]; 2] {
        let ratio = self.ratio();
        let [low, high] = [*EDGE.start(), *EDGE.end()].map(f64::from);
        [
            [(low * ratio).ceil(), (high * ratio).floor()],
            [(low / ratio).ceil(), (high / ratio).floor()],
        ]
        .map(|range| range.map(|edge| clamp_edge(edge as i64)))
    }

    /// Whether any other size keeps the aspect ratio.
    pub fn aspect_resizable(&self) -> bool {
        let [width, height] = self.aspect_ranges();
        width[0] != width[1] || height[0] != height[1]
    }

    pub fn ranges(&self) -> [[u32; 2]; 2] {
        if self.keep_aspect {
            self.aspect_ranges()
        } else {
            [[*EDGE.start(), *EDGE.end()]; 2]
        }
    }

    pub fn set_width(&mut self, width: u32) {
        let [low, high] = self.ranges()[0];
        self.size[0] = width.clamp(low, high);
        if self.keep_aspect {
            self.size[1] = clamp_edge((f64::from(self.size[0]) / self.ratio()).round() as i64);
        }
    }

    pub fn set_height(&mut self, height: u32) {
        let [low, high] = self.ranges()[1];
        self.size[1] = height.clamp(low, high);
        if self.keep_aspect {
            self.size[0] = clamp_edge((f64::from(self.size[1]) * self.ratio()).round() as i64);
        }
    }

    /// The scale both edges can take within the limits, in percent.
    pub fn percent_range(&self) -> [f64; 2] {
        let edges = self.current.map(f64::from);
        let [low, high] = [*EDGE.start(), *EDGE.end()].map(f64::from);
        [
            (100.0 * low / edges[0]).max(100.0 * low / edges[1]),
            (100.0 * high / edges[0]).min(100.0 * high / edges[1]),
        ]
    }

    /// Scales both edges by `percent`.
    pub fn set_percent(&mut self, percent: f64) {
        let [low, high] = self.percent_range();
        let factor = percent.clamp(low, high) / 100.0;
        self.size = self
            .current
            .map(|edge| clamp_edge((f64::from(edge) * factor).round() as i64));
    }

    /// Horizontal and vertical scale in percent.
    pub fn scales(&self) -> [f64; 2] {
        [0, 1].map(|axis| 100.0 * f64::from(self.size[axis]) / f64::from(self.current[axis]))
    }

    /// The uniform scale shown: the horizontal one with the aspect kept,
    /// otherwise the mean of both.
    pub fn percent(&self) -> f64 {
        let [horizontal, vertical] = self.scales();
        if self.keep_aspect {
            horizontal
        } else {
            (horizontal * vertical).sqrt()
        }
    }

    /// Keeping the aspect again takes the height from the width.
    pub fn set_keep_aspect(&mut self, keep: bool) {
        self.keep_aspect = keep && self.aspect_resizable();
        if self.keep_aspect {
            self.set_width(self.size[0]);
        }
    }

    pub fn distorted(&self) -> bool {
        let [horizontal, vertical] = self.scales();
        !self.keep_aspect && (horizontal - vertical).abs() > 1e-9 * horizontal.max(vertical)
    }
}

/// Shows the open dialog and applies it when confirmed.
pub fn show(ctx: &egui::Context, canvas: &mut Canvas, panels: &mut Panels) {
    let Some(dialog) = panels.resize.as_mut() else {
        return;
    };
    let mut close = false;
    let mut apply = false;
    let modal = egui::Modal::new(egui::Id::new("resize dialog")).show(ctx, |ui| {
        ui.set_width(420.0);
        let can_apply = match dialog {
            Dialog::Canvas(dialog) => canvas_form(ui, dialog),
            Dialog::Image(dialog) => image_form(ui, dialog),
        };
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("dialog-cancel")).clicked() {
                    close = true;
                }
                let ok = ui.add_enabled(can_apply, egui::Button::new(tr("dialog-ok")));
                let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
                if ok.clicked() || (enter && can_apply) {
                    apply = true;
                }
            });
        });
    });
    if apply {
        let result = match &*dialog {
            Dialog::Canvas(dialog) => {
                let (offset, size) = (dialog.offset, dialog.size);
                canvas.edit(|session| session.crop_canvas(offset, size))
            }
            Dialog::Image(dialog) => {
                let (size, sampling) = (dialog.size, dialog.sampling);
                canvas.edit(|session| session.resample_image(size, sampling))
            }
        };
        panels.report(result);
        close = true;
    }
    if close || modal.should_close() {
        panels.resize = None;
    }
}

pub(super) fn heading(ui: &mut egui::Ui, title: &str, description: &str) {
    ui.heading(title);
    ui.label(egui::RichText::new(description).color(theme::MUTED));
    ui.add_space(6.0);
}

/// A whole number field; `signed` shows a plus on positive values.
fn whole(
    ui: &mut egui::Ui,
    value: &mut i64,
    range: std::ops::RangeInclusive<i64>,
    signed: bool,
    name: &str,
) -> egui::Response {
    let response = ui.add_sized(
        [96.0, 26.0],
        egui::DragValue::new(value)
            .range(range)
            .speed(1.0)
            .suffix(" px")
            .custom_formatter(move |value, _| {
                if signed && value > 0.0 {
                    format!("+{value}")
                } else {
                    format!("{value}")
                }
            }),
    );
    let shown = *value as f64;
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::drag_value(true, shown);
        info.label = Some(name.to_owned());
        info
    });
    response
}

fn summary(ui: &mut egui::Ui, from: [u32; 2], to: [u32; 2]) {
    ui.label(tr_with(
        "size-change",
        &args([
            ("width", from[0].to_string()),
            ("height", from[1].to_string()),
            ("new_width", to[0].to_string()),
            ("new_height", to[1].to_string()),
        ]),
    ));
}

/// Draws `rectangles` (left, top, width, height, highlighted) scaled to fit
/// a preview box together.
fn preview(ui: &mut egui::Ui, name: &str, rectangles: &[([f64; 4], bool)]) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 150.0),
        egui::Sense::hover(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, name));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, egui::CornerRadius::same(4), theme::BASE);
    let low = [0, 1].map(|axis| {
        rectangles
            .iter()
            .map(|(r, _)| r[axis])
            .fold(f64::MAX, f64::min)
    });
    let high = [0, 1].map(|axis| {
        rectangles
            .iter()
            .map(|(r, _)| r[axis] + r[axis + 2])
            .fold(f64::MIN, f64::max)
    });
    let room = rect.shrink(12.0);
    let span = [high[0] - low[0], high[1] - low[1]];
    let scale = (f64::from(room.width()) / span[0]).min(f64::from(room.height()) / span[1]);
    let origin =
        room.center() - egui::vec2((span[0] * scale) as f32, (span[1] * scale) as f32) / 2.0;
    for &([left, top, width, height], highlighted) in rectangles {
        let min = origin
            + egui::vec2(
                ((left - low[0]) * scale) as f32,
                ((top - low[1]) * scale) as f32,
            );
        let shape = egui::Rect::from_min_size(
            min,
            egui::vec2(
                (width * scale).max(1.0) as f32,
                (height * scale).max(1.0) as f32,
            ),
        );
        if highlighted {
            painter.rect(
                shape,
                egui::CornerRadius::ZERO,
                theme::ACCENT.gamma_multiply(0.25),
                egui::Stroke::new(1.5, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
        } else {
            painter.rect(
                shape,
                egui::CornerRadius::ZERO,
                theme::CONTROL,
                egui::Stroke::new(1.0, theme::BORDER),
                egui::StrokeKind::Inside,
            );
        }
    }
}

const ANCHORS: [&str; 9] = [
    "anchor-top-left",
    "anchor-top-center",
    "anchor-top-right",
    "anchor-middle-left",
    "anchor-center",
    "anchor-middle-right",
    "anchor-bottom-left",
    "anchor-bottom-center",
    "anchor-bottom-right",
];

/// The canvas size form; returns whether it can be applied.
fn canvas_form(ui: &mut egui::Ui, dialog: &mut CanvasSize) -> bool {
    heading(ui, tr("canvas-size-title"), tr("canvas-size-description"));
    let [width, height] = dialog.current.map(f64::from);
    let [new_width, new_height] = dialog.size.map(f64::from);
    let [x, y] = dialog.offset.map(f64::from);
    preview(
        ui,
        tr("canvas-size-preview"),
        &[
            ([0.0, 0.0, new_width, new_height], false),
            ([x, y, width, height], true),
        ],
    );
    summary(ui, dialog.current, dialog.size);
    ui.label(tr_with(
        "canvas-size-offset-summary",
        &args([("x", x.to_string()), ("y", y.to_string())]),
    ));
    ui.label(egui::RichText::new(tr(dialog.hint())).color(theme::MUTED));
    ui.add_space(8.0);

    widgets::field_label(ui, tr("canvas-size-section"));
    let mut relative = dialog.relative;
    if widgets::checkbox(ui, &mut relative, tr("canvas-size-relative"))
        .on_hover_text(tr("canvas-size-relative-tip"))
        .changed()
    {
        dialog.relative = relative;
    }
    let mut size = dialog.size;
    for (axis, [label, change, name, change_name]) in [
        [
            "size-width",
            "canvas-size-width-change",
            "canvas-size-width",
            "canvas-size-width-change-name",
        ],
        [
            "size-height",
            "canvas-size-height-change",
            "canvas-size-height",
            "canvas-size-height-change-name",
        ],
    ]
    .into_iter()
    .enumerate()
    {
        let current = i64::from(dialog.current[axis]);
        let [low, high] = [*EDGE.start(), *EDGE.end()].map(i64::from);
        if dialog.relative {
            let mut delta = i64::from(size[axis]) - current;
            super::form_row(ui, tr(change), |ui| {
                whole(
                    ui,
                    &mut delta,
                    low - current..=high - current,
                    true,
                    tr(change_name),
                )
            });
            size[axis] = clamp_edge(current + delta);
        } else {
            let mut edge = i64::from(size[axis]);
            super::form_row(ui, tr(label), |ui| {
                whole(ui, &mut edge, low..=high, false, tr(name))
            });
            size[axis] = clamp_edge(edge);
        }
    }
    if size != dialog.size {
        dialog.set_size(size);
    }
    ui.add_space(8.0);

    widgets::field_label(ui, tr("canvas-size-anchor"));
    egui::Grid::new("anchor")
        .spacing([4.0, 4.0])
        .show(ui, |ui| {
            for (index, &key) in ANCHORS.iter().enumerate() {
                let chosen = dialog.anchor == Some(index);
                let button = egui::Button::new("")
                    .min_size(egui::vec2(28.0, 28.0))
                    .fill(if chosen { theme::ACCENT } else { theme::BASE });
                let response = ui.add(button).on_hover_text(tr(key));
                response.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, chosen, tr(key))
                });
                if response.clicked() {
                    dialog.set_anchor(index);
                }
                if index % 3 == 2 {
                    ui.end_row();
                }
            }
        });
    let mut offset = dialog.offset.map(i64::from);
    let reach = i64::from(*EDGE.end());
    for (axis, label, name) in [
        (0, "canvas-size-offset-x", "canvas-size-offset-x-name"),
        (1, "canvas-size-offset-y", "canvas-size-offset-y-name"),
    ] {
        super::form_row(ui, tr(label), |ui| {
            whole(ui, &mut offset[axis], -reach..=reach, true, tr(name))
        });
    }
    let offset = offset.map(|value| value as i32);
    if offset != dialog.offset {
        dialog.set_offset(offset);
    }
    dialog.changes()
}

/// The image size form; returns whether it can be applied.
fn image_form(ui: &mut egui::Ui, dialog: &mut ImageSize) -> bool {
    heading(ui, tr("image-size-title"), tr("image-size-description"));
    let [width, height] = dialog.current.map(f64::from);
    let [new_width, new_height] = dialog.size.map(f64::from);
    preview(
        ui,
        tr("image-size-preview"),
        &[
            ([0.0, 0.0, width, height], false),
            ([0.0, 0.0, new_width, new_height], true),
        ],
    );
    summary(ui, dialog.current, dialog.size);
    let [horizontal, vertical] = dialog.scales();
    ui.label(tr_with(
        "image-size-scales",
        &args([
            ("horizontal", format!("{horizontal:.1}")),
            ("vertical", format!("{vertical:.1}")),
        ]),
    ));
    if dialog.distorted() {
        ui.label(egui::RichText::new(tr("image-size-distorted")).color(theme::MUTED));
    }
    ui.add_space(8.0);

    widgets::field_label(ui, tr("image-size-section"));
    let ranges = dialog.ranges();
    for (axis, label, name) in [
        (0, "size-width", "image-size-width"),
        (1, "size-height", "image-size-height"),
    ] {
        let mut edge = i64::from(dialog.size[axis]);
        let [low, high] = ranges[axis].map(i64::from);
        super::form_row(ui, tr(label), |ui| {
            whole(ui, &mut edge, low..=high, false, tr(name))
        });
        let edge = clamp_edge(edge);
        if edge != dialog.size[axis] {
            if axis == 0 {
                dialog.set_width(edge);
            } else {
                dialog.set_height(edge);
            }
        }
    }
    let mut percent = dialog.percent();
    let [low, high] = dialog.percent_range();
    let response = super::form_row(ui, tr("image-size-scale"), |ui| {
        let response = ui.add_sized(
            [96.0, 26.0],
            egui::DragValue::new(&mut percent)
                .range(low..=high)
                .speed(1.0)
                .max_decimals(1)
                .suffix(" %"),
        );
        let shown = percent;
        response.widget_info(|| {
            let mut info = egui::WidgetInfo::drag_value(true, shown);
            info.label = Some(tr("image-size-scale").to_owned());
            info
        });
        response
    });
    if response.on_hover_text(tr("image-size-scale-tip")).changed() {
        dialog.set_percent(percent);
    }
    let resizable = dialog.aspect_resizable();
    let mut keep = dialog.keep_aspect;
    let check = ui
        .add_enabled_ui(resizable, |ui| {
            widgets::checkbox(ui, &mut keep, tr("image-size-keep-aspect"))
        })
        .inner;
    if !resizable {
        check.on_disabled_hover_text(tr("image-size-keep-aspect-fixed"));
    } else if keep != dialog.keep_aspect {
        dialog.set_keep_aspect(keep);
    }
    ui.add_space(8.0);

    widgets::field_label(ui, tr("image-size-sampling"));
    ui.horizontal(|ui| {
        for (sampling, title) in [
            (Sampling::Smooth, "sampling-smooth"),
            (Sampling::Nearest, "sampling-pixels"),
        ] {
            ui.radio_value(&mut dialog.sampling, sampling, tr(title));
        }
    });
    dialog.size != dialog.current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reference_point_and_typed_offsets_follow_each_other() {
        // As 2.2.13's configuresCanvasSizeDialog.
        let mut dialog = CanvasSize::new([640, 480]);
        assert_eq!((dialog.anchor, dialog.offset), (Some(4), [0, 0]));
        assert!(!dialog.changes());
        dialog.set_size([800, 600]);
        assert_eq!(dialog.offset, [80, 60]);
        dialog.set_anchor(0);
        assert_eq!(dialog.offset, [0, 0]);
        dialog.set_anchor(8);
        assert_eq!(dialog.offset, [160, 120]);
        dialog.set_size([540, 500]);
        assert_eq!(dialog.offset, [-100, 20]);
        dialog.set_anchor(4);
        assert_eq!(dialog.offset, [-50, 10]);
        dialog.set_offset([-50, 11]);
        assert_eq!(dialog.anchor, None);
        dialog.set_offset([0, 20]);
        assert_eq!(dialog.anchor, Some(6));
        // The middle rounds toward zero.
        dialog.set_size([635, 485]);
        dialog.set_anchor(4);
        assert_eq!(dialog.offset, [-2, 2]);
        // The same size with the artwork moved is a change.
        let mut shift = CanvasSize::new([100, 100]);
        shift.set_offset([-20, 5]);
        assert!(shift.changes());
        assert_eq!(shift.anchor, None);
    }

    #[test]
    fn canvas_sizes_and_offsets_stay_within_the_limits() {
        let mut dialog = CanvasSize::new([100, 80]);
        dialog.set_size([0, 5000]);
        assert_eq!(dialog.size, [1, 4096]);
        dialog.set_offset([-9000, 9000]);
        assert_eq!(dialog.offset, [-4096, 4096]);
        assert_eq!(dialog.hint(), "canvas-size-outside");
        dialog.set_size([100, 80]);
        dialog.set_offset([-10, 0]);
        assert_eq!(dialog.hint(), "canvas-size-clipped");
        dialog.set_size([120, 80]);
        dialog.set_offset([10, 0]);
        assert_eq!(dialog.hint(), "canvas-size-expanded");
    }

    #[test]
    fn the_image_size_keeps_the_aspect_ratio_unless_told_not_to() {
        // As 2.2.13's configuresImageSizeDialog.
        let mut dialog = ImageSize::new([640, 480]);
        assert!(dialog.keep_aspect);
        dialog.set_width(1280);
        assert_eq!(dialog.size, [1280, 960]);
        assert_eq!(dialog.percent(), 200.0);
        dialog.set_percent(150.0);
        assert_eq!(dialog.size, [960, 720]);
        dialog.set_keep_aspect(false);
        dialog.set_width(800);
        dialog.set_height(900);
        assert_eq!(dialog.size, [800, 900]);
        assert_eq!(dialog.scales(), [125.0, 187.5]);
        assert!(dialog.distorted());
        dialog.set_keep_aspect(true);
        assert_eq!(dialog.size, [800, 600]);
        assert!(!dialog.distorted());
        dialog.set_percent(200.0);
        assert_eq!(dialog.size, [1280, 960]);
        // The scale stops where an edge would leave the limits.
        dialog.set_percent(10_000.0);
        assert_eq!(dialog.size, [4096, 3072]);
        dialog.set_height(1);
        assert_eq!(dialog.size, [1, 1]);
    }

    #[test]
    fn extreme_aspect_ratios_cannot_be_kept() {
        // As 2.2.13's unlocksImageSizeDialogForExtremeAspectRatios.
        for current in [[4096, 1], [1, 4096]] {
            let mut dialog = ImageSize::new(current);
            assert!(!dialog.aspect_resizable());
            assert!(!dialog.keep_aspect);
            dialog.set_keep_aspect(true);
            assert!(!dialog.keep_aspect);
            assert_eq!(dialog.ranges(), [[1, 4096]; 2]);
        }
        let mut wide = ImageSize::new([4096, 1]);
        wide.set_width(2048);
        assert_eq!(wide.size, [2048, 1]);
        let mut thin = ImageSize::new([100, 1]);
        assert!(thin.keep_aspect);
        thin.set_width(400);
        assert_eq!(thin.size, [400, 4]);
    }
}
