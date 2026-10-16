// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The tools and the colour history in the settings file, as 2.2.13 keeps its
//! `drawingTools` keys and colour history. A value out of range is brought
//! into it, an unknown one is left at its default, and either is logged.

use serde_json::{Map, Value, json};
use ugu_core::brush::{self, Preset};
use ugu_core::ops::{Rgba8, Sampling};
use ugu_core::store::limits::STROKE_WIDTH;
use ugu_session::{
    Brushes, ColorHistory, Kept, Reads, Shape, ShapeKind, ShapePaint, ShapeSettings, TextSettings,
    Tool, ToolSettings, Tools, is_pixel,
};

const TOOLS: [(Tool, &str); 10] = [
    (Tool::Pen, "brush"),
    (Tool::Eraser, "eraser"),
    (Tool::Select, "select"),
    (Tool::Wand, "wand"),
    (Tool::Fill, "fill"),
    (Tool::Text, "text"),
    (Tool::Eyedropper, "eyedropper"),
    (Tool::Shape, "shape"),
    (Tool::Move, "move"),
    (Tool::Symmetry, "symmetry"),
];
const SHAPES: [(ShapeKind, &str); 3] = [
    (ShapeKind::Freehand, "freehand"),
    (ShapeKind::Rectangle, "rectangle"),
    (ShapeKind::Ellipse, "ellipse"),
];
const FIGURES: [(Shape, &str); 9] = [
    (Shape::Line, "line"),
    (Shape::Rectangle, "rectangle"),
    (Shape::Ellipse, "ellipse"),
    (Shape::Polygon, "polygon"),
    (Shape::Star, "star"),
    (Shape::Parallelogram, "parallelogram"),
    (Shape::Trapezoid, "trapezoid"),
    (Shape::Semicircle, "semicircle"),
    (Shape::Moon, "moon"),
];
const SHAPE_PAINTS: [(ShapePaint, &str); 3] = [
    (ShapePaint::Outline, "outline"),
    (ShapePaint::Fill, "fill"),
    (ShapePaint::Both, "both"),
];
const SAMPLINGS: [(Sampling, &str); 2] =
    [(Sampling::Smooth, "smooth"), (Sampling::Nearest, "nearest")];
const READS: [(Reads, &str); 3] = [
    (Reads::Current, "current"),
    (Reads::Marked, "marked"),
    (Reads::Visible, "visible"),
];

fn name<T: PartialEq + Copy>(table: &[(T, &'static str)], value: T) -> &'static str {
    table
        .iter()
        .find(|(each, _)| *each == value)
        .map(|(_, name)| *name)
        .expect("every value is named")
}

/// Reads `key` of `object` as one of `table`'s names into `into`.
fn named<T: Copy>(object: &Map<String, Value>, key: &str, table: &[(T, &str)], into: &mut T) {
    let Some(value) = object.get(key) else {
        return;
    };
    match table.iter().find(|(_, name)| value.as_str() == Some(*name)) {
        Some((found, _)) => *into = *found,
        None => tracing::warn!(key, %value, "unknown tool setting"),
    }
}

fn boolean(object: &Map<String, Value>, key: &str, into: &mut bool) {
    match object.get(key) {
        None => {}
        Some(Value::Bool(value)) => *into = *value,
        Some(value) => tracing::warn!(key, %value, "a tool setting is not true or false"),
    }
}

/// Reads `key` as a number brought into `range`.
fn number(
    object: &Map<String, Value>,
    key: &str,
    range: std::ops::RangeInclusive<f32>,
) -> Option<f32> {
    let value = object.get(key)?;
    let Some(number) = value.as_f64().map(|number| number as f32) else {
        tracing::warn!(key, %value, "a tool setting is not a number");
        return None;
    };
    let kept = number.clamp(*range.start(), *range.end());
    if kept != number {
        tracing::warn!(key, number, kept, "a tool setting was out of range");
    }
    Some(kept)
}

fn object<'a>(value: &'a Value, key: &str) -> Option<&'a Map<String, Value>> {
    let found = value.get(key)?;
    if found.as_object().is_none() {
        tracing::warn!(key, "a tool setting is not an object");
    }
    found.as_object()
}

/// `#rrggbbaa`, straight alpha.
pub(super) fn color_text(Rgba8([r, g, b, a]): Rgba8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
}

pub(super) fn parse_color(text: &str) -> Option<Rgba8> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 8 || !digits.is_ascii() {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
    Some(Rgba8([channel(0)?, channel(2)?, channel(4)?, channel(6)?]))
}

/// The preset named `id` among `presets`, a pixel one when `pixel_only`,
/// else `kept`.
fn preset(
    id: Option<&Value>,
    presets: &'static [Preset],
    pixel_only: bool,
    kept: &'static Preset,
) -> &'static Preset {
    let Some(id) = id else {
        return kept;
    };
    presets
        .iter()
        .find(|preset| id.as_str() == Some(preset.id) && (!pixel_only || is_pixel(preset)))
        .unwrap_or_else(|| {
            tracing::warn!(%id, "unknown preset in the settings");
            kept
        })
}

pub(super) fn parse(value: &Value) -> Tools {
    read(value, Tools::default())
}

/// `brushes` with what `value` holds taken over them: `brush`, `eraser` and
/// every preset's width, stabilizer and opacity in `presets`.
fn read_brushes(value: &Value, mut brushes: Brushes, pixel_only: bool) -> Brushes {
    // Every preset's, the ones in use included; one at its own is the same
    // as one not kept.
    for settings in [brushes.pen, brushes.eraser] {
        let kept = Kept::from(&settings);
        if kept != Kept::of(settings.preset) {
            brushes.remembered.insert(settings.preset.id, kept);
        }
    }
    if let Some(presets) = object(value, "presets") {
        for (id, kept) in presets {
            let Some(preset) = brush::find(id).filter(|preset| !pixel_only || is_pixel(preset))
            else {
                tracing::warn!(id, "unknown preset in the settings");
                continue;
            };
            let Some(kept) = kept.as_object() else {
                continue;
            };
            let own = Kept::of(preset);
            let kept = Kept {
                width: number(kept, "width", STROKE_WIDTH).unwrap_or(own.width),
                stabilizer: number(kept, "stabilizer", 0.0..=1.0).unwrap_or(own.stabilizer),
                // Not in settings written before the opacity setting.
                opacity: number(kept, "opacity", 0.01..=1.0).unwrap_or(own.opacity),
            };
            brushes.remembered.insert(preset.id, kept);
        }
    }
    let settings_of = |chosen: &'static Preset, base: ToolSettings| {
        let kept = brushes
            .remembered
            .get(chosen.id)
            .copied()
            .unwrap_or_else(|| Kept::of(chosen));
        ToolSettings {
            preset: chosen,
            width: kept.width,
            stabilizer: kept.stabilizer,
            opacity: kept.opacity,
            ..base
        }
    };
    let brush = object(value, "brush");
    let pen = preset(
        brush.and_then(|brush| brush.get("preset")),
        &brush::BRUSHES,
        pixel_only,
        brushes.pen.preset,
    );
    brushes.pen = settings_of(pen, brushes.pen);
    if let Some(brush) = brush {
        boolean(brush, "antialias", &mut brushes.pen.antialias);
    }
    let eraser_value = object(value, "eraser");
    let eraser = preset(
        eraser_value.and_then(|eraser| eraser.get("preset")),
        &brush::ERASERS,
        pixel_only,
        brushes.eraser.preset,
    );
    // Another eraser starts from its own antialiasing, as choosing it does.
    let antialias = if eraser.id == brushes.eraser.preset.id {
        brushes.eraser.antialias
    } else {
        eraser.brush.antialias
    };
    brushes.eraser = ToolSettings {
        antialias,
        ..settings_of(eraser, brushes.eraser)
    };
    if let Some(eraser) = eraser_value {
        boolean(eraser, "antialias", &mut brushes.eraser.antialias);
    }
    brushes.remembered.remove(pen.id);
    brushes.remembered.remove(eraser.id);
    brushes
}

/// Settings from before pixel mode had brushes of its own kept the pixel
/// brush and eraser it chose in `brush` and `eraser`, and the ones they
/// replaced in `beforePixel`: those go back out of pixel mode and the pixel
/// ones become the pixel mode's.
fn read_before_pixel(before: &Map<String, Value>, tools: &mut Tools) {
    let pen = preset(
        before.get("brush"),
        &brush::BRUSHES,
        false,
        tools.plain.pen.preset,
    );
    let eraser = preset(
        before.get("eraser"),
        &brush::ERASERS,
        false,
        tools.plain.eraser.preset,
    );
    for (tool, chosen) in [(Tool::Pen, pen), (Tool::Eraser, eraser)] {
        let replaced = *tools.plain.get(tool);
        if !is_pixel(replaced.preset) {
            continue;
        }
        *tools.pixel.get_mut(tool) = replaced;
        tools.plain.choose(tool, chosen);
        tools.plain.remembered.remove(replaced.preset.id);
    }
    boolean(before, "fillAntialias", &mut tools.fill.antialias);
}

/// `tools` with what `value` holds taken over them.
pub(super) fn read(value: &Value, mut tools: Tools) -> Tools {
    let Some(top) = value.as_object() else {
        tracing::warn!("the tool settings are not an object");
        return tools;
    };
    named(top, "tool", &TOOLS, &mut tools.tool);
    if let Some(text) = object(value, "brush").and_then(|brush| brush.get("color")) {
        match text.as_str().and_then(parse_color) {
            Some(color) => tools.color = color,
            None => tracing::warn!(%text, "the brush colour in the settings is not #rrggbbaa"),
        }
    }
    tools.plain = read_brushes(value, tools.plain, false);
    if let Some(pixel) = top.get("pixel") {
        tools.pixel = read_brushes(pixel, tools.pixel, true);
    }

    if let Some(selection) = object(value, "selection") {
        named(selection, "shape", &SHAPES, &mut tools.selection_shape);
        boolean(selection, "paint", &mut tools.lasso_paints);
        named(
            selection,
            "sampling",
            &SAMPLINGS,
            &mut tools.transform_sampling,
        );
    }
    if let Some(shape) = object(value, "shape") {
        named(shape, "shape", &FIGURES, &mut tools.shape.shape);
        let corners = ShapeSettings::CORNERS;
        let range = *corners.start() as f32..=*corners.end() as f32;
        if let Some(sides) = number(shape, "sides", range.clone()) {
            tools.shape.sides = sides.round() as u32;
        }
        if let Some(points) = number(shape, "points", range) {
            tools.shape.points = points.round() as u32;
        }
        let inner = ShapeSettings::INNER;
        if let Some(inner) = number(shape, "inner", *inner.start() as f32..=*inner.end() as f32) {
            tools.shape.inner = inner.round() as u32;
        }
        named(shape, "paint", &SHAPE_PAINTS, &mut tools.shape.paint);
    }
    if let Some(tablet) = object(value, "tablet") {
        boolean(tablet, "pressure", &mut tools.uses_pressure);
    }
    if let Some(fill) = object(value, "fill") {
        named(fill, "reads", &READS, &mut tools.fill.reads);
        boolean(fill, "byColour", &mut tools.fill.by_colour);
        if let Some(tolerance) = number(fill, "tolerance", 0.0..=255.0) {
            tools.fill.tolerance = tolerance.round() as u8;
        }
        boolean(fill, "antialias", &mut tools.fill.antialias);
    }
    if let Some(text) = object(value, "text") {
        match text.get("family") {
            None | Some(Value::Null) => {}
            Some(Value::String(family)) if !family.is_empty() => {
                tools.text.family = Some(family.clone());
            }
            Some(value) => tracing::warn!(%value, "the text family in the settings is not a name"),
        }
        if let Some(size) = number(text, "size", TextSettings::SIZE) {
            tools.text.size = size;
        }
        boolean(text, "filled", &mut tools.text.filled);
    }
    if let Some(before) = object(value, "beforePixel") {
        read_before_pixel(before, &mut tools);
    }
    tools
}

/// `brush`, `eraser` and `presets` of `brushes`.
fn brushes_to_json(brushes: &Brushes) -> Map<String, Value> {
    let mut presets = Map::new();
    let mut kept: Vec<(&str, Kept)> = brushes
        .remembered
        .iter()
        .map(|(id, kept)| (*id, *kept))
        .collect();
    for settings in [&brushes.pen, &brushes.eraser] {
        kept.retain(|(id, _)| *id != settings.preset.id);
        kept.push((settings.preset.id, Kept::from(settings)));
    }
    for (id, kept) in kept {
        presets.insert(
            id.to_owned(),
            json!({
                "width": kept.width,
                "stabilizer": kept.stabilizer,
                "opacity": kept.opacity,
            }),
        );
    }
    let mut object = Map::new();
    object.insert(
        "brush".to_owned(),
        json!({
            "preset": brushes.pen.preset.id,
            "antialias": brushes.pen.antialias,
        }),
    );
    object.insert(
        "eraser".to_owned(),
        json!({
            "preset": brushes.eraser.preset.id,
            "antialias": brushes.eraser.antialias,
        }),
    );
    object.insert("presets".to_owned(), presets.into());
    object
}

pub(super) fn to_json(tools: &Tools) -> Value {
    let mut text = Map::new();
    if let Some(family) = &tools.text.family {
        text.insert("family".to_owned(), family.clone().into());
    }
    text.insert("size".to_owned(), tools.text.size.into());
    text.insert("filled".to_owned(), tools.text.filled.into());
    let mut value = json!({
        "tool": name(&TOOLS, tools.tool),
        "pixel": brushes_to_json(&tools.pixel),
        "selection": {
            "shape": name(&SHAPES, tools.selection_shape),
            "paint": tools.lasso_paints,
            "sampling": name(&SAMPLINGS, tools.transform_sampling),
        },
        "shape": {
            "shape": name(&FIGURES, tools.shape.shape),
            "sides": tools.shape.sides,
            "points": tools.shape.points,
            "inner": tools.shape.inner,
            "paint": name(&SHAPE_PAINTS, tools.shape.paint),
        },
        "tablet": { "pressure": tools.uses_pressure },
        "fill": {
            "reads": name(&READS, tools.fill.reads),
            "byColour": tools.fill.by_colour,
            "tolerance": tools.fill.tolerance,
            "antialias": tools.fill.antialias,
        },
        "text": text,
    });
    let object = value.as_object_mut().expect("an object");
    object.extend(brushes_to_json(&tools.plain));
    if let Some(Value::Object(brush)) = object.get_mut("brush") {
        brush.insert("color".to_owned(), color_text(tools.color).into());
    }
    value
}

pub(super) fn parse_history(value: &Value) -> ColorHistory {
    let Some(names) = value.as_array() else {
        tracing::warn!("the colour history is not a list");
        return ColorHistory::default();
    };
    ColorHistory::from_colors(names.iter().filter_map(|name| {
        let color = name.as_str().and_then(parse_color);
        if color.is_none() {
            tracing::warn!(%name, "a colour in the history is not #rrggbbaa");
        }
        color
    }))
}

pub(super) fn history_to_json(history: &ColorHistory) -> Value {
    history
        .colors()
        .iter()
        .map(|color| Value::from(color_text(*color)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changed() -> Tools {
        let mut tools = Tools {
            tool: Tool::Wand,
            selection_shape: ShapeKind::Ellipse,
            lasso_paints: true,
            shape: ShapeSettings {
                shape: Shape::Moon,
                sides: 7,
                points: 9,
                inner: 30,
                paint: ShapePaint::Both,
            },
            uses_pressure: false,
            transform_sampling: Sampling::Nearest,
            ..Tools::default()
        };
        tools.color = Rgba8([10, 20, 30, 40]);
        tools.plain.pen = ToolSettings {
            preset: &brush::BRUSHES[4],
            width: 33.5,
            stabilizer: 0.4,
            opacity: 0.45,
            antialias: true,
        };
        tools.plain.eraser = ToolSettings {
            preset: &brush::ERASERS[2],
            width: 80.0,
            stabilizer: 0.0,
            opacity: brush::ERASERS[2].brush.opacity,
            // Not the preset's own.
            antialias: !brush::ERASERS[2].brush.antialias,
        };
        tools.plain.remembered.insert(
            brush::BRUSHES[0].id,
            Kept {
                width: 12.0,
                stabilizer: 0.25,
                opacity: 0.7,
            },
        );
        tools.plain.remembered.insert(
            brush::ERASERS[0].id,
            Kept {
                width: 40.0,
                stabilizer: 0.5,
                opacity: 1.0,
            },
        );
        tools.pixel.pen.width = 3.0;
        tools.pixel.eraser.stabilizer = 0.2;
        tools.fill.reads = Reads::Visible;
        tools.fill.by_colour = true;
        tools.fill.tolerance = 7;
        tools.fill.antialias = false;
        tools.text = TextSettings {
            family: Some("Malgun Gothic".to_owned()),
            size: 120.0,
            filled: true,
        };
        tools
    }

    #[test]
    fn tools_are_read_back_as_written() {
        let tools = changed();
        assert_eq!(parse(&to_json(&tools)), tools);
        assert_eq!(parse(&to_json(&Tools::default())), Tools::default());
        assert_eq!(parse(&Value::Null), Tools::default());
    }

    #[test]
    fn every_tool_is_written_and_read_back() {
        let every = [
            Tool::Pen,
            Tool::Eraser,
            Tool::Select,
            Tool::Wand,
            Tool::Fill,
            Tool::Text,
            Tool::Eyedropper,
            Tool::Shape,
            Tool::Move,
            Tool::Symmetry,
        ];
        for tool in every {
            // A new tool does not build here until it is listed above.
            match tool {
                Tool::Pen
                | Tool::Eraser
                | Tool::Select
                | Tool::Wand
                | Tool::Fill
                | Tool::Text
                | Tool::Eyedropper
                | Tool::Shape
                | Tool::Move
                | Tool::Symmetry => {}
            }
            let tools = Tools {
                tool,
                ..Tools::default()
            };
            assert_eq!(parse(&to_json(&tools)).tool, tool);
        }
    }

    #[test]
    fn out_of_range_values_are_brought_in_and_unknown_ones_left_alone() {
        let tools = parse(&json!({
            "tool": "laser",
            "brush": { "preset": "no-such-brush", "color": "red", "antialias": 1 },
            "eraser": { "preset": brush::BRUSHES[1].id },
            "presets": {
                "ink-pen": { "width": 9000, "stabilizer": -2 },
                "nothing": { "width": 5 },
            },
            "selection": { "shape": 3, "sampling": "nearest" },
            "fill": { "tolerance": 400, "reads": "marked" },
            "tablet": { "pressure": "off" },
            "text": { "size": 2, "family": "" },
        }));
        assert_eq!(tools.tool, Tool::Pen);
        assert_eq!(tools.plain.pen.preset.id, brush::BRUSHES[0].id);
        assert_eq!(tools.plain.pen.width, *STROKE_WIDTH.end());
        assert_eq!(tools.plain.pen.stabilizer, 0.0);
        assert_eq!(tools.color, Tools::default().color);
        assert!(!tools.plain.pen.antialias);
        // A brush is no eraser.
        assert_eq!(tools.plain.eraser.preset.id, brush::ERASERS[0].id);
        // "ink-pen" is in use, "nothing" no preset.
        assert!(tools.plain.remembered.is_empty());
        assert_eq!(tools.transform_sampling, Sampling::Nearest);
        assert!(tools.uses_pressure);
        assert_eq!(tools.fill.tolerance, 255);
        assert_eq!(tools.fill.reads, Reads::Marked);
        assert_eq!(tools.text.size, *TextSettings::SIZE.start());
        assert_eq!(tools.text.family, None);
    }

    #[test]
    fn what_is_missing_stays_as_it_was() {
        let kept = changed();
        assert_eq!(read(&json!({}), kept.clone()), kept);
        let tools = read(
            &json!({
                "brush": { "preset": brush::BRUSHES[1].id },
                "fill": { "tolerance": 9 },
            }),
            kept.clone(),
        );
        assert_eq!(tools.plain.pen.preset.id, brush::BRUSHES[1].id);
        assert_eq!(tools.color, kept.color);
        assert_eq!(tools.fill.tolerance, 9);
        assert_eq!(tools.fill.reads, kept.fill.reads);
        assert_eq!(tools.text, kept.text);
        // The brush left keeps its width for when it is chosen again.
        assert_eq!(
            tools.plain.remembered.get(kept.plain.pen.preset.id),
            Some(&Kept::from(&kept.plain.pen))
        );
        assert!(!tools.plain.remembered.contains_key(brush::BRUSHES[1].id));
        assert_eq!(tools.pixel, kept.pixel);
    }

    #[test]
    fn pixel_mode_keeps_pixel_presets_alone() {
        let tools = parse(&json!({
            "pixel": {
                "brush": { "preset": brush::BRUSHES[1].id },
                "presets": { (brush::BRUSHES[1].id): { "width": 30 } },
            },
        }));
        assert_eq!(tools.pixel, Brushes::pixel());
    }

    #[test]
    fn settings_from_before_the_pixel_brushes_put_the_replaced_ones_back() {
        let pencil = brush::find("pixel-pencil").unwrap();
        let eraser = brush::find("pixel-eraser").unwrap();
        let tools = parse(&json!({
            "brush": { "preset": pencil.id, "color": "#102030ff" },
            "eraser": { "preset": eraser.id },
            "presets": {
                (pencil.id): { "width": 2, "stabilizer": 0 },
                (brush::BRUSHES[3].id): { "width": 17, "stabilizer": 0.5 },
            },
            "fill": { "antialias": false },
            "beforePixel": {
                "brush": brush::BRUSHES[3].id,
                "eraser": brush::ERASERS[1].id,
                "fillAntialias": true,
            },
        }));
        assert_eq!(tools.plain.pen.preset.id, brush::BRUSHES[3].id);
        assert_eq!(
            (tools.plain.pen.width, tools.plain.pen.stabilizer),
            (17.0, 0.5)
        );
        assert_eq!(tools.plain.eraser.preset.id, brush::ERASERS[1].id);
        assert_eq!(tools.pixel.pen.preset.id, pencil.id);
        assert_eq!(tools.pixel.pen.width, 2.0);
        assert_eq!(tools.pixel.eraser.preset.id, eraser.id);
        assert!(tools.fill.antialias);
        assert_eq!(tools.color, Rgba8([0x10, 0x20, 0x30, 0xff]));
        assert!(!tools.plain.remembered.contains_key(pencil.id));
        // Written again, the old key is gone and nothing changes.
        let written = to_json(&tools);
        assert_eq!(written.get("beforePixel"), None);
        assert_eq!(parse(&written), tools);
    }

    #[test]
    fn the_history_keeps_order_drops_repeats_and_bad_names() {
        let history = parse_history(&json!(["#ff000080", "#00ff00ff", "#ff000080", "blue", 3]));
        assert_eq!(
            history.colors(),
            [Rgba8([255, 0, 0, 128]), Rgba8([0, 255, 0, 255])]
        );
        assert_eq!(parse_history(&history_to_json(&history)), history);
        assert_eq!(parse_history(&json!({})), ColorHistory::default());
    }
}
