// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The layer dock, after 2.2.13's: rows with a thumbnail, "opacity  blend"
//! over the name, and visibility and wobble toggles; a row of buttons; and
//! the current layer's blend mode, group, clipping, reference flag and
//! opacity. Layers move by dragging, into a group by dropping on it, and
//! are renamed by double-click or F2. Groups also fold, which 2.2.13's list
//! did not.

use std::collections::HashSet;

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

fn report(refusal: &mut Option<String>, result: Result<Outcome, EditError>) {
    match result {
        Ok(Outcome::Committed(_)) => *refusal = None,
        Ok(Outcome::NoChange) => {}
        Err(error) => {
            tracing::warn!(%error, "edit refused");
            *refusal = Some(error.to_string());
        }
    }
}

impl LayerDock {
    pub fn show(&mut self, ui: &mut Ui, canvas: &mut Canvas, refusal: &mut Option<String>) {
        let current = canvas.session().current_layer();
        let mut rows = Vec::new();
        let document = canvas.session().document();
        flatten(document, &document.layers, 0, &self.folded, &mut rows);

        let list_height = (ui.available_height() - 210.0).max(ROW * 2.0);
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

        ui.add_space(4.0);
        self.buttons(ui, canvas, refusal);
        if let Some(text) = refusal.as_deref() {
            ui.colored_label(theme::ACCENT, text);
        }
        ui.add_space(4.0);
        self.properties(ui, canvas, current, refusal);
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
            painter.rect_filled(card, CornerRadius::same(7), theme::CONTROL);
            let bar = Rect::from_center_size(
                egui::pos2(card.left() + 4.0 + 1.5, card.center().y),
                egui::vec2(3.0, 20.0),
            );
            painter.rect_filled(bar, CornerRadius::same(2), theme::ACCENT);
        } else if response.hovered() {
            painter.rect_filled(card, CornerRadius::same(7), theme::HOVER);
        }
        let dim = |color: Color32| {
            if row.visible {
                color
            } else {
                color.gamma_multiply(0.4)
            }
        };

        // Fold toggle for groups, then the thumbnail, indented by depth.
        let left = card.left() + 3.0 + 8.0 + row.depth as f32 * INDENT;
        if let Some(folded) = row.folded {
            let fold =
                Rect::from_center_size(egui::pos2(left + 2.0, card.center().y), Vec2::splat(14.0));
            let fold_response = ui.interact(fold, response.id.with("fold"), Sense::click());
            let glyph = if folded {
                Glyph::MoveDown
            } else {
                Glyph::MoveUp
            };
            icons::paint(&painter, fold, glyph, theme::MUTED, 0.0);
            if fold_response.clicked() && !self.folded.remove(&row.id) {
                self.folded.insert(row.id);
            }
        }
        let thumb = Rect::from_min_size(
            egui::pos2(left + 12.0, card.center().y - THUMB.y / 2.0),
            THUMB,
        );
        painter.rect(
            thumb,
            CornerRadius::same(4),
            dim(theme::BASE),
            Stroke::new(1.0, theme::BORDER),
            egui::StrokeKind::Inside,
        );

        // Toggles on the right: visibility, and wobble for paint layers.
        let eye = Rect::from_center_size(
            egui::pos2(card.right() - 8.0 - 10.0, card.center().y),
            Vec2::splat(20.0),
        );
        let eye_response = ui.interact(eye.expand(3.0), response.id.with("eye"), Sense::click());
        let (glyph, ink) = if row.visible {
            (Glyph::EyeOpen, theme::MUTED)
        } else {
            (Glyph::EyeClosed, theme::DISABLED)
        };
        icons::paint(&painter, eye, glyph, ink, 0.0);
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
                    theme::ACCENT
                } else {
                    theme::DISABLED
                },
                0.0,
            );
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

        // "opacity %  blend" over the name, with badges before the name.
        let text_left = thumb.right() + 10.0;
        let percent = tr_with(
            "percent",
            &super::args([("value", format!("{:.0} ", row.opacity * 100.0))]),
        );
        painter.text(
            egui::pos2(text_left, card.center().y - 2.0),
            Align2::LEFT_BOTTOM,
            format!("{percent}  {}", blend_name(row.blend)),
            FontId::proportional(theme::SMALL),
            theme::MUTED,
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
                    theme::ACCENT,
                );
                let width = galley.size().x;
                painter.galley(egui::pos2(x, name_pos.y), galley, theme::ACCENT);
                x += width + 5.0;
            }
            let name_color = if row.visible {
                theme::TEXT
            } else {
                theme::MUTED
            };
            let galley = painter.layout(
                row.name.clone(),
                FontId::proportional(theme::BODY),
                name_color,
                (text_right - x).max(10.0),
            );
            painter.galley(egui::pos2(x, name_pos.y), galley, name_color);
        }

        let toggles_hit = eye_response.hovered() || ui.ctx().is_being_dragged(eye_response.id);
        if response.clicked() && !toggles_hit {
            let id = row.id;
            canvas.edit(|session| session.select_layer(id));
        }
        if response.double_clicked()
            || (selected
                && response.has_focus()
                && ui.input(|input| input.key_pressed(egui::Key::F2)))
        {
            self.renaming = Some((row.id, row.name.clone()));
        }
        if selected && response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Space))
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
                painter.rect_filled(line(rect.top()), CornerRadius::same(1), theme::ACCENT)
            }
            Drop::Below(_) => {
                painter.rect_filled(line(rect.bottom()), CornerRadius::same(1), theme::ACCENT)
            }
            Drop::Into(_) => painter.rect_stroke(
                card,
                CornerRadius::same(7),
                Stroke::new(2.0, theme::ACCENT),
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
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.add_space(4.0);
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
            let merge = widgets::icon_button(
                ui,
                Glyph::MoveDown,
                16.0,
                merge_refused.unwrap_or(tr("layer-merge")),
                merge_refused.is_none(),
            );
            if merge.clicked() {
                report(refusal, canvas.edit(Session::merge_down));
            }
            if widgets::icon_button(ui, Glyph::Remove, 16.0, tr("layer-delete"), removable)
                .clicked()
            {
                report(refusal, canvas.edit(Session::remove_layer));
            }
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

        egui::Grid::new("layer-properties")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                widgets::field_label(ui, tr("blend-mode"));
                let mut chosen = blend;
                egui::ComboBox::from_id_salt("layer-blend")
                    .width(ui.available_width())
                    .selected_text(blend_name(blend))
                    .show_ui(ui, |ui| {
                        for each in BLENDS {
                            ui.selectable_value(&mut chosen, each, blend_name(each));
                        }
                    });
                ui.end_row();
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

                widgets::field_label(ui, tr("group"));
                let mut target = parent;
                let shown = parent
                    .and_then(|id| groups.iter().find(|(group, _)| *group == id))
                    .map_or(tr("group-none"), |(_, name)| name.as_str());
                egui::ComboBox::from_id_salt("layer-group")
                    .width(ui.available_width())
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut target, None, tr("group-none"));
                        for (id, name) in &groups {
                            ui.selectable_value(&mut target, Some(*id), name);
                        }
                    });
                ui.end_row();
                if target != parent {
                    report(
                        refusal,
                        canvas.edit(|session| session.move_to_group(target)),
                    );
                }
            });

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
        let slider = ui
            .horizontal(|ui| {
                widgets::field_label(ui, tr("opacity"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(tr_with(
                        "percent",
                        &super::args([("value", format!("{percent:.0}"))]),
                    ));
                    let width = ui.available_width().max(40.0);
                    widgets::slider(ui, &mut percent, 0.0..=100.0, tr("opacity"), width)
                })
                .inner
            })
            .inner;
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
        ui.painter()
            .rect_filled(rect, CornerRadius::same(7), theme::HOVER);
    }
    let ink = if enabled {
        theme::TEXT
    } else {
        theme::DISABLED
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
