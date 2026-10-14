// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The layer dock, after 2.2.13's: rows with a thumbnail, "opacity  blend"
//! over the name, and visibility and wobble toggles; a row of buttons; and
//! the current layer's blend mode, group, clipping, reference flag and
//! opacity. Layers move by dragging, into a group by dropping on it, and
//! are renamed by double-click or F2. Groups also fold, which 2.2.13's list
//! did not.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use egui::{Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, Ui, Vec2};
use ugu_core::command;
use ugu_core::document::{Document, Layer, LayerId, LayerKind};
use ugu_core::edit::{EditError, Outcome};
use ugu_core::ops::{Blend, MergeRefusal, Wobble};
use ugu_session::Session;

use crate::canvas::Canvas;
use crate::i18n::{tr, tr_with};
use crate::icons::{self, Glyph};
use crate::theme;
use crate::widgets;

const ROW: f32 = 56.0;
const INDENT: f32 = 14.0;
/// `widgets::icon_button`'s side for a 16pt glyph.
const DELETE_SIDE: f32 = 16.0 + 8.0;
const THUMB: Vec2 = Vec2::new(48.0, 32.0);
const BLENDS: [Blend; 4] = [
    Blend::Normal,
    Blend::Multiply,
    Blend::Screen,
    Blend::Overlay,
];

#[derive(Default)]
pub struct LayerDock {
    folded: HashSet<LayerId>,
    /// The layer being renamed and the text so far.
    renaming: Option<(LayerId, String)>,
    /// Opacity being dragged, committed when the drag ends.
    opacity: Option<(LayerId, f32)>,
    /// The layer being dragged in the list.
    dragging: Option<LayerId>,
    thumbnails: HashMap<LayerId, Thumbnail>,
    /// How tall the buttons and properties under the list were last drawn,
    /// which the list leaves room for.
    below: f32,
    /// Other documents' folded groups, by generation, while another is shown.
    folded_elsewhere: HashMap<u64, HashSet<LayerId>>,
}

/// A layer's thumbnail: premultiplied pixels, what they were made from, and
/// the texture showing them on the paper colour.
struct Thumbnail {
    key: u64,
    size: [usize; 2],
    pixels: Arc<Vec<u8>>,
    texture: egui::TextureHandle,
}

/// `pixels` over the paper colour, for showing.
fn on_paper(pixels: &[u8], size: [usize; 2], paper: [u8; 4]) -> egui::ColorImage {
    let rgba: Vec<u8> = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|pixel| {
            let rest = 1.0 - f32::from(pixel[3]) / 255.0;
            let mix = |channel: usize| {
                (f32::from(pixel[channel]) + f32::from(paper[channel]) * rest)
                    .round()
                    .min(255.0) as u8
            };
            [mix(0), mix(1), mix(2), 255]
        })
        .collect();
    egui::ColorImage::from_rgba_unmultiplied(size, &rgba)
}

/// Where a dragged layer would land.
#[derive(Clone, Copy, PartialEq)]
enum Drop {
    Above(LayerId),
    Below(LayerId),
    Into(LayerId),
}

struct Row {
    id: LayerId,
    name: String,
    depth: usize,
    visible: bool,
    opacity: f32,
    blend: Blend,
    /// For a group, whether it is folded.
    folded: Option<bool>,
    clipped: bool,
    reference: bool,
    /// For a paint layer, whether it wobbles.
    wobbles: Option<bool>,
    /// Whether it has keys, and whether it is an animation folder.
    keyed: bool,
    folder: bool,
}

fn blend_name(blend: Blend) -> &'static str {
    match blend {
        Blend::Normal => tr("blend-normal"),
        Blend::Multiply => tr("blend-multiply"),
        Blend::Screen => tr("blend-screen"),
        Blend::Overlay => tr("blend-overlay"),
    }
}

fn flatten(
    document: &Document,
    layers: &[Layer],
    depth: usize,
    folded: &HashSet<LayerId>,
    rows: &mut Vec<Row>,
) {
    for layer in layers.iter().rev() {
        let fold = folded.contains(&layer.id);
        rows.push(Row {
            id: layer.id,
            name: layer.name.clone(),
            depth,
            visible: layer.visible,
            opacity: layer.opacity(),
            blend: layer.blend(),
            folded: matches!(layer.kind, LayerKind::Group(_)).then_some(fold),
            clipped: layer.clip_to_below(),
            reference: layer.reference,
            wobbles: match &layer.kind {
                LayerKind::Paint(paint) => {
                    Some(paint.wobble.unwrap_or(document.wobble).amount > 0.0)
                }
                LayerKind::Group(_) => None,
            },
            keyed: !layer.keys().is_empty(),
            folder: layer.cels().is_some(),
        });
        if let LayerKind::Group(group) = &layer.kind
            && !fold
        {
            flatten(document, &group.children, depth + 1, folded, rows);
        }
    }
}

/// Why `id` cannot be merged down, in 2.2.13's words, or `None`.
fn merge_refusal(document: &Document, id: LayerId) -> Option<&'static str> {
    Some(match command::merge_down(document, id).err()? {
        EditError::Merge(MergeRefusal::Blend | MergeRefusal::Clipping) => tr("merge-properties"),
        EditError::Merge(MergeRefusal::CanvasChange) => tr("merge-canvas-change"),
        EditError::Merge(MergeRefusal::CanvasEpoch) => tr("merge-epoch"),
        EditError::Merge(MergeRefusal::Animated) => tr("merge-animated"),
        _ => {
            let paint = |id: LayerId| {
                matches!(
                    document.layer(id).map(|layer| &layer.kind),
                    Some(LayerKind::Paint(_))
                )
            };
            let below = document.position(id).and_then(|(parent, index)| {
                Some(
                    command::siblings(document, parent)?
                        .get(index.checked_sub(1)?)?
                        .id,
                )
            });
            if !paint(id) {
                tr("merge-select-paint")
            } else if !below.is_some_and(paint) {
                tr("merge-no-paint-below")
            } else {
                tr("merge-properties")
            }
        }
    })
}

/// The groups `id` can move into: all but itself and what it holds.
fn groups_outside(layers: &[Layer], id: LayerId, groups: &mut Vec<(LayerId, String)>) {
    for layer in layers.iter().rev() {
        if layer.id == id {
            continue;
        }
        if let LayerKind::Group(group) = &layer.kind {
            groups.push((layer.id, layer.name.clone()));
            groups_outside(&group.children, id, groups);
        }
    }
}

fn copy_name(name: &str) -> String {
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("name", name.to_owned());
    tr_with("layer-copy-name", &args)
}

fn report(refusal: &mut Option<String>, result: Result<Outcome, EditError>) {
    match result {
        Ok(Outcome::Committed(_)) => *refusal = None,
        Ok(Outcome::NoChange) => {}
        Err(error) => {
            tracing::warn!(%error, "edit refused");
            *refusal = Some(crate::canvas::edit_reason(&error).to_owned());
        }
    }
}

impl LayerDock {
    /// Drops the thumbnail textures, which a new GPU device does not have;
    /// they are made again from the canvas's thumbnails.
    pub fn forget_textures(&mut self) {
        self.thumbnails.clear();
    }

    /// Brings the thumbnails up to date with the canvas's, groups made from
    /// their children.
    fn refresh_thumbnails(&mut self, ctx: &egui::Context, canvas: &Canvas) {
        let document = canvas.session().document();
        let paper = document.background.0;
        self.refresh(ctx, canvas, &document.layers, paper);
        self.thumbnails
            .retain(|id, _| document.layer(*id).is_some());
    }

    fn refresh(&mut self, ctx: &egui::Context, canvas: &Canvas, layers: &[Layer], paper: [u8; 4]) {
        for layer in layers {
            let made = match &layer.kind {
                LayerKind::Paint(_) => canvas.thumbnail(layer.id).and_then(|(revision, pixmap)| {
                    if self
                        .thumbnails
                        .get(&layer.id)
                        .is_some_and(|thumbnail| thumbnail.key == revision)
                    {
                        return None;
                    }
                    let size = [usize::from(pixmap.width()), usize::from(pixmap.height())];
                    Some((revision, size, pixmap.data_as_u8_slice().to_vec()))
                }),
                LayerKind::Group(group) => {
                    self.refresh(ctx, canvas, &group.children, paper);
                    canvas.thumbnail(layer.id).and_then(|(key, pixmap)| {
                        if self
                            .thumbnails
                            .get(&layer.id)
                            .is_some_and(|thumbnail| thumbnail.key == key)
                        {
                            return None;
                        }
                        let size = [usize::from(pixmap.width()), usize::from(pixmap.height())];
                        Some((key, size, pixmap.data_as_u8_slice().to_vec()))
                    })
                }
            };
            if let Some((key, size, pixels)) = made {
                let image = on_paper(&pixels, size, paper);
                let options = egui::TextureOptions::LINEAR;
                match self.thumbnails.get_mut(&layer.id) {
                    Some(thumbnail) => {
                        thumbnail.texture.set(image, options);
                        thumbnail.key = key;
                        thumbnail.size = size;
                        thumbnail.pixels = Arc::new(pixels);
                    }
                    None => {
                        let texture =
                            ctx.load_texture(format!("layer {}", layer.id.0), image, options);
                        self.thumbnails.insert(
                            layer.id,
                            Thumbnail {
                                key,
                                size,
                                pixels: Arc::new(pixels),
                                texture,
                            },
                        );
                    }
                }
            }
        }
    }

    /// Drops what was under way in document `from`'s list and keeps its
    /// folded groups for when it is shown again, as `to` is now. Layer ids
    /// are a document's own, so nothing else carries over.
    pub fn switch_document(&mut self, from: u64, to: u64) {
        let folded = std::mem::take(&mut self.folded);
        if !folded.is_empty() {
            self.folded_elsewhere.insert(from, folded);
        }
        self.folded = self.folded_elsewhere.remove(&to).unwrap_or_default();
        self.renaming = None;
        self.opacity = None;
        self.dragging = None;
        self.thumbnails.clear();
    }

    /// Forgets the folded groups of documents no longer open.
    pub fn keep_documents(&mut self, open: &[u64]) {
        self.folded_elsewhere
            .retain(|generation, _| open.contains(generation));
    }

    pub fn show(&mut self, ui: &mut Ui, canvas: &mut Canvas, refusal: &mut Option<String>) {
        // Rows are drawn before what is done to them is applied, so a change
        // needs another frame to show, on screen and to screen readers.
        let state = |dock: &Self, canvas: &Canvas| {
            (
                canvas.session().revision(),
                canvas.session().current_layer(),
                dock.folded.len(),
                dock.renaming.as_ref().map(|(id, _)| *id),
            )
        };
        let before = state(self, canvas);
        self.draw(ui, canvas, refusal);
        if state(self, canvas) != before {
            ui.ctx().request_repaint();
        }
    }

    fn draw(&mut self, ui: &mut Ui, canvas: &mut Canvas, refusal: &mut Option<String>) {
        self.refresh_thumbnails(ui.ctx(), canvas);
        let current = canvas.session().current_layer();
        let mut rows = Vec::new();
        let document = canvas.session().document();
        flatten(document, &document.layers, 0, &self.folded, &mut rows);

        let list_height = (ui.available_height() - self.below).max(ROW * 2.0);
        let mut drop = None;
        egui::ScrollArea::vertical()
            .id_salt("layer-list")
            .max_height(list_height)
            .min_scrolled_height(list_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for row in &rows {
                    if let Some(target) = self.row(ui, canvas, row, row.id == current, refusal) {
                        drop = Some(target);
                    }
                }
            });
        if let Some(target) = drop
            && let Some(dragged) = self.dragging.take()
        {
            report(
                refusal,
                canvas.edit(|session| move_by_drop(session, dragged, target)),
            );
        }
        if ui.input(|input| input.pointer.any_released()) {
            self.dragging = None;
        }

        let list_end = ui.cursor().top();
        ui.add_space(theme::space::XS);
        self.buttons(ui, canvas, refusal);
        if let Some(text) = refusal.as_deref() {
            ui.colored_label(theme::accent_ink(), text);
        }
        ui.add_space(theme::space::XS);
        self.properties(ui, canvas, current, refusal);
        let below = ui.cursor().top() - list_end;
        if below != self.below {
            self.below = below;
            ui.ctx().request_repaint();
        }
    }

    /// Draws one row and handles what is done to it; returns a drop target
    /// when a dragged layer is let go over it.
    fn row(
        &mut self,
        ui: &mut Ui,
        canvas: &mut Canvas,
        row: &Row,
        selected: bool,
        refusal: &mut Option<String>,
    ) -> Option<Drop> {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), ROW),
            Sense::click_and_drag(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &row.name)
        });
        let painter = ui.painter_at(rect);
        let card = rect.shrink2(egui::vec2(4.0, 2.0));
        if selected {
            painter.rect_filled(
                card,
                CornerRadius::same(theme::radius::CONTROL),
                theme::control(),
            );
            let bar = Rect::from_center_size(
                egui::pos2(card.left() + 4.0 + 1.5, card.center().y),
                egui::vec2(3.0, 20.0),
            );
            painter.rect_filled(bar, CornerRadius::same(2), theme::accent());
        } else if response.hovered() {
            painter.rect_filled(
                card,
                CornerRadius::same(theme::radius::CONTROL),
                theme::hover(),
            );
        }
        // The painter is clipped to the row, so the ring goes inside.
        if response.has_focus() {
            painter.rect_stroke(
                card,
                CornerRadius::same(theme::radius::CONTROL),
                Stroke::new(1.0, theme::accent_ink()),
                egui::StrokeKind::Inside,
            );
        }
        let dim = |color: Color32| {
            if row.visible {
                color
            } else {
                color.gamma_multiply(0.4)
            }
        };

        // Fold toggle for groups, then the thumbnail, indented by depth.
        let left = card.left() + 13.0 + row.depth as f32 * INDENT;
        if let Some(folded) = row.folded {
            let fold =
                Rect::from_center_size(egui::pos2(left + 2.0, card.center().y), Vec2::splat(14.0));
            let fold_response = ui.interact(fold, response.id.with("fold"), Sense::click());
            let glyph = if folded {
                Glyph::MoveDown
            } else {
                Glyph::MoveUp
            };
            icons::paint(&painter, fold, glyph, theme::muted(), 0.0);
            if fold_response.has_focus() {
                crate::widgets::focus_ring(&painter, fold, theme::radius::SMALL);
            }
            fold_response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    if folded {
                        tr("group-unfold")
                    } else {
                        tr("group-fold")
                    },
                )
            });
            if fold_response.clicked() && !self.folded.remove(&row.id) {
                self.folded.insert(row.id);
            }
        }
        let thumb = Rect::from_min_size(
            egui::pos2(left + 12.0, card.center().y - THUMB.y / 2.0),
            THUMB,
        );
        painter.rect_filled(
            thumb,
            CornerRadius::same(theme::radius::SMALL),
            dim(theme::base()),
        );
        if let Some(thumbnail) = self.thumbnails.get(&row.id) {
            let [width, height] = thumbnail.size.map(|edge| edge as f32);
            let scale = (thumb.width() / width).min(thumb.height() / height);
            let shown =
                Rect::from_center_size(thumb.center(), egui::vec2(width * scale, height * scale));
            let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(thumbnail.texture.id(), shown, uv, dim(Color32::WHITE));
        }
        painter.rect_stroke(
            thumb,
            CornerRadius::same(theme::radius::SMALL),
            Stroke::new(1.0, theme::border()),
            egui::StrokeKind::Inside,
        );

        // Toggles on the right: visibility, and wobble for paint layers.
        let eye = Rect::from_center_size(
            egui::pos2(card.right() - 8.0 - 10.0, card.center().y),
            Vec2::splat(20.0),
        );
        let eye_response = ui.interact(eye.expand(3.0), response.id.with("eye"), Sense::click());
        let (glyph, ink) = if row.visible {
            (Glyph::EyeOpen, theme::muted())
        } else {
            (Glyph::EyeClosed, theme::disabled())
        };
        icons::paint(&painter, eye, glyph, ink, 0.0);
        if eye_response.has_focus() {
            crate::widgets::focus_ring(&painter, eye, theme::radius::SMALL);
        }
        eye_response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Checkbox,
                true,
                row.visible,
                if row.visible {
                    tr("layer-visible")
                } else {
                    tr("layer-hidden")
                },
            )
        });
        if eye_response.clicked() {
            let (id, shown) = (row.id, !row.visible);
            report(
                refusal,
                canvas.edit(|session| {
                    session.update_layer(id, "Show or hide layer", |layer| layer.visible = shown)
                }),
            );
        }
        let mut text_right = eye.left() - 6.0;
        if let Some(wobbles) = row.wobbles {
            let wobble = Rect::from_center_size(
                egui::pos2(eye.left() - 6.0 - 9.0, card.center().y),
                Vec2::splat(18.0),
            );
            let wobble_response = ui.interact(
                wobble.expand(3.0),
                response.id.with("wobble"),
                Sense::click(),
            );
            icons::paint(
                &painter,
                wobble,
                Glyph::Wobble,
                if wobbles {
                    theme::accent_ink()
                } else {
                    theme::disabled()
                },
                0.0,
            );
            if wobble_response.has_focus() {
                crate::widgets::focus_ring(&painter, wobble, theme::radius::SMALL);
            }
            wobble_response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Checkbox,
                    true,
                    wobbles,
                    if wobbles {
                        tr("layer-wobbles")
                    } else {
                        tr("layer-still")
                    },
                )
            });
            if wobble_response.clicked() {
                let id = row.id;
                // Held still, or back to following the drawing.
                report(
                    refusal,
                    canvas.edit(|session| {
                        session.update_layer(id, "Layer wobble", |layer| {
                            if let LayerKind::Paint(paint) = &mut layer.kind {
                                paint.wobble = if wobbles {
                                    Some(Wobble::classic(0.0))
                                } else {
                                    None
                                };
                            }
                        })
                    }),
                );
            }
            text_right = wobble.left() - 6.0;
        }
        // Marks, not buttons: moved by keys, and an animation folder.
        for (shown, glyph, name) in [
            (row.keyed, Glyph::Key, tr("layer-keyed")),
            (
                row.folder,
                Glyph::AnimationFolder,
                tr("layer-animation-folder"),
            ),
        ] {
            if !shown {
                continue;
            }
            let mark = Rect::from_center_size(
                egui::pos2(text_right - 8.0, card.center().y),
                Vec2::splat(14.0),
            );
            icons::paint(&painter, mark, glyph, dim(theme::muted()), 0.0);
            ui.interact(mark, response.id.with(name), Sense::hover())
                .on_hover_text(name)
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, name));
            text_right = mark.left() - 4.0;
        }

        // "opacity %  blend" over the name, with badges before the name.
        let text_left = thumb.right() + 10.0;
        let percent = tr_with(
            "percent",
            &super::args([("value", format!("{:.0} ", row.opacity * 100.0))]),
        );
        let meta = one_line(
            &painter,
            format!("{percent}  {}", blend_name(row.blend)),
            theme::SMALL,
            theme::muted(),
            text_right - text_left,
        );
        painter.galley(
            egui::pos2(text_left, card.center().y - 2.0 - meta.size().y),
            meta,
            theme::muted(),
        );
        let name_pos = egui::pos2(text_left, card.center().y + 1.0);
        let renaming = self.renaming.as_ref().is_some_and(|(id, _)| *id == row.id);
        if renaming {
            let field_rect = Rect::from_min_max(
                egui::pos2(text_left - 4.0, name_pos.y - 2.0),
                egui::pos2(text_right, name_pos.y + 20.0),
            );
            let (_, text) = self.renaming.as_mut().expect("renaming");
            let field = ui.put(
                field_rect,
                egui::TextEdit::singleline(text).id_salt("layer-rename"),
            );
            if !field.has_focus() && !field.lost_focus() {
                field.request_focus();
            }
            let cancel = ui.input(|input| input.key_pressed(egui::Key::Escape));
            if field.lost_focus() || cancel {
                let (id, text) = self.renaming.take().expect("renaming");
                let edited: String = text
                    .trim()
                    .chars()
                    .take(ugu_core::document::limits::LAYER_NAME_CHARS)
                    .collect();
                if !cancel && !edited.is_empty() && edited != row.name {
                    report(
                        refusal,
                        canvas.edit(|session| {
                            session.update_layer(id, "Rename layer", |layer| layer.name = edited)
                        }),
                    );
                    tracing::info!("layer renamed");
                }
            }
        } else {
            let mut badges = Vec::new();
            if row.folded.is_some() {
                badges.push("G");
            }
            if row.clipped {
                badges.push("↳");
            }
            if row.reference {
                badges.push("R");
            }
            let mut x = text_left;
            for badge in badges {
                let galley = painter.layout_no_wrap(
                    badge.to_owned(),
                    FontId::proportional(theme::BODY),
                    theme::accent_ink(),
                );
                let width = galley.size().x;
                painter.galley(egui::pos2(x, name_pos.y), galley, theme::accent_ink());
                x += width + 5.0;
            }
            let name_color = if row.visible {
                theme::text()
            } else {
                theme::muted()
            };
            let galley = one_line(
                &painter,
                row.name.clone(),
                theme::BODY,
                name_color,
                text_right - x,
            );
            painter.galley(egui::pos2(x, name_pos.y), galley, name_color);
        }

        let toggles_hit = eye_response.hovered() || ui.ctx().is_being_dragged(eye_response.id);
        if response.clicked() && !toggles_hit {
            let id = row.id;
            canvas.edit_settled(|session| session.select_layer(id));
            // egui focuses only text fields on click; F2 and Space need the row.
            response.request_focus();
        }
        if response.double_clicked()
            || (selected
                && response.has_focus()
                && ui.input(|input| input.key_pressed(egui::Key::F2)))
        {
            self.renaming = Some((row.id, row.name.clone()));
        }
        // Over the canvas, Space pans instead.
        let over_canvas = ui.ctx().pointer_hover_pos().is_some_and(|pos| {
            let ppp = ui.ctx().pixels_per_point();
            canvas.contains([f64::from(pos.x * ppp), f64::from(pos.y * ppp)])
        });
        if selected
            && response.has_focus()
            && !over_canvas
            && ui.input(|input| input.key_pressed(egui::Key::Space))
        {
            let (id, shown) = (row.id, !row.visible);
            report(
                refusal,
                canvas.edit(|session| {
                    session.update_layer(id, "Show or hide layer", |layer| layer.visible = shown)
                }),
            );
        }

        // Dragging: where the pointer is in the row decides the drop.
        if response.drag_started() {
            self.dragging = Some(row.id);
        }
        let dragged = self.dragging?;
        if dragged == row.id {
            return None;
        }
        let pointer = ui.input(|input| input.pointer.interact_pos())?;
        if !rect.contains(pointer) {
            return None;
        }
        let t = (pointer.y - rect.top()) / rect.height();
        let target = if row.folded.is_some() && (0.3..0.7).contains(&t) {
            Drop::Into(row.id)
        } else if t < 0.5 {
            Drop::Above(row.id)
        } else {
            Drop::Below(row.id)
        };
        let line = |y: f32| {
            Rect::from_min_max(
                egui::pos2(card.left() + 8.0, y - 1.0),
                egui::pos2(card.right() - 8.0, y + 1.0),
            )
        };
        match target {
            Drop::Above(_) => {
                painter.rect_filled(line(rect.top()), CornerRadius::same(1), theme::accent())
            }
            Drop::Below(_) => {
                painter.rect_filled(line(rect.bottom()), CornerRadius::same(1), theme::accent())
            }
            Drop::Into(_) => painter.rect_stroke(
                card,
                CornerRadius::same(theme::radius::CONTROL),
                Stroke::new(2.0, theme::accent_ink()),
                egui::StrokeKind::Inside,
            ),
        };
        ui.input(|input| input.pointer.any_released())
            .then_some(target)
    }

    fn buttons(&mut self, ui: &mut Ui, canvas: &mut Canvas, refusal: &mut Option<String>) {
        let document = canvas.session().document();
        let current = canvas.session().current_layer();
        let is_group = matches!(
            document.layer(current).map(|layer| &layer.kind),
            Some(LayerKind::Group(_))
        );
        let (count, index) = document
            .position(current)
            .and_then(|(parent, index)| Some((command::siblings(document, parent)?.len(), index)))
            .unwrap_or((0, 0));
        let merge_refused = merge_refusal(document, current);
        let removable = canvas.session().can_remove_layer();
        let duplicable = canvas.session().can_duplicate_layer();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.add_space(theme::space::XS);
            if widgets::icon_button(ui, Glyph::Add, 16.0, tr("layer-add"), true).clicked() {
                report(refusal, canvas.edit(Session::add_layer));
                tracing::info!("layer added");
            }
            if text_button(ui, Glyph::Add, "G", tr("layer-add-group"), true).clicked() {
                report(refusal, canvas.edit(Session::add_group));
            }
            if text_button(ui, Glyph::Remove, "G", tr("layer-ungroup"), is_group).clicked() {
                report(refusal, canvas.edit(Session::ungroup));
            }
            // Making layers, then what a layer holds, then the order.
            ui.add_space(theme::space::M);
            if widgets::icon_button(
                ui,
                Glyph::Duplicate,
                16.0,
                tr("layer-duplicate"),
                duplicable,
            )
            .clicked()
            {
                report(
                    refusal,
                    canvas.edit(|session| session.duplicate_layer(copy_name)),
                );
            }
            let merge = widgets::icon_button_tip(
                ui,
                Glyph::MergeDown,
                16.0,
                tr("layer-merge"),
                merge_refused.unwrap_or(tr("layer-merge")),
                merge_refused.is_none(),
            );
            if merge.clicked() {
                report(refusal, canvas.edit(Session::merge_down));
            }
            ui.add_space(theme::space::M);
            if widgets::icon_button(ui, Glyph::MoveUp, 16.0, tr("layer-up"), index + 1 < count)
                .clicked()
            {
                report(refusal, canvas.edit(|session| session.move_layer(1)));
            }
            if widgets::icon_button(ui, Glyph::MoveDown, 16.0, tr("layer-down"), index > 0)
                .clicked()
            {
                report(refusal, canvas.edit(|session| session.move_layer(-1)));
            }
            // Apart from the rest at the far end, so it is not hit by mistake.
            // A right-to-left layout with no room left starts at the cursor
            // and grows back over the buttons before it, so it is used only
            // when the button fits; otherwise the button follows the rest.
            let delete = |ui: &mut Ui| {
                widgets::icon_button(ui, Glyph::Delete, 16.0, tr("layer-delete"), removable)
            };
            let fits = ui.available_width() >= theme::space::M + DELETE_SIDE + theme::space::XS;
            let deleted = if fits {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(theme::space::XS);
                    delete(ui)
                })
                .inner
            } else {
                ui.add_space(theme::space::M);
                delete(ui)
            };
            if deleted.clicked() {
                report(refusal, canvas.edit(Session::remove_layer));
            }
        });
    }

    fn properties(
        &mut self,
        ui: &mut Ui,
        canvas: &mut Canvas,
        current: LayerId,
        refusal: &mut Option<String>,
    ) {
        let document = canvas.session().document();
        let Some(layer) = document.layer(current) else {
            return;
        };
        let (opacity, blend, clipped, reference) = (
            layer.opacity(),
            layer.blend(),
            layer.clip_to_below(),
            layer.reference,
        );
        let is_paint = matches!(layer.kind, LayerKind::Paint(_));
        let parent = document.position(current).and_then(|(parent, _)| parent);
        let mut groups = Vec::new();
        groups_outside(&document.layers, current, &mut groups);

        let mut chosen = blend;
        widgets::form_row(ui, tr("blend-mode"), |ui| {
            egui::ComboBox::from_id_salt("layer-blend")
                .width(ui.available_width())
                .selected_text(blend_name(blend))
                .show_ui(ui, |ui| {
                    for each in BLENDS {
                        widgets::choice(ui, &mut chosen, each, blend_name(each));
                    }
                })
                .response
                .widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::ComboBox,
                        true,
                        tr("blend-mode-name"),
                    )
                });
        });
        if chosen != blend {
            report(
                refusal,
                canvas.edit(|session| {
                    session.update_layer(current, "Change layer blend mode", |layer| {
                        match &mut layer.kind {
                            LayerKind::Paint(paint) => paint.blend = chosen,
                            LayerKind::Group(group) => group.blend = chosen,
                        }
                    })
                }),
            );
        }

        let mut target = parent;
        let shown = parent
            .and_then(|id| groups.iter().find(|(group, _)| *group == id))
            .map_or(tr("group-none"), |(_, name)| name.as_str());
        widgets::form_row(ui, tr("group"), |ui| {
            egui::ComboBox::from_id_salt("layer-group")
                .width(ui.available_width())
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    widgets::choice(ui, &mut target, None, tr("group-none"));
                    for (id, name) in &groups {
                        widgets::choice(ui, &mut target, Some(*id), name);
                    }
                })
                .response
                .widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, tr("group-parent"))
                });
        });
        if target != parent {
            report(
                refusal,
                canvas.edit(|session| session.move_to_group(target)),
            );
        }

        if is_paint {
            let mut clip = clipped;
            if widgets::checkbox(ui, &mut clip, tr("clip"))
                .on_hover_text(tr("clip-tip"))
                .changed()
            {
                report(
                    refusal,
                    canvas.edit(|session| {
                        session.update_layer(current, "Change layer clipping", |layer| {
                            if let LayerKind::Paint(paint) = &mut layer.kind {
                                paint.clip_to_below = clip;
                            }
                        })
                    }),
                );
            }
            let mut marked = reference;
            if widgets::checkbox(ui, &mut marked, tr("reference"))
                .on_hover_text(tr("reference-tip"))
                .changed()
            {
                report(
                    refusal,
                    canvas.edit(|session| {
                        session.update_layer(current, "Reference layer", |layer| {
                            layer.reference = marked
                        })
                    }),
                );
            }
        }

        if self.opacity.is_none_or(|(id, _)| id != current) {
            self.opacity = Some((current, opacity));
        }
        let mut percent = self.opacity.expect("set").1 * 100.0;
        let slider = widgets::form_row(ui, tr("opacity"), |ui| {
            let field = widgets::number(ui, &mut percent, 0.0..=100.0, "%", 0, 64.0, tr("opacity"));
            let width = ui.available_width().max(40.0);
            widgets::slider(ui, &mut percent, 0.0..=100.0, tr("opacity"), width).union(field)
        });
        let value = percent.round() / 100.0;
        self.opacity = Some((current, value));
        // One undo step when the drag ends, as 2.2.13 commits on release.
        if (slider.drag_stopped() || (slider.changed() && !slider.dragged())) && value != opacity {
            report(
                refusal,
                canvas.edit(|session| {
                    session.update_layer(current, "Layer opacity", |layer| match &mut layer.kind {
                        LayerKind::Paint(paint) => paint.opacity = value,
                        LayerKind::Group(group) => group.opacity = value,
                    })
                }),
            );
            self.opacity = None;
        }
    }
}

/// `text` on one line `width` wide at most, cut short with an ellipsis.
fn one_line(
    painter: &egui::Painter,
    text: String,
    size: f32,
    color: Color32,
    width: f32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text, FontId::proportional(size), color);
    job.wrap = egui::text::TextWrapping::from_wrap_mode_and_width(
        egui::TextWrapMode::Truncate,
        width.max(10.0),
    );
    painter.layout_job(job)
}

/// A glyph followed by a letter, as 2.2.13's "+ G" button.
fn text_button(
    ui: &mut Ui,
    glyph: Glyph,
    letter: &str,
    name: &str,
    enabled: bool,
) -> egui::Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(36.0, 24.0), sense);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name));
    if enabled && response.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(theme::radius::CONTROL),
            theme::hover(),
        );
    }
    let ink = if enabled {
        theme::text()
    } else {
        theme::disabled()
    };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(
            egui::pos2(rect.left() + 12.0, rect.center().y),
            Vec2::splat(16.0),
        ),
        glyph,
        ink,
        0.0,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 21.0, rect.center().y),
        Align2::LEFT_CENTER,
        letter,
        FontId::proportional(theme::BODY),
        ink,
    );
    if response.has_focus() {
        crate::widgets::focus_ring(ui.painter(), rect, theme::radius::CONTROL);
    }
    response.on_hover_text(name)
}

/// Moves `id` to where it was dropped, as one undo step.
fn move_by_drop(session: &mut Session, id: LayerId, target: Drop) -> Result<Outcome, EditError> {
    let document = session.document();
    let (parent, index) = match target {
        Drop::Into(group) => {
            let count = command::siblings(document, Some(group)).map_or(0, <[Layer]>::len);
            (Some(group), count)
        }
        Drop::Above(next) | Drop::Below(next) => {
            let (parent, index) = document
                .position(next)
                .ok_or(EditError::NoSuchLayer(next))?;
            // The list runs top first, so above means a higher index.
            let at = if matches!(target, Drop::Above(_)) {
                index + 1
            } else {
                index
            };
            // Taking `id` out first shifts later siblings down by one.
            let shift = matches!(document.position(id), Some((from_parent, from)) if from_parent == parent && from < at);
            (parent, at - usize::from(shift))
        }
    };
    if document.position(id) == Some((parent, index)) {
        return Ok(Outcome::NoChange);
    }
    session.move_layer_to(id, parent, index)
}

#[cfg(test)]
mod tests {
    use egui::{Pos2, RawInput, Rect, vec2};
    use ugu_core::document::Document;

    use super::*;

    #[test]
    fn delete_never_covers_the_other_layer_buttons() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut canvas = Canvas::new(Document::new([64, 64]), |_| {});
        let mut dock = LayerDock::default();
        let mut refusal = None;
        for width in (150..=460).step_by(10).map(|width| width as f32) {
            let mut output = ctx.run_ui(
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                    ..RawInput::default()
                },
                |ui| {
                    ui.allocate_ui(vec2(width, 100.0), |ui| {
                        dock.buttons(ui, &mut canvas, &mut refusal);
                    });
                },
            );
            output.textures_delta.clear();
            let tree = output.platform_output.accesskit_update.unwrap();
            let bounds = |name: &str| {
                tree.nodes
                    .iter()
                    .find(|(_, node)| node.label().is_some_and(|label| label == name))
                    .and_then(|(_, node)| node.bounds())
                    .unwrap_or_else(|| panic!("no {name} at {width}"))
            };
            let down = bounds(tr("layer-down"));
            let delete = bounds(tr("layer-delete"));
            assert!(delete.x0 >= down.x1, "at {width}: {down:?} and {delete:?}");
        }
    }
}
