// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The open documents' tabs over the canvas: click to show one, drag to
//! reorder, the cross or a middle click to close.

use egui::{CornerRadius, FontId, Rect, Response, Sense, Ui, vec2};

use super::Panels;
use crate::canvas::{Canvas, TabInfo};
use crate::files::{self, Files};
use crate::i18n::tr_with;
use crate::icons::{self, Glyph};
use crate::{theme, widgets};

/// How tall the bar is, its margin included.
pub const HEIGHT: f32 = theme::CONTROL_HEIGHT + 2.0 * theme::space::XS;
const NARROWEST: f32 = 96.0;
const WIDEST: f32 = 220.0;
/// The close button's side.
const CLOSE: f32 = 20.0;

pub fn tab_bar(ui: &mut Ui, canvas: &mut Canvas, files: &mut Files, panels: &mut Panels) {
    let infos = canvas.tab_infos();
    let shown = canvas.tab_position();
    let mut rects = Vec::with_capacity(infos.len());
    let mut chosen = None;
    let mut closed = None;
    egui::ScrollArea::horizontal()
        .id_salt("document tabs")
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme::space::XXS;
                for (index, info) in infos.iter().enumerate() {
                    let (response, close) = tab(ui, info, index == shown);
                    if index == shown && canvas.tab_count() > 1 {
                        response.scroll_to_me(None);
                    }
                    if response.clicked() {
                        chosen = Some(index);
                    }
                    if close || response.middle_clicked() {
                        closed = Some(index);
                    }
                    if response.drag_started() {
                        panels.tab_drag = Some(index);
                    }
                    rects.push(response.rect);
                }
            });
        });
    if let Some(from) = panels.tab_drag {
        if ui.input(|input| input.pointer.primary_down()) {
            let pointer = ui.input(|input| input.pointer.interact_pos());
            let over =
                pointer.and_then(|at| rects.iter().position(|rect| rect.x_range().contains(at.x)));
            if let Some(to) = over.filter(|&to| to != from) {
                canvas.move_tab(from, to);
                panels.tab_drag = Some(to);
            }
        } else {
            panels.tab_drag = None;
        }
    }
    if let Some(index) = chosen {
        let _ = canvas.show_tab(index);
    }
    // Asked about there, if it has unsaved changes.
    if let Some(index) = closed
        && canvas.show_tab(index)
    {
        files.request(files::Action::CloseTab, canvas);
    }
}

/// What the close button of the tab of document `name` is called, so that a
/// screen reader tells the tabs' buttons apart.
pub fn close_name(name: &str) -> String {
    let mut args = fluent_bundle::FluentArgs::new();
    args.set("name", name.to_owned());
    tr_with("tab-close-named", &args)
}

/// One tab: the document's name, marked when it has unsaved changes, and a
/// close button while it is shown or under the pointer. Returns it and
/// whether its close button was clicked.
fn tab(ui: &mut Ui, info: &TabInfo, current: bool) -> (Response, bool) {
    let padding = theme::space::L;
    let title = if info.dirty {
        format!("{}*", info.name)
    } else {
        info.name.clone()
    };
    let ink = if current {
        theme::text()
    } else {
        theme::muted()
    };
    let mut job = egui::text::LayoutJob::simple_singleline(
        title.clone(),
        FontId::proportional(theme::BODY),
        ink,
    );
    job.wrap = egui::text::TextWrapping::from_wrap_mode_and_width(
        egui::TextWrapMode::Truncate,
        WIDEST - 2.0 * padding - CLOSE,
    );
    let galley = ui.painter().layout_job(job);
    let cut = galley.elided;
    let width = (padding + galley.size().x + theme::space::XS + CLOSE + theme::space::XS)
        .clamp(NARROWEST, WIDEST);
    let (rect, response) =
        ui.allocate_exact_size(vec2(width, theme::CONTROL_HEIGHT), Sense::click_and_drag());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, current, &title)
    });
    let hovered = response.hovered();
    let painter = ui.painter();
    let corner = CornerRadius::same(theme::radius::CONTROL);
    if current {
        painter.rect_filled(rect, corner, theme::control());
        let bar = Rect::from_min_max(
            egui::pos2(rect.left() + padding, rect.bottom() - 2.0),
            egui::pos2(rect.right() - padding, rect.bottom()),
        );
        painter.rect_filled(bar, CornerRadius::same(1), theme::accent_ink());
    } else if hovered {
        painter.rect_filled(rect, corner, theme::hover());
    }
    painter.galley(
        egui::pos2(
            rect.left() + padding,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        ink,
    );
    if response.has_focus() {
        widgets::focus_ring(painter, rect, theme::radius::CONTROL);
    }
    let cross = Rect::from_center_size(
        egui::pos2(
            rect.right() - theme::space::XS - CLOSE / 2.0,
            rect.center().y,
        ),
        vec2(CLOSE, CLOSE),
    );
    let close = ui.interact(cross, response.id.with("close"), Sense::click());
    let close_name = close_name(&info.name);
    close.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, close_name.as_str())
    });
    if current || hovered || close.hovered() || close.has_focus() {
        if close.hovered() {
            ui.painter().rect_filled(
                cross,
                CornerRadius::same(theme::radius::SMALL),
                theme::hover(),
            );
        }
        icons::paint(
            ui.painter(),
            Rect::from_center_size(cross.center(), vec2(12.0, 12.0)),
            Glyph::Cancel,
            theme::muted(),
            0.0,
        );
        if close.has_focus() {
            widgets::focus_ring(ui.painter(), cross, theme::radius::SMALL);
        }
    }
    let clicked = close.on_hover_text(close_name).clicked();
    let response = if cut {
        response.on_hover_text(title)
    } else {
        response
    };
    (response, clicked)
}
