// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The outlines of placed text: Parley lays the lines out with the
//! installed fonts, falling back for characters a font lacks, and Skrifa
//! gives each glyph's contours, flattened to polygons in document pixels.
//! The document keeps only what is drawn from them, so the text looks the
//! same on a PC without the font.

use kurbo::{BezPath, PathEl};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, Layout, LayoutContext,
    PositionedLayoutItem, StyleProperty,
};
use skrifa::instance::{LocationRef, NormalizedCoord, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};
use ugu_core::text::Outline;

/// How far a flattened contour may stray from the glyph's curves, in
/// document pixels.
const TOLERANCE: f64 = 0.2;

/// The fonts installed and the scratch space for laying text out.
pub struct Fonts {
    fonts: FontContext,
    layouts: LayoutContext<()>,
    /// Families tried after the chosen one, for characters it lacks.
    fallback: &'static str,
}

impl Fonts {
    /// `language` (as "ko" or "ja") orders the fallback, so kana and kanji
    /// take that language's forms.
    pub fn new(language: &str) -> Self {
        let fallback = match language {
            "ko" => "\"Malgun Gothic\", \"Yu Gothic UI\", sans-serif",
            "ja" => "\"Yu Gothic UI\", Meiryo, \"Malgun Gothic\", sans-serif",
            _ => "\"Segoe UI\", \"Yu Gothic UI\", \"Malgun Gothic\", sans-serif",
        };
        Self {
            fonts: FontContext::new(),
            layouts: LayoutContext::new(),
            fallback,
        }
    }

    /// The installed font families, sorted.
    pub fn families(&mut self) -> Vec<String> {
        let mut names: Vec<String> = self
            .fonts
            .collection
            .family_names()
            .map(str::to_owned)
            .collect();
        names.sort_by_key(|name| name.to_lowercase());
        names.dedup();
        names
    }

    /// `text` in `family` (the fallback alone when `None`) at `size` pixels
    /// per em, one line per `\n`.
    pub fn outline(&mut self, text: &str, family: Option<&str>, size: f32) -> Outline {
        let stack = match family {
            Some(family) => format!("\"{}\", {}", family.replace('"', ""), self.fallback),
            None => self.fallback.to_owned(),
        };
        let mut builder = self
            .layouts
            .ranged_builder(&mut self.fonts, text, 1.0, false);
        builder.push_default(StyleProperty::FontFamily(FontFamily::Source(stack.into())));
        builder.push_default(StyleProperty::FontSize(size));
        let mut layout: Layout<()> = builder.build(text);
        layout.break_all_lines(None);
        layout.align(Alignment::Start, AlignmentOptions::default());
        let mut path = BezPath::new();
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyphs) = item else {
                    continue;
                };
                let run = glyphs.run();
                let font = run.font();
                let Ok(font) = FontRef::from_index(font.data.as_ref(), font.index) else {
                    continue;
                };
                let outlines = font.outline_glyphs();
                let coords: Vec<NormalizedCoord> = run
                    .normalized_coords()
                    .iter()
                    .map(|&coord| NormalizedCoord::from_bits(coord))
                    .collect();
                let size = Size::new(run.font_size());
                for glyph in glyphs.positioned_glyphs() {
                    if let Some(outline) = outlines.get(GlyphId::new(glyph.id)) {
                        let mut pen = Pen {
                            path: &mut path,
                            at: [f64::from(glyph.x), f64::from(glyph.y)],
                        };
                        // A glyph that fails to draw is left out.
                        let _ = outline.draw(
                            DrawSettings::unhinted(size, LocationRef::new(&coords)),
                            &mut pen,
                        );
                    }
                }
            }
        }
        Outline {
            contours: flatten(&path),
            size: [f64::from(layout.width()), f64::from(layout.height())],
        }
    }
}

/// Puts a glyph's contours at its place on the baseline, y down.
struct Pen<'a> {
    path: &'a mut BezPath,
    at: [f64; 2],
}

impl Pen<'_> {
    fn point(&self, x: f32, y: f32) -> (f64, f64) {
        (self.at[0] + f64::from(x), self.at[1] - f64::from(y))
    }
}

impl OutlinePen for Pen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to(self.point(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(self.point(x, y));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.path.quad_to(self.point(cx0, cy0), self.point(x, y));
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.path
            .curve_to(self.point(cx0, cy0), self.point(cx1, cy1), self.point(x, y));
    }

    fn close(&mut self) {
        self.path.close_path();
    }
}

/// The path's contours as polygons, dropping repeated points and ones of
/// fewer than three points.
fn flatten(path: &BezPath) -> Vec<Vec<[f64; 2]>> {
    let mut contours = Vec::new();
    let mut current: Vec<[f64; 2]> = Vec::new();
    let mut finish = |current: &mut Vec<[f64; 2]>| {
        if current.len() > 1 && current.first() == current.last() {
            current.pop();
        }
        if current.len() >= 3 {
            contours.push(std::mem::take(current));
        }
        current.clear();
    };
    kurbo::flatten(path, TOLERANCE, |element| match element {
        PathEl::MoveTo(point) => {
            finish(&mut current);
            current.push([point.x, point.y]);
        }
        PathEl::LineTo(point) => {
            if current.last() != Some(&[point.x, point.y]) {
                current.push([point.x, point.y]);
            }
        }
        PathEl::ClosePath => finish(&mut current),
        PathEl::QuadTo(..) | PathEl::CurveTo(..) => unreachable!("flattened"),
    });
    finish(&mut current);
    contours
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Twice the signed area: positive when the contour turns clockwise on
    /// screen (y down).
    fn area(contour: &[[f64; 2]]) -> f64 {
        contour
            .iter()
            .zip(contour.iter().cycle().skip(1))
            .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
            .sum()
    }

    #[test]
    fn letters_become_closed_contours_below_and_right_of_the_origin() {
        let mut fonts = Fonts::new("en");
        let outline = fonts.outline("Ug", None, 64.0);
        assert!(outline.contours.len() >= 2);
        assert!(outline.size[0] > 40.0 && outline.size[1] > 64.0);
        for contour in &outline.contours {
            assert!(contour.len() >= 3);
            assert_ne!(contour.first(), contour.last());
            for &[x, y] in contour {
                assert!((0.0..=outline.size[0] + 1.0).contains(&x), "{x}");
                assert!((0.0..=outline.size[1]).contains(&y), "{y}");
            }
        }
        // The same text gives the same contours.
        assert_eq!(fonts.outline("Ug", None, 64.0), outline);
    }

    #[test]
    fn a_counter_runs_the_other_way_round() {
        let mut fonts = Fonts::new("en");
        let outline = fonts.outline("o", None, 100.0);
        let [outer, inner] = &outline.contours[..] else {
            panic!("an o has two contours");
        };
        assert!(area(outer).signum() != area(inner).signum());
    }

    #[test]
    fn each_line_starts_lower_and_empty_lines_count() {
        let mut fonts = Fonts::new("ko");
        let lowest = |outline: &Outline| {
            outline
                .contours
                .iter()
                .flatten()
                .map(|point| point[1])
                .fold(f64::MIN, f64::max)
        };
        let one = fonts.outline("가", None, 40.0);
        let two = fonts.outline("가\n가", None, 40.0);
        let three = fonts.outline("가\n\n가", None, 40.0);
        assert_eq!(two.contours.len(), 2 * one.contours.len());
        let line = lowest(&two) - lowest(&one);
        assert!(line > 40.0);
        assert!((lowest(&three) - lowest(&two) - line).abs() < 0.5);
    }

    #[test]
    fn characters_a_font_lacks_come_from_the_fallback() {
        let mut fonts = Fonts::new("ko");
        let latin = fonts.outline("A", Some("Arial"), 40.0);
        let both = fonts.outline("A한", Some("Arial"), 40.0);
        assert!(both.contours.len() > latin.contours.len());
        assert!(fonts.families().iter().any(|family| family == "Arial"));
    }
}
