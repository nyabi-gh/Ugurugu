// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The brush cursor, as 2.2.13 draws it (`ui/CanvasWidgetEvents.cpp`): with
//! the brush or the eraser, or a pen's eraser end, a circle as wide as the
//! stroke takes the pointer's place, dashed for erasing. A pixel brush shows
//! the outline of the pixels a press would cover instead.

use ugu_core::store::BrushEngine;
use ugu_session::Tool;

use super::{Canvas, Interaction};

pub enum BrushCursor {
    /// Centre and radius, in client physical pixels.
    Circle {
        centre: [f64; 2],
        radius: f64,
        eraser: bool,
    },
    /// The pixels' outline, in client physical pixels.
    Pixels {
        outline: Vec<[f64; 2]>,
        eraser: bool,
    },
}

impl Canvas {
    /// Whether the pen over the canvas shows its eraser end.
    pub fn set_hover_eraser(&mut self, eraser: bool) {
        self.hover_eraser = eraser;
    }

    /// The brush cursor for the pointer at `pointer` (client physical
    /// pixels), or at the stroke's last point while one is drawn; `None`
    /// when a press would not draw.
    pub fn brush_cursor(&self, pointer: [f64; 2]) -> Option<BrushCursor> {
        let position = match self.interaction {
            Interaction::Idle if !self.picks() => pointer,
            Interaction::Drawing { .. } => self.drawn_at?,
            _ => return None,
        };
        let session = &self.session;
        if session.pending().is_some() || session.tool() == Tool::Text {
            return None;
        }
        let eraser = self.hover_eraser || session.tool() == Tool::Eraser;
        if !eraser && session.tool() != Tool::Pen {
            return None;
        }
        let settings = if eraser {
            &session.eraser
        } else {
            &session.pen
        };
        let brush = settings.preset.brush;
        if brush.engine == BrushEngine::Pixel {
            let [x, y] = self.to_document(position).map(f64::floor);
            let outline = ugu_render::pixel::tip_outline(settings.width, brush.tip)
                .into_iter()
                .map(|[dx, dy]| self.to_client([x + f64::from(dx), y + f64::from(dy)]))
                .collect();
            return Some(BrushCursor::Pixels { outline, eraser });
        }
        Some(BrushCursor::Circle {
            centre: position,
            radius: (f64::from(settings.width) * self.scale / 2.0).max(1.0),
            eraser,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{CanvasInput, Gesture};
    use ugu_core::document::Document;
    use ugu_win::clock::Ticks;
    use ugu_win::pointer::{Buttons, PointerKind, PointerSample};

    fn canvas() -> Canvas {
        let mut canvas = Canvas::new(Document::new([320, 200]), |_| {});
        canvas.set_area([0, 0, 640, 400]);
        canvas
    }

    #[test]
    fn the_cursor_is_as_wide_as_the_stroke_and_dashed_for_erasing() {
        let mut canvas = canvas();
        let scale = canvas.scale;
        let width = f64::from(canvas.session().pen.width);
        let Some(BrushCursor::Circle {
            centre,
            radius,
            eraser,
        }) = canvas.brush_cursor([100.0, 80.0])
        else {
            panic!("a circle");
        };
        assert_eq!((centre, eraser), ([100.0, 80.0], false));
        assert!((radius - width * scale / 2.0).abs() < 1e-9);
        canvas.set_hover_eraser(true);
        assert!(matches!(
            canvas.brush_cursor([100.0, 80.0]),
            Some(BrushCursor::Circle { eraser: true, .. })
        ));
        canvas.set_hover_eraser(false);
        canvas.edit(|session| session.set_tool(Tool::Fill)).unwrap();
        assert!(canvas.brush_cursor([100.0, 80.0]).is_none());
    }

    #[test]
    fn a_pixel_brush_outlines_the_pixels_a_press_covers_and_follows_the_stroke() {
        let mut canvas = canvas();
        let pixel = ugu_core::brush::find("pixel-pencil").unwrap();
        canvas.edit(|session| {
            session.pen.preset = pixel;
            session.pen.width = 1.0;
        });
        let pointer = canvas.to_client([10.4, 20.7]);
        let Some(BrushCursor::Pixels { outline, .. }) = canvas.brush_cursor(pointer) else {
            panic!("pixels");
        };
        let corners: Vec<[f64; 2]> = outline
            .iter()
            .map(|&client| canvas.to_document(client).map(f64::round))
            .collect();
        assert_eq!(
            corners,
            [[11.0, 20.0], [11.0, 21.0], [10.0, 21.0], [10.0, 20.0]]
        );

        let sample = |position| PointerSample {
            position,
            pressure: None,
            tilt: None,
            twist: None,
            buttons: Buttons::PRIMARY,
            in_contact: true,
            inverted: false,
            time: Ticks(0),
            frame_id: 0,
        };
        canvas.apply(CanvasInput::Begin(
            Gesture::Draw,
            PointerKind::Mouse,
            sample(pointer),
        ));
        let later = canvas.to_client([30.5, 40.5]);
        canvas.apply(CanvasInput::Extend(sample(later)));
        let Some(BrushCursor::Pixels { outline, .. }) = canvas.brush_cursor(pointer) else {
            panic!("pixels while drawing");
        };
        assert_eq!(canvas.to_document(outline[3]).map(f64::round), [30.0, 40.0]);
    }
}
