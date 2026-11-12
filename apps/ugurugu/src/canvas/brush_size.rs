// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The brush size as `[` and `]` step it and as a drag on the canvas sets
//! it: Alt and the secondary button, as in Photoshop.

use ugu_session::Tool;

use super::{Canvas, Interaction};

/// The sizes `[` and `]` step through, closer together where a pixel more
/// shows; their ends bound the size slider and the drag. The last is the
/// widest stroke a document holds (`store::limits::STROKE_WIDTH`).
pub const BRUSH_SIZES: [f32; 32] = [
    1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 10.0, 12.0, 14.0, 16.0, 20.0, 24.0, 28.0, 32.0, 40.0,
    48.0, 56.0, 64.0, 80.0, 96.0, 112.0, 128.0, 160.0, 192.0, 224.0, 256.0, 320.0, 384.0, 448.0,
    512.0,
];

impl Canvas {
    /// Whether the tool has a size for `[`, `]` and the drag to change.
    pub fn has_brush_size(&self) -> bool {
        matches!(self.tools.tool, Tool::Pen | Tool::Eraser | Tool::Shape)
    }

    /// Sets the tool's size to the next of `BRUSH_SIZES` up or down from it.
    pub fn step_brush_size(&mut self, larger: bool) {
        let tool = self.tools.tool;
        let width = self.brushes().get(tool).width;
        let next = if larger {
            BRUSH_SIZES.iter().copied().find(|&size| size > width)
        } else {
            BRUSH_SIZES.iter().rev().copied().find(|&size| size < width)
        };
        if let Some(next) = next {
            self.brushes_mut().get_mut(tool).width = next;
        }
    }

    /// The size being dragged out, while it is.
    pub fn brush_size_dragged(&self) -> Option<f32> {
        match self.interaction {
            Interaction::BrushSizing { .. } => Some(self.brushes().get(self.tools.tool).width),
            _ => None,
        }
    }

    /// Starts sizing the brush by dragging from `position` (client physical
    /// pixels); a tool without a size ignores the drag.
    pub(super) fn begin_brush_sizing(&mut self, position: [f64; 2]) {
        if self.has_brush_size() {
            self.interaction = Interaction::BrushSizing {
                start: position,
                base: self.brushes().get(self.tools.tool).width,
            };
        }
    }

    /// Sizes the brush for the drag at `position`: the ring on screen grows
    /// by as much as the pointer moved right of where the drag began, and
    /// shrinks as it moves left. Sizes are whole pixels.
    pub(super) fn drag_brush_size(&mut self, position: [f64; 2]) {
        let Interaction::BrushSizing { start, base } = self.interaction else {
            return;
        };
        let moved = position[0] - start[0];
        let width = if moved == 0.0 {
            base
        } else {
            let wanted = f64::from(base) + moved / self.tab.scale;
            (wanted.round() as f32).clamp(BRUSH_SIZES[0], BRUSH_SIZES[BRUSH_SIZES.len() - 1])
        };
        let tool = self.tools.tool;
        self.brushes_mut().get_mut(tool).width = width;
    }

    /// Puts the size back as it was when the drag began.
    pub(super) fn undrag_brush_size(&mut self, base: f32) {
        let tool = self.tools.tool;
        self.brushes_mut().get_mut(tool).width = base;
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

    fn at(x: f64, y: f64) -> PointerSample {
        PointerSample {
            position: [x, y],
            pressure: None,
            tilt: None,
            twist: None,
            buttons: Buttons::SECONDARY,
            in_contact: true,
            inverted: false,
            time: Ticks(0),
            frame_id: 0,
        }
    }

    #[test]
    fn a_drag_sideways_sizes_the_brush_as_the_ring_on_screen_grows() {
        let mut canvas = canvas();
        let scale = canvas.tab.scale;
        canvas.brushes_mut().pen.width = 10.0;
        canvas.apply(CanvasInput::Begin(
            Gesture::BrushSize,
            PointerKind::Mouse,
            at(100.0, 80.0),
        ));
        assert_eq!(canvas.brush_size_dragged(), Some(10.0));
        // Ten document pixels to the right, and up or down changes nothing.
        canvas.apply(CanvasInput::Extend(at(100.0 + 10.0 * scale, 20.0)));
        assert_eq!(canvas.brushes().pen.width, 20.0);
        canvas.apply(CanvasInput::Extend(at(100.0 - 4.4 * scale, 80.0)));
        assert_eq!(canvas.brushes().pen.width, 6.0);
        // The ring stays where the drag began, as wide as the size.
        let Some(super::super::BrushCursor::Circle { centre, radius, .. }) =
            canvas.brush_cursor([300.0, 300.0])
        else {
            panic!("a circle");
        };
        assert_eq!(centre, [100.0, 80.0]);
        assert!((radius - 6.0 * scale / 2.0).abs() < 1e-9);
        // Within the sizes the slider offers.
        canvas.apply(CanvasInput::Extend(at(100.0 - 1000.0 * scale, 80.0)));
        assert_eq!(canvas.brushes().pen.width, 1.0);
        canvas.apply(CanvasInput::Extend(at(100.0 + 1000.0 * scale, 80.0)));
        assert_eq!(canvas.brushes().pen.width, 512.0);
        canvas.apply(CanvasInput::End(at(100.0 + 3.0 * scale, 80.0)));
        assert_eq!(canvas.brushes().pen.width, 13.0);
        assert_eq!(canvas.brush_size_dragged(), None);
        // The drag drew nothing.
        assert!(canvas.session().document().store.strokes.is_empty());
    }

    #[test]
    fn the_sizes_rise_to_the_widest_stroke_a_document_holds() {
        assert!(BRUSH_SIZES.windows(2).all(|pair| pair[0] < pair[1]));
        let widest = *ugu_core::store::limits::STROKE_WIDTH.end();
        assert_eq!(BRUSH_SIZES[BRUSH_SIZES.len() - 1], widest);
        let mut canvas = canvas();
        canvas.brushes_mut().pen.width = 128.0;
        canvas.step_brush_size(true);
        assert_eq!(canvas.brushes().pen.width, 160.0);
        for _ in 0..BRUSH_SIZES.len() {
            canvas.step_brush_size(true);
        }
        assert_eq!(canvas.brushes().pen.width, widest);
    }

    #[test]
    fn a_size_drag_sizes_the_tools_own_brush_and_esc_puts_it_back() {
        let mut canvas = canvas();
        let scale = canvas.tab.scale;
        canvas.set_tool(Tool::Eraser).unwrap();
        let [pen, eraser] = [canvas.brushes().pen.width, canvas.brushes().eraser.width];
        canvas.apply(CanvasInput::Begin(
            Gesture::BrushSize,
            PointerKind::Mouse,
            at(100.0, 80.0),
        ));
        canvas.apply(CanvasInput::Extend(at(100.0 + 5.0 * scale, 80.0)));
        assert_eq!(canvas.brushes().eraser.width, eraser + 5.0);
        assert_eq!(canvas.brushes().pen.width, pen);
        canvas.escape();
        assert_eq!(canvas.brushes().eraser.width, eraser);
        assert_eq!(canvas.brush_size_dragged(), None);
    }

    #[test]
    fn a_tool_without_a_size_ignores_the_drag() {
        let mut canvas = canvas();
        let scale = canvas.tab.scale;
        canvas.set_tool(Tool::Fill).unwrap();
        let width = canvas.brushes().pen.width;
        canvas.apply(CanvasInput::Begin(
            Gesture::BrushSize,
            PointerKind::Mouse,
            at(100.0, 80.0),
        ));
        canvas.apply(CanvasInput::Extend(at(100.0 + 5.0 * scale, 80.0)));
        canvas.apply(CanvasInput::End(at(100.0 + 5.0 * scale, 80.0)));
        assert_eq!(canvas.brushes().pen.width, width);
        assert_eq!(canvas.brush_size_dragged(), None);
    }
}
