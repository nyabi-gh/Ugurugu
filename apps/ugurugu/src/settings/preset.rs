// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Tool preset files: the tools as the settings keep them and the document's
//! wobble, as 2.2.13's `.wwpreset` held its drawing tools and motion. Reading
//! one takes what it holds over the tools and wobble in use; what it does not
//! know is left alone and what is out of range is brought into it, as in the
//! settings file.

use std::io::Read;
use std::path::Path;

use serde_json::{Map, Value, json};
use ugu_core::document::limits;
use ugu_core::ops::{MotionStyle, Wobble};
use ugu_session::Tools;

use super::tools;

const FORMAT: &str = "ugurugu.tools";
const VERSION: u64 = 1;
pub const EXTENSION: &str = "ugurugu-tools";
/// As 2.2.13's.
const LARGEST: u64 = 1024 * 1024;

const STYLES: [(MotionStyle, &str); 3] = [
    (MotionStyle::Classic, "classic"),
    (MotionStyle::Smooth, "smooth"),
    (MotionStyle::Stepped, "stepped"),
];

#[derive(Debug, PartialEq, Eq)]
pub enum PresetError {
    Unreadable(String),
    TooLarge,
    /// Not JSON, or not a preset of a version this reads.
    NotPreset,
}

/// The file at `path`, if it is no larger than a preset can be.
pub fn load(path: &Path) -> Result<Vec<u8>, PresetError> {
    let unreadable = |error: std::io::Error| PresetError::Unreadable(error.to_string());
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(unreadable)?
        .take(LARGEST + 1)
        .read_to_end(&mut bytes)
        .map_err(unreadable)?;
    if bytes.len() as u64 > LARGEST {
        return Err(PresetError::TooLarge);
    }
    Ok(bytes)
}

pub fn write(tools: &Tools, wobble: Wobble) -> Vec<u8> {
    let motion = wobble.motion;
    let style = STYLES
        .iter()
        .find(|(style, _)| *style == motion.style)
        .map(|(_, name)| *name)
        .expect("every style is named");
    let preset = json!({
        "format": FORMAT,
        "version": VERSION,
        "tools": tools::to_json(tools),
        "motion": {
            "amount": wobble.amount,
            "style": style,
            "poses": motion.poses,
            "detail": motion.detail,
            "linked": motion.linked,
            "randomness": motion.randomness,
            "broken": motion.broken,
            "breakAmount": motion.break_amount,
            "breakRange": motion.break_range,
        },
    });
    serde_json::to_vec_pretty(&preset).expect("JSON values are written")
}

/// `tools` and `wobble` with what the preset in `bytes` holds taken over
/// them.
pub fn read(bytes: &[u8], tools: Tools, wobble: Wobble) -> Result<(Tools, Wobble), PresetError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| PresetError::NotPreset)?;
    if value.get("format").and_then(Value::as_str) != Some(FORMAT)
        || value.get("version").and_then(Value::as_u64) != Some(VERSION)
    {
        return Err(PresetError::NotPreset);
    }
    let tools = match value.get("tools") {
        Some(found) => tools::read(found, tools),
        None => tools,
    };
    let wobble = match value.get("motion") {
        Some(Value::Object(motion)) => read_wobble(motion, wobble),
        Some(_) => {
            tracing::warn!("the preset motion is not an object");
            wobble
        }
        None => wobble,
    };
    Ok((tools, wobble))
}

fn read_wobble(object: &Map<String, Value>, mut wobble: Wobble) -> Wobble {
    let share = 0.0..=1.0;
    let number = |key: &str, range: std::ops::RangeInclusive<f32>| -> Option<f32> {
        let value = object.get(key)?;
        let Some(number) = value.as_f64().map(|number| number as f32) else {
            tracing::warn!(key, %value, "a preset motion value is not a number");
            return None;
        };
        let kept = number.clamp(*range.start(), *range.end());
        if kept != number {
            tracing::warn!(key, number, kept, "a preset motion value was out of range");
        }
        Some(kept)
    };
    let whole = |key: &str, range: std::ops::RangeInclusive<u32>| {
        number(key, *range.start() as f32..=*range.end() as f32).map(|number| number.round() as u32)
    };
    let motion = &mut wobble.motion;
    if let Some(amount) = number("amount", limits::WOBBLE_AMOUNT) {
        wobble.amount = amount;
    }
    if let Some(style) = object.get("style") {
        match STYLES
            .iter()
            .find(|(_, name)| style.as_str() == Some(*name))
        {
            Some((found, _)) => motion.style = *found,
            None => tracing::warn!(%style, "unknown motion style in the preset"),
        }
    }
    if let Some(poses) = whole("poses", limits::MOTION_POSES) {
        motion.poses = poses;
    }
    if let Some(detail) = whole("detail", limits::MOTION_DETAIL) {
        motion.detail = detail;
    }
    if let Some(linked) = number("linked", share.clone()) {
        motion.linked = linked;
    }
    if let Some(randomness) = number("randomness", share.clone()) {
        motion.randomness = randomness;
    }
    match object.get("broken") {
        None => {}
        Some(Value::Bool(broken)) => motion.broken = *broken,
        Some(value) => tracing::warn!(%value, "the preset's broken line is not true or false"),
    }
    if let Some(amount) = number("breakAmount", share) {
        motion.break_amount = amount;
    }
    if let Some(range) = number("breakRange", limits::BREAK_RANGE) {
        motion.break_range = range;
    }
    wobble
}

#[cfg(test)]
mod tests {
    use super::*;
    use ugu_core::brush;
    use ugu_core::ops::{Motion, Rgba8};
    use ugu_session::{Tool, ToolSettings};

    fn tools() -> Tools {
        let mut tools = Tools {
            tool: Tool::Fill,
            ..Tools::default()
        };
        tools.pen = ToolSettings {
            preset: &brush::BRUSHES[3],
            width: 21.0,
            stabilizer: 0.3,
            antialias: true,
            color: Rgba8([0, 0, 255, 255]),
        };
        tools.fill.tolerance = 12;
        tools
    }

    fn wobble() -> Wobble {
        Wobble {
            amount: 4.5,
            motion: Motion {
                style: MotionStyle::Smooth,
                poses: 6,
                detail: 18,
                linked: 0.25,
                randomness: 0.5,
                broken: true,
                break_amount: 0.6,
                break_range: 40.0,
            },
        }
    }

    #[test]
    fn a_preset_is_read_back_as_written() {
        let (tools, wobble) = (tools(), wobble());
        let bytes = write(&tools, wobble);
        assert_eq!(
            read(&bytes, Tools::default(), Wobble::classic(1.0)),
            Ok((tools, wobble))
        );
    }

    #[test]
    fn the_colour_history_is_not_in_a_preset() {
        let mut tools = tools();
        tools.colors.record(Rgba8([1, 2, 3, 255]));
        let mine = Tools::default();
        let (read, _) = read(&write(&tools, wobble()), mine.clone(), wobble()).unwrap();
        assert_eq!(read.colors, mine.colors);
    }

    #[test]
    fn what_is_unknown_is_ignored_and_out_of_range_brought_in() {
        let preset = json!({
            "format": FORMAT,
            "version": 1,
            "maker": "someone",
            "tools": { "tool": "laser", "fill": { "tolerance": 900 }, "future": 1 },
            "motion": {
                "amount": 99,
                "style": "bouncy",
                "poses": 0,
                "detail": "many",
                "linked": -1,
                "breakRange": 1000,
                "wiggle": true,
            },
        });
        let (tools, wobble) = read(preset.to_string().as_bytes(), tools(), self::wobble()).unwrap();
        assert_eq!(tools.tool, Tool::Fill);
        assert_eq!(tools.fill.tolerance, 255);
        assert_eq!(tools.pen, self::tools().pen);
        assert_eq!(wobble.amount, *limits::WOBBLE_AMOUNT.end());
        assert_eq!(wobble.motion.style, MotionStyle::Smooth);
        assert_eq!(wobble.motion.poses, 1);
        assert_eq!(wobble.motion.detail, 18);
        assert_eq!(wobble.motion.linked, 0.0);
        assert_eq!(wobble.motion.break_range, *limits::BREAK_RANGE.end());
        assert!(wobble.motion.broken);
    }

    #[test]
    fn other_files_are_not_presets() {
        let read = |bytes: &[u8]| read(bytes, Tools::default(), Wobble::classic(1.0));
        assert_eq!(read(b"not json"), Err(PresetError::NotPreset));
        assert_eq!(read(b"[1, 2]"), Err(PresetError::NotPreset));
        for preset in [
            json!({ "format": "UGURUGU_PRESET", "version": 1 }),
            json!({ "format": FORMAT, "version": 2 }),
            json!({ "format": FORMAT }),
        ] {
            assert_eq!(
                read(preset.to_string().as_bytes()),
                Err(PresetError::NotPreset)
            );
        }
        // Nothing but the header changes nothing.
        let header = json!({ "format": FORMAT, "version": 1 }).to_string();
        assert_eq!(
            read(header.as_bytes()),
            Ok((Tools::default(), Wobble::classic(1.0)))
        );
    }

    #[test]
    fn a_file_larger_than_a_preset_is_not_read() {
        let folder = std::env::temp_dir().join(format!("ugurugu-preset-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(format!("large.{EXTENSION}"));
        std::fs::write(&path, vec![b' '; LARGEST as usize + 1]).unwrap();
        assert_eq!(load(&path), Err(PresetError::TooLarge));
        let bytes = write(&tools(), wobble());
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(load(&path), Ok(bytes));
        assert!(matches!(
            load(&folder.join("missing")),
            Err(PresetError::Unreadable(_))
        ));
        let _ = std::fs::remove_dir_all(&folder);
    }
}
