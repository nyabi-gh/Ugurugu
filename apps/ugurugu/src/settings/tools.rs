// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The tools and the colour history in the settings file, as 2.2.13 keeps its
//! `drawingTools` keys and colour history. A value out of range is brought
//! into it, an unknown one is left at its default, and either is logged.

use serde_json::{Map, Value, json};
use ugu_core::brush::{self, Preset};
use ugu_core::ops::{Rgba8, Sampling};
use ugu_core::store::limits::STROKE_WIDTH;
use ugu_session::{ColorHistory, Reads, ShapeKind, TextSettings, Tool, ToolSettings, Tools};

const TOOLS: [(Tool, &str); 7] = [
    (Tool::Pen, "brush"),
    (Tool::Eraser, "eraser"),
    (Tool::Select, "select"),
    (Tool::Wand, "wand"),
    (Tool::Fill, "fill"),
    (Tool::Text, "text"),
    (Tool::Eyedropper, "eyedropper"),
];
const SHAPES: [(ShapeKind, &str); 3] = [
    (ShapeKind::Freehand, "freehand"),
    (ShapeKind::Rectangle, "rectangle"),
    (ShapeKind::Ellipse, "ellipse"),
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

/// The preset named `id` among `presets`, else `kept`.
fn preset(
    id: Option<&Value>,
    presets: &'static [Preset],
    kept: &'static Preset,
) -> &'static Preset {
    let Some(id) = id else {
        return kept;
    };
    presets
        .iter()
        .find(|preset| id.as_str() == Some(preset.id))
        .unwrap_or_else(|| {
            tracing::warn!(%id, "unknown preset in the settings");
            kept
        })
}

pub(super) fn parse(value: &Value) -> Tools {
    read(value, Tools::default())
}

/// `tools` with what `value` holds taken over them.
pub(super) fn read(value: &Value, mut tools: Tools) -> Tools {
    let Some(top) = value.as_object() else {
        tracing::warn!("the tool settings are not an object");
        return tools;
    };
    named(top, "tool", &TOOLS, &mut tools.tool);

    // Every preset's width and stabilizer, the ones in use included; one
    // at its own is the same as one not kept.
    for settings in [tools.pen, tools.eraser] {
        let kept = (settings.width, settings.stabilizer);
        if kept != (settings.preset.size, 0.0) {
            tools.remembered.insert(settings.preset.id, kept);
        }
    }
    if let Some(presets) = object(value, "presets") {
        for (id, kept) in presets {
            let Some(preset) = brush::find(id) else {
                tracing::warn!(id, "unknown preset in the settings");
                continue;
            };
            let Some(kept) = kept.as_object() else {
                continue;
            };
            let width = number(kept, "width", STROKE_WIDTH).unwrap_or(preset.size);
            let stabilizer = number(kept, "stabilizer", 0.0..=1.0).unwrap_or(0.0);
            tools.remembered.insert(preset.id, (width, stabilizer));
        }
    }
    let settings_of = |chosen: &'static Preset, base: ToolSettings| {
        let (width, stabilizer) = tools
            .remembered
            .get(chosen.id)
            .copied()
            .unwrap_or((chosen.size, 0.0));
        ToolSettings {
            preset: chosen,
            width,
            stabilizer,
            ..base
        }
    };
    let brush = object(value, "brush");
    let pen = preset(
        brush.and_then(|brush| brush.get("preset")),
        &brush::BRUSHES,
        tools.pen.preset,
    );
    tools.pen = settings_of(pen, tools.pen);
    if let Some(brush) = brush {
        if let Some(text) = brush.get("color") {
            match text.as_str().and_then(parse_color) {
                Some(color) => tools.pen.color = color,
                None => tracing::warn!(%text, "the brush colour in the settings is not #rrggbbaa"),
            }
        }
        boolean(brush, "antialias", &mut tools.pen.antialias);
    }
    let eraser = preset(
        object(value, "eraser").and_then(|eraser| eraser.get("preset")),
        &brush::ERASERS,
        tools.eraser.preset,
    );
    tools.eraser = ToolSettings {
        antialias: eraser.brush.antialias,
        ..settings_of(eraser, tools.eraser)
    };
    tools.remembered.remove(pen.id);
    tools.remembered.remove(eraser.id);

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
    tools
}

pub(super) fn to_json(tools: &Tools) -> Value {
    let mut presets = Map::new();
    let mut kept: Vec<(&str, (f32, f32))> = tools
        .remembered
        .iter()
        .map(|(id, kept)| (*id, *kept))
        .collect();
    for settings in [&tools.pen, &tools.eraser] {
        kept.retain(|(id, _)| *id != settings.preset.id);
        kept.push((settings.preset.id, (settings.width, settings.stabilizer)));
    }
    for (id, (width, stabilizer)) in kept {
        presets.insert(
            id.to_owned(),
            json!({ "width": width, "stabilizer": stabilizer }),
        );
    }
    let mut text = Map::new();
    if let Some(family) = &tools.text.family {
        text.insert("family".to_owned(), family.clone().into());
    }
    text.insert("size".to_owned(), tools.text.size.into());
    text.insert("filled".to_owned(), tools.text.filled.into());
    json!({
        "tool": name(&TOOLS, tools.tool),
        "brush": {
            "preset": tools.pen.preset.id,
            "color": color_text(tools.pen.color),
            "antialias": tools.pen.antialias,
        },
        "eraser": { "preset": tools.eraser.preset.id },
        "presets": presets,
        "selection": {
            "shape": name(&SHAPES, tools.selection_shape),
            "paint": tools.lasso_paints,
            "sampling": name(&SAMPLINGS, tools.transform_sampling),
        },
        "fill": {
            "reads": name(&READS, tools.fill.reads),
            "byColour": tools.fill.by_colour,
            "tolerance": tools.fill.tolerance,
            "antialias": tools.fill.antialias,
        },
        "text": text,
    })
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
            transform_sampling: Sampling::Nearest,
            ..Tools::default()
        };
        tools.pen = ToolSettings {
            preset: &brush::BRUSHES[4],
            width: 33.5,
            stabilizer: 0.4,
            antialias: true,
            color: Rgba8([10, 20, 30, 40]),
        };
        tools.eraser = ToolSettings {
            preset: &brush::ERASERS[2],
            width: 80.0,
            stabilizer: 0.0,
            antialias: brush::ERASERS[2].brush.antialias,
            ..ToolSettings::ERASER
        };
        tools.remembered.insert(brush::BRUSHES[0].id, (12.0, 0.25));
        tools.remembered.insert(brush::ERASERS[0].id, (40.0, 0.5));
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
            "text": { "size": 2, "family": "" },
        }));
        assert_eq!(tools.tool, Tool::Pen);
        assert_eq!(tools.pen.preset.id, brush::BRUSHES[0].id);
        assert_eq!(tools.pen.width, *STROKE_WIDTH.end());
        assert_eq!(tools.pen.stabilizer, 0.0);
        assert_eq!(tools.pen.color, ToolSettings::PEN.color);
        assert!(!tools.pen.antialias);
        // A brush is no eraser.
        assert_eq!(tools.eraser.preset.id, brush::ERASERS[0].id);
        // "ink-pen" is in use, "nothing" no preset.
        assert!(tools.remembered.is_empty());
        assert_eq!(tools.transform_sampling, Sampling::Nearest);
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
        assert_eq!(tools.pen.preset.id, brush::BRUSHES[1].id);
        assert_eq!(tools.pen.color, kept.pen.color);
        assert_eq!(tools.fill.tolerance, 9);
        assert_eq!(tools.fill.reads, kept.fill.reads);
        assert_eq!(tools.text, kept.text);
        // The brush left keeps its width for when it is chosen again.
        assert_eq!(
            tools.remembered.get(kept.pen.preset.id),
            Some(&(kept.pen.width, kept.pen.stabilizer))
        );
        assert!(!tools.remembered.contains_key(brush::BRUSHES[1].id));
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
