// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The navigator, as Clip Studio Paint's: the whole canvas small, the part
//! of it in view outlined, and the zoom and the angle. Pressing on the
//! picture brings that point to the middle of the view, and dragging the
//! outline moves the view with it. The render thread draws the picture
//! (`ugu_render::overview`) at the size asked for here.

use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};

use crate::canvas::Canvas;
use crate::i18n::tr;
use crate::{theme, widgets};

/// The picture is not made lower than this.
const PICTURE_LEAST: f32 = 80.0;
/// The zoom and angle rows under the picture.
const ROWS: f32 = 2.0 * theme::CONTROL_HEIGHT + 2.0 * theme::space::S;

#[derive(Default)]
pub struct Navigator {
    /// The picture's size in physical pixels, asked for by this frame; none
    /// while the panel is not shown.
    pub wanted: Option<[u32; 2]>,
    /// The picture the render thread drew last, and its size.
    pub picture: Option<(egui::TextureId, [u32; 2])>,
    /// While the picture is pressed: the view's middle less the point
    /// pressed, in document points.
    grab: Option<[f64; 2]>,
}

/// The least height under the panel's head.
pub const LEAST: f32 = PICTURE_LEAST + ROWS;

/// The height under the head that shows the picture as wide as the panel.
pub fn natural_height(width: f32, canvas: &Canvas) -> f32 {
    let [document_width, document_height] = canvas.session().document().canvas;
    let aspect = document_height as f32 / document_width.max(1) as f32;
    (width * aspect.min(1.0)).max(PICTURE_LEAST) + ROWS
}

/// The panel in `room`, or as tall as it would be.
pub fn show(ui: &mut Ui, canvas: &mut Canvas, navigator: &mut Navigator, room: Option<f32>) {
    let width = ui.available_width();
    let height = room.map_or_else(
        || natural_height(width, canvas) - ROWS,
        |room| (room - ROWS).max(PICTURE_LEAST),
    );
    picture(ui, canvas, navigator, vec2(width, height));
    ui.add_space(theme::space::S);
    ui.horizontal(|ui| super::zoom_controls(ui, canvas, (width - 80.0).max(40.0)));
    ui.horizontal(|ui| turn_controls(ui, canvas, (width - 72.0).max(40.0)));
}

fn picture(ui: &mut Ui, canvas: &mut Canvas, navigator: &mut Navigator, size: Vec2) {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, true, tr("navigator-picture"))
    });
    let [r, g, b, _] = theme::workspace();
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::from_rgb(r, g, b));
    let document = canvas.session().document().canvas.map(f64::from);
    let ppp = ui.ctx().pixels_per_point();
    // As many whole physical pixels as fit, so that each shows one of the
    // picture's.
    let room = [rect.width(), rect.height()].map(|edge| f64::from((edge * ppp).floor()).max(1.0));
    let factor = (document[0] / room[0]).max(document[1] / room[1]);
    let pixels = document.map(|edge| (edge / factor).round().max(1.0) as u32);
    navigator.wanted = Some(pixels);
    let shown = vec2(pixels[0] as f32, pixels[1] as f32) / ppp;
    let corner = ((rect.center().to_vec2() * ppp - shown * ppp / 2.0).round() / ppp).to_pos2();
    let image = Rect::from_min_size(corner, shown);
    if let Some((texture, _)) = navigator.picture {
        painter.image(
            texture,
            image,
            Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    let to_screen = |[x, y]: [f64; 2]| {
        pos2(
            image.left() + (x / document[0]) as f32 * image.width(),
            image.top() + (y / document[1]) as f32 * image.height(),
        )
    };
    let to_document = |point: Pos2| {
        [
            f64::from((point.x - image.left()) / image.width()) * document[0],
            f64::from((point.y - image.top()) / image.height()) * document[1],
        ]
    };
    let outline = canvas.view_outline();
    if let Some(corners) = outline {
        let points: Vec<Pos2> = corners.iter().map(|corner| to_screen(*corner)).collect();
        // Dark under the accent, so that it shows over any drawing.
        painter.add(egui::Shape::closed_line(
            points.clone(),
            Stroke::new(3.0, Color32::from_black_alpha(140)),
        ));
        painter.add(egui::Shape::closed_line(
            points,
            Stroke::new(1.5, theme::accent()),
        ));
    }
    let middle = outline.map(|corners| {
        [0, 1].map(|axis| corners.iter().map(|corner| corner[axis]).sum::<f64>() / 4.0)
    });
    let inside = |point: [f64; 2]| outline.is_some_and(|corners| contains(&corners, point));
    if let Some(pointer) = response.hover_pos() {
        ui.ctx().set_cursor_icon(if navigator.grab.is_some() {
            egui::CursorIcon::Grabbing
        } else if inside(to_document(pointer)) {
            egui::CursorIcon::Grab
        } else {
            egui::CursorIcon::PointingHand
        });
    }
    let down = response.is_pointer_button_down_on();
    // The frame the button is released in can hold the last moves too.
    let pointer = if down {
        response.interact_pointer_pos()
    } else if navigator.grab.is_some() {
        ui.input(|input| input.pointer.latest_pos())
    } else {
        None
    };
    if let Some(pointer) = pointer {
        let point = to_document(pointer);
        let grab = *navigator.grab.get_or_insert_with(|| {
            // Where the button went down: the pointer may have moved on in
            // the frame it was pressed in.
            let pressed = ui
                .input(|input| input.pointer.press_origin())
                .map_or(point, to_document);
            match middle {
                Some(middle) if inside(pressed) => [middle[0] - pressed[0], middle[1] - pressed[1]],
                _ => [0.0, 0.0],
            }
        });
        canvas.center_on([point[0] + grab[0], point[1] + grab[1]]);
    }
    if !down {
        navigator.grab = None;
    }
    if response.has_focus() {
        if let Some(middle) = middle {
            let step = document.map(|edge| edge / 20.0);
            let moved = ui.input(|input| {
                let along = |less: egui::Key, more: egui::Key| {
                    f64::from(i8::from(input.key_pressed(more)) - i8::from(input.key_pressed(less)))
                };
                [
                    along(egui::Key::ArrowLeft, egui::Key::ArrowRight),
                    along(egui::Key::ArrowUp, egui::Key::ArrowDown),
                ]
            });
            if moved != [0.0, 0.0] {
                canvas.center_on([
                    middle[0] + moved[0] * step[0],
                    middle[1] + moved[1] * step[1],
                ]);
            }
        }
        widgets::focus_ring(&painter, rect.shrink(1.0), 0);
    }
    response.on_hover_text(tr("navigator-picture"));
}

/// Whether `point` is inside the convex quadrilateral `corners`, given in
/// order around it either way.
fn contains(corners: &[[f64; 2]; 4], point: [f64; 2]) -> bool {
    let sides: Vec<f64> = (0..4)
        .map(|index| {
            let [ax, ay] = corners[index];
            let [bx, by] = corners[(index + 1) % 4];
            (bx - ax) * (point[1] - ay) - (by - ay) * (point[0] - ax)
        })
        .collect();
    sides.iter().all(|side| *side >= 0.0) || sides.iter().all(|side| *side <= 0.0)
}

/// The view's angle as a slider and a field.
fn turn_controls(ui: &mut Ui, canvas: &mut Canvas, slider_width: f32) {
    let mut degrees = canvas.angle() as f32;
    let slider = widgets::slider(
        ui,
        &mut degrees,
        -180.0..=180.0,
        tr("canvas-rotation"),
        slider_width,
    );
    if slider.changed() {
        canvas.turn_to(f64::from(degrees.round()));
    }
    let mut degrees = canvas.angle() as f32;
    let field = widgets::number(
        ui,
        &mut degrees,
        -180.0..=180.0,
        "°",
        0,
        64.0,
        tr("canvas-rotation"),
    )
    .on_hover_text(tr("canvas-rotation"));
    if field.changed() {
        canvas.turn_to(f64::from(degrees));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, PointerButton, RawInput};
    use ugu_core::document::Document;

    /// A navigator 200 points wide at the window's top left beside a canvas
    /// area, at one physical pixel per point.
    struct Window {
        ctx: egui::Context,
        canvas: Canvas,
        navigator: Navigator,
    }

    impl Window {
        fn new(document: [u32; 2]) -> Self {
            Self {
                ctx: egui::Context::default(),
                canvas: Canvas::new(Document::new(document), |_| {}),
                navigator: Navigator::default(),
            }
        }

        fn frame(&mut self, events: Vec<Event>) {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 600.0))),
                events,
                ..RawInput::default()
            };
            let Self {
                ctx,
                canvas,
                navigator,
            } = self;
            let mut output = ctx.run_ui(input, |ui| {
                let canvas_area = Rect::from_min_max(pos2(300.0, 0.0), pos2(1000.0, 600.0));
                ui.scope_builder(egui::UiBuilder::new().max_rect(canvas_area), |ui| {
                    canvas.layout(ui);
                });
                let panel = Rect::from_min_size(Pos2::ZERO, vec2(200.0, 600.0));
                ui.scope_builder(egui::UiBuilder::new().max_rect(panel), |ui| {
                    show(ui, canvas, navigator, None);
                });
            });
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

        /// The document point in the middle of the view.
        fn middle(&self) -> [f64; 2] {
            let corners = self.canvas.view_outline().expect("laid out");
            [0, 1].map(|axis| corners.iter().map(|corner| corner[axis]).sum::<f64>() / 4.0)
        }
    }

    #[test]
    fn a_drag_ends_where_it_is_released_when_the_release_comes_with_the_last_move() {
        // 800 x 400 document pixels in a 200 x 100 picture: 4 per point.
        let mut window = Window::new([800, 400]);
        window.frame(Vec::new());
        window.canvas.zoom_to(4.0);
        window.frame(Vec::new());
        let pressed = pos2(50.0, 50.0);
        window.frame(vec![Event::PointerMoved(pressed)]);
        window.frame(vec![Window::button(pressed, true)]);
        let jumped = window.middle();
        assert!(
            (jumped[0] - 200.0).abs() < 1.0 && (jumped[1] - 200.0).abs() < 1.0,
            "{jumped:?}"
        );
        let released = pos2(120.0, 50.0);
        window.frame(vec![
            Event::PointerMoved(released),
            Window::button(released, false),
        ]);
        let ended = window.middle();
        assert!(
            (ended[0] - 480.0).abs() < 1.0 && (ended[1] - 200.0).abs() < 1.0,
            "{ended:?}"
        );
    }

    #[test]
    fn the_outline_dragged_keeps_the_point_pressed_when_it_moves_in_the_frame_it_is_pressed() {
        let mut window = Window::new([800, 400]);
        window.frame(Vec::new());
        window.canvas.zoom_to(4.0);
        window.frame(Vec::new());
        // Brought to document point 200, 200 first.
        let jump = pos2(50.0, 50.0);
        window.frame(vec![Event::PointerMoved(jump)]);
        window.frame(vec![Window::button(jump, true)]);
        window.frame(vec![Window::button(jump, false)]);
        // Pressed inside the outline, 5 points right of its middle, and
        // moved 30 further before the frame is drawn.
        let pressed = pos2(55.0, 50.0);
        let moved = pos2(85.0, 50.0);
        window.frame(vec![Event::PointerMoved(pressed)]);
        window.frame(vec![
            Window::button(pressed, true),
            Event::PointerMoved(moved),
        ]);
        window.frame(vec![Window::button(moved, false)]);
        let ended = window.middle();
        assert!(
            (ended[0] - 320.0).abs() < 1.0 && (ended[1] - 200.0).abs() < 1.0,
            "{ended:?}"
        );
    }

    #[test]
    fn a_point_is_inside_a_turned_outline_either_way_round() {
        let square = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        let diamond = [[2.0, -1.0], [5.0, 2.0], [2.0, 5.0], [-1.0, 2.0]];
        for corners in [square, diamond] {
            let mut reversed = corners;
            reversed.reverse();
            for outline in [corners, reversed] {
                assert!(contains(&outline, [2.0, 2.0]));
                assert!(!contains(&outline, [6.0, 2.0]));
            }
        }
        assert!(!contains(&diamond, [-0.5, -0.5]));
    }
}
