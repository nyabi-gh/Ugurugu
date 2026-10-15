// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Changing the canvas size and the image size, as 2.2.13's dialogs: a new
//! size, absolute or relative, with a reference point and offset for the
//! canvas, and a uniform scale and kept aspect ratio for the image, which
//! also chooses how pixels are resampled. The canvas size is set beside a
//! frame on the canvas, which shows it and can be dragged.

use ugu_core::document::limits;
use ugu_core::ops::Sampling;

use crate::canvas::{Canvas, CanvasSize, clamp_edge};
use crate::i18n::{tr, tr_with};
use crate::theme;
use crate::widgets;

use super::{Panels, args, corners_on_screen, transform_box};

const EDGE: std::ops::RangeInclusive<u32> = limits::CANVAS_EDGE;

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

/// Shows the image size dialog and applies it when confirmed.
pub fn show(ctx: &egui::Context, canvas: &mut Canvas, panels: &mut Panels) {
    let Some(dialog) = panels.resize.as_mut() else {
        return;
    };
    let mut close = false;
    let mut apply = false;
    let pixel = canvas.session().pixel_mode();
    let modal = widgets::dialog(
        ctx,
        "resize dialog",
        460.0,
        tr("image-size-title"),
        Some(tr("image-size-description")),
        |ui| {
            let can_apply = image_form(ui, dialog, pixel);
            widgets::dialog_buttons(ui, |ui| {
                let ok =
                    widgets::button(ui, tr("dialog-ok"), widgets::Emphasis::Primary, can_apply);
                if widgets::button(ui, tr("dialog-cancel"), widgets::Emphasis::Secondary, true)
                    .clicked()
                {
                    close = true;
                }
                let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
                if ok.clicked() || (enter && can_apply) {
                    apply = true;
                }
            });
        },
    );
    if apply {
        let (size, sampling) = (dialog.size, dialog.sampling);
        panels.report(canvas.edit(|session| session.resample_image(size, sampling)));
        close = true;
    }
    if close || modal.should_close() {
        panels.resize = None;
    }
}

const SIZING_WIDTH: f32 = 340.0;

/// The window beside the canvas while it is sized, as Clip Studio's: what
/// the frame does and will do, the size and where the artwork lands, typed,
/// and applying or cancelling. It starts at the top right of the app's
/// window, over the right dock, and can be moved; the canvas takes no
/// presses under it. Tells the canvas where it is, so that the document is
/// placed clear of it.
pub fn sizing_window(
    ctx: &egui::Context,
    canvas_area: egui::Rect,
    canvas: &mut Canvas,
    panels: &mut Panels,
) {
    if canvas.sizing().is_none() {
        return;
    }
    let margin = theme::space::M;
    let (mut apply, mut cancel, mut open) = (false, false, true);
    let shown = egui::Window::new(tr("canvas-size-title"))
        .id(egui::Id::new("canvas size"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::RIGHT_TOP)
        .default_pos(egui::pos2(
            ctx.content_rect().right() - margin,
            canvas_area.top() + margin,
        ))
        .constrain_to(ctx.content_rect())
        .show(ctx, |ui| {
            ui.set_width(SIZING_WIDTH);
            let Some(dialog) = canvas.sizing_mut() else {
                return;
            };
            ui.add(
                egui::Label::new(
                    egui::RichText::new(tr("canvas-size-drag-hint"))
                        .size(theme::SMALL)
                        .color(theme::muted()),
                )
                .wrap(),
            );
            ui.add_space(theme::space::S);
            summary(ui, dialog.current, dialog.size);
            quiet(ui, tr(dialog.hint()));
            let can_apply = canvas_form(ui, dialog);
            widgets::dialog_buttons(ui, |ui| {
                apply = widgets::button(ui, tr("dialog-ok"), widgets::Emphasis::Primary, can_apply)
                    .clicked();
                cancel =
                    widgets::button(ui, tr("dialog-cancel"), widgets::Emphasis::Secondary, true)
                        .clicked();
            });
        });
    if let Some(shown) = shown {
        let ppp = ctx.pixels_per_point();
        let rect = shown.response.rect;
        let edge = |value: f32| (value * ppp).round() as i32;
        canvas.set_sizing_cover([
            edge(rect.left()),
            edge(rect.top()),
            edge(rect.right()),
            edge(rect.bottom()),
        ]);
    }
    if apply {
        panels.report(canvas.apply_sizing());
    } else if cancel || !open {
        canvas.cancel_sizing();
    }
}

/// Over the canvas being sized: the drawing the new canvas leaves out,
/// darkened, the room it adds, in the paper colour it will have, and the
/// frame with its handles.
pub fn sizing_overlay(ui: &mut egui::Ui, canvas: &Canvas) {
    let (Some(size), Some(corners)) = (canvas.sizing(), canvas.sizing_box()) else {
        return;
    };
    let frame = size.frame().map(|edge| edge as f64);
    let [width, height] = size.current.map(f64::from);
    let document = [0.0, 0.0, width, height];
    let [red, green, blue, alpha] = canvas.session().document().background.0;
    let paper = egui::Color32::from_rgba_unmultiplied(red, green, blue, alpha);
    let shade = egui::Color32::from_black_alpha(140);
    for (outer, inner, fill) in [(document, frame, shade), (frame, document, paper)] {
        for band in outside(outer, inner) {
            let points = corners_on_screen(ui, canvas, band).to_vec();
            ui.painter().add(egui::Shape::convex_polygon(
                points,
                fill,
                egui::Stroke::NONE,
            ));
        }
    }
    transform_box(ui, canvas, &corners, |at| canvas.sizing_grip_at(at));
}

/// The parts of rectangle `outer` (left, top, right, bottom) outside
/// `inner`, as at most four rectangles.
fn outside(outer: [f64; 4], inner: [f64; 4]) -> Vec<[f64; 4]> {
    let [left, top, right, bottom] = outer;
    let [in_left, in_right] = [inner[0], inner[2]].map(|x| x.clamp(left, right));
    let [in_top, in_bottom] = [inner[1], inner[3]].map(|y| y.clamp(top, bottom));
    if in_left >= in_right || in_top >= in_bottom {
        return vec![outer];
    }
    [
        [left, top, right, in_top],
        [left, in_bottom, right, bottom],
        [left, in_top, in_left, in_bottom],
        [in_right, in_top, right, in_bottom],
    ]
    .into_iter()
    .filter(|[left, top, right, bottom]| left < right && top < bottom)
    .collect()
}

fn summary(ui: &mut egui::Ui, from: [u32; 2], to: [u32; 2]) {
    quiet(
        ui,
        &tr_with(
            "size-change",
            &args([
                ("width", from[0].to_string()),
                ("height", from[1].to_string()),
                ("new_width", to[0].to_string()),
                ("new_height", to[1].to_string()),
            ]),
        ),
    );
}

/// A line under the preview, quieter than the settings.
fn quiet(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme::SMALL)
            .color(theme::muted()),
    );
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
    painter.rect_filled(
        rect,
        egui::CornerRadius::same(theme::radius::SMALL),
        theme::base(),
    );
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
                theme::accent().gamma_multiply(0.25),
                egui::Stroke::new(1.5, theme::accent_ink()),
                egui::StrokeKind::Inside,
            );
        } else {
            painter.rect(
                shape,
                egui::CornerRadius::ZERO,
                theme::control(),
                egui::Stroke::new(1.0, theme::border()),
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

/// The canvas size form: relative or not, width and height, the reference
/// point and the offset; returns whether the size can be applied.
fn canvas_form(ui: &mut egui::Ui, dialog: &mut CanvasSize) -> bool {
    widgets::section(ui, tr("canvas-size-section"), false);
    let mut size = dialog.size;
    widgets::card(ui, |ui| {
        let mut relative = dialog.relative;
        widgets::switch_row(
            ui,
            true,
            tr("canvas-size-relative"),
            None,
            &mut relative,
            true,
        )
        .on_hover_text(tr("canvas-size-relative-tip"));
        dialog.relative = relative;
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
                widgets::setting_row(ui, false, tr(change), None, |ui| {
                    widgets::whole(
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
                widgets::setting_row(ui, false, tr(label), None, |ui| {
                    widgets::whole(ui, &mut edge, low..=high, false, tr(name))
                });
                size[axis] = clamp_edge(edge);
            }
        }
    });
    if size != dialog.size {
        dialog.set_size(size);
    }

    widgets::section(ui, tr("canvas-size-position"), false);
    let mut offset = dialog.offset.map(i64::from);
    widgets::card(ui, |ui| {
        widgets::tall_setting_row(
            ui,
            true,
            tr("canvas-size-reference"),
            egui::Vec2::splat(ANCHORS_EDGE),
            |ui| anchors(ui, dialog),
        );
        let reach = i64::from(*EDGE.end());
        for (axis, label, name) in [
            (0, "canvas-size-offset-x", "canvas-size-offset-x-name"),
            (1, "canvas-size-offset-y", "canvas-size-offset-y-name"),
        ] {
            widgets::setting_row(ui, false, tr(label), None, |ui| {
                widgets::whole(ui, &mut offset[axis], -reach..=reach, true, tr(name))
            });
        }
    });
    let offset = offset.map(|value| value as i32);
    if offset != dialog.offset {
        dialog.set_offset(offset);
    }
    dialog.changes()
}

const ANCHOR: f32 = 18.0;
const ANCHOR_GAP: f32 = 4.0;
const ANCHORS_EDGE: f32 = 3.0 * ANCHOR + 2.0 * ANCHOR_GAP;

/// The nine points the canvas can grow or shrink around, as squares.
fn anchors(ui: &mut egui::Ui, dialog: &mut CanvasSize) {
    let (area, _) = ui.allocate_exact_size(egui::Vec2::splat(ANCHORS_EDGE), egui::Sense::hover());
    for (index, &key) in ANCHORS.iter().enumerate() {
        let chosen = dialog.anchor == Some(index);
        let step = ANCHOR + ANCHOR_GAP;
        let rect = egui::Rect::from_min_size(
            area.min + egui::vec2((index % 3) as f32 * step, (index / 3) as f32 * step),
            egui::Vec2::splat(ANCHOR),
        );
        let response = ui.interact(rect, ui.id().with(("anchor", index)), egui::Sense::click());
        let (fill, edge) = if chosen {
            (theme::accent(), theme::accent_ink())
        } else if response.hovered() {
            (theme::hover(), theme::disabled())
        } else {
            (theme::control(), theme::border())
        };
        ui.painter().rect(
            rect,
            egui::CornerRadius::same(theme::radius::SMALL),
            fill,
            egui::Stroke::new(1.0, edge),
            egui::StrokeKind::Inside,
        );
        if response.has_focus() {
            widgets::focus_ring(ui.painter(), rect, theme::radius::SMALL);
        }
        let response = response.on_hover_text(tr(key));
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, chosen, tr(key))
        });
        if response.clicked() {
            dialog.set_anchor(index);
        }
    }
}

/// The image size form; returns whether it can be applied.
/// In pixel mode it always resamples by whole pixels, so there is no choice.
fn image_form(ui: &mut egui::Ui, dialog: &mut ImageSize, pixel: bool) -> bool {
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
    ui.add_space(theme::space::S);
    summary(ui, dialog.current, dialog.size);
    let [horizontal, vertical] = dialog.scales();
    quiet(
        ui,
        &tr_with(
            "image-size-scales",
            &args([
                ("horizontal", format!("{horizontal:.1}")),
                ("vertical", format!("{vertical:.1}")),
            ]),
        ),
    );
    if dialog.distorted() {
        ui.label(
            egui::RichText::new(tr("image-size-distorted"))
                .size(theme::SMALL)
                .color(theme::accent_ink()),
        );
    }

    widgets::section(ui, tr("image-size-section"), false);
    widgets::card(ui, |ui| {
        let ranges = dialog.ranges();
        for (axis, label, name) in [
            (0, "size-width", "image-size-width"),
            (1, "size-height", "image-size-height"),
        ] {
            let mut edge = i64::from(dialog.size[axis]);
            let [low, high] = ranges[axis].map(i64::from);
            widgets::setting_row(ui, axis == 0, tr(label), None, |ui| {
                widgets::whole(ui, &mut edge, low..=high, false, tr(name))
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
        let response = widgets::setting_row(ui, false, tr("image-size-scale"), None, |ui| {
            let response = ui.add_sized(
                [96.0, theme::CONTROL_HEIGHT],
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
        let fixed = (!resizable).then(|| tr("image-size-keep-aspect-fixed"));
        let mut keep = dialog.keep_aspect;
        widgets::switch_row(
            ui,
            false,
            tr("image-size-keep-aspect"),
            fixed,
            &mut keep,
            resizable,
        );
        if resizable && keep != dialog.keep_aspect {
            dialog.set_keep_aspect(keep);
        }
        if !pixel {
            widgets::setting_row(ui, false, tr("image-size-sampling"), None, |ui| {
                let name = |sampling| {
                    tr(match sampling {
                        Sampling::Smooth => "sampling-smooth",
                        Sampling::Nearest => "sampling-pixels",
                    })
                };
                egui::ComboBox::from_id_salt("image sampling")
                    .selected_text(name(dialog.sampling))
                    .width(190.0)
                    .show_ui(ui, |ui| {
                        for sampling in [Sampling::Smooth, Sampling::Nearest] {
                            widgets::choice(ui, &mut dialog.sampling, sampling, name(sampling));
                        }
                    })
                    .response
                    .widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::ComboBox,
                            true,
                            tr("image-size-sampling"),
                        )
                    });
            });
        }
    });
    dialog.size != dialog.current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_parts_outside_a_rectangle_cover_the_rest_of_it() {
        let outer = [0.0, 0.0, 100.0, 80.0];
        assert_eq!(outside(outer, outer), Vec::<[f64; 4]>::new());
        assert_eq!(outside(outer, [200.0, 0.0, 300.0, 80.0]), [outer]);
        // Cropped on the right and grown upward: only a strip on the right.
        assert_eq!(
            outside(outer, [0.0, -20.0, 90.0, 80.0]),
            [[90.0, 0.0, 100.0, 80.0]]
        );
        let inner = [10.0, 20.0, 60.0, 50.0];
        let area: f64 = outside(outer, inner)
            .iter()
            .map(|[left, top, right, bottom]| (right - left) * (bottom - top))
            .sum();
        assert_eq!(area, 100.0 * 80.0 - 50.0 * 30.0);
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
