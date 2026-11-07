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
    /// pixels), at the stroke's last point while one is drawn, or where a
    /// drag sizing the brush began, showing the size it sets; `None` when a
    /// press would not draw.
    pub fn brush_cursor(&self, pointer: [f64; 2]) -> Option<BrushCursor> {
        if self.sizing.is_some() {
            return None;
        }
        let (position, eraser_end) = match self.interaction {
            Interaction::Idle if !self.picks() => (pointer, self.hover_eraser),
            Interaction::Drawing { .. } => (self.drawn_at?, self.hover_eraser),
            Interaction::BrushSizing { start, .. } => (start, false),
            _ => return None,
        };
        let tool = self.tools.tool;
        let sizing_brush = matches!(self.interaction, Interaction::BrushSizing { .. });
        if self.tab.session.pending().is_some() && !sizing_brush || tool == Tool::Text {
            return None;
        }
        let eraser = eraser_end || tool == Tool::Eraser;
        if !eraser && !matches!(tool, Tool::Pen | Tool::Shape) {
            return None;
        }
        let settings = if eraser {
            &self.brushes().eraser
        } else {
            &self.brushes().pen
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
            radius: (f64::from(settings.width) * self.tab.scale / 2.0).max(1.0),
            eraser,
        })
    }

    /// The straight line a press at `pointer` would start its stroke with,
    /// from where the last stroke lifted to `pointer`, in client physical
    /// pixels; `None` unless the press would draw a joined stroke.
    pub fn join_guide(&self, pointer: [f64; 2]) -> Option<[[f64; 2]; 2]> {
        if !self.joins()
            || !matches!(self.interaction, Interaction::Idle)
            || self.sizing.is_some()
            || self.picks()
            || self.tab.session.pending().is_some()
        {
            return None;
        }
        // The tools whose press `apply` makes a stroke.
        let strokes = match self.tools.tool {
            Tool::Pen | Tool::Eraser => true,
            Tool::Shape => self.hover_eraser,
            _ => false,
        };
        let end = self.tab.session.stroke_end().filter(|_| strokes)?;
        Some([self.to_client(end), pointer])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{CanvasInput, Gesture};
    use ugu_core::document::Document;
    use ugu_platform::clock::Ticks;
    use ugu_platform::pointer::{Buttons, PointerKind, PointerSample};

    fn canvas() -> Canvas {
        let mut canvas = Canvas::new(Document::new([320, 200]), |_| {});
        canvas.set_area([0, 0, 640, 400]);
        canvas
    }

    #[test]
    fn the_cursor_is_as_wide_as_the_stroke_and_dashed_for_erasing() {
        let mut canvas = canvas();
        let scale = canvas.tab.scale;
        let width = f64::from(canvas.brushes().pen.width);
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
        canvas.set_tool(Tool::Fill).unwrap();
        assert!(canvas.brush_cursor([100.0, 80.0]).is_none());
    }

    #[test]
    fn a_pixel_brush_outlines_the_pixels_a_press_covers_and_follows_the_stroke() {
        let mut canvas = canvas();
        let pixel = ugu_core::brush::find("pixel-pencil").unwrap();
        canvas.choose_preset(Tool::Pen, pixel);
        canvas.brushes_mut().pen.width = 1.0;
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

    #[test]
    fn pixel_mode_draws_with_its_own_brushes_and_another_document_with_the_plain_ones() {
        let mut canvas = canvas();
        let plain = canvas.brushes().clone();
        let pointer = canvas.to_client([10.4, 20.7]);
        canvas.edit(|session| session.set_pixel_mode(true)).unwrap();
        assert!(matches!(
            canvas.brush_cursor(pointer),
            Some(BrushCursor::Pixels { .. })
        ));
        canvas.brushes_mut().pen.width = 4.0;
        canvas.replace(Document::new([320, 200]), true);
        assert_eq!(canvas.brushes(), &plain);
        assert!(matches!(
            canvas.brush_cursor(pointer),
            Some(BrushCursor::Circle { .. })
        ));
        let mut pixel = Document::new([320, 200]);
        pixel.pixel = true;
        canvas.replace(pixel, true);
        assert_eq!(canvas.brushes().pen.width, 4.0);
    }
}
