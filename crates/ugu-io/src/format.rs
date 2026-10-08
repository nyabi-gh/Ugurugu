// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The JSON parts of a `.ugurugu` file (ADR section 4) and the binary entry
//! layouts. These types mirror the file, not the document model: the
//! document's invariants are checked by `ugu_core` after reading.

use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "ugurugu-document";
pub const SCHEMA: u32 = 1;
pub const RENDER_REVISION: u32 = 1;

pub const MANIFEST: &str = "manifest.json";
pub const DOCUMENT: &str = "document.json";
pub const STROKE_MAGIC: [u8; 4] = *b"UGS\0";
pub const MASK_MAGIC: [u8; 4] = *b"UGM\0";
pub const BINARY_VERSION: u16 = 1;

/// The points of every stroke, one entry: per-entry overhead dominated
/// saving 20,000 strokes when each had its own.
pub const STROKES: &str = "strokes.bin";
/// The bits of every mask.
pub const MASKS: &str = "masks.bin";

pub fn image_entry(id: &str) -> String {
    format!("images/{id}.png")
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: String,
    pub schema: u32,
    pub render_revision: u32,
    /// 32 hex digits.
    pub document_id: String,
    /// Features a reader must support to open the file.
    pub required: Vec<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentDto {
    pub canvas: [u32; 2],
    /// `#rrggbbaa`.
    pub background: String,
    pub frames: u32,
    pub frames_per_second: f32,
    pub wobble: WobbleDto,
    /// Bottom first.
    pub layers: Vec<LayerDto>,
    pub strokes: Vec<StrokeDto>,
    pub masks: Vec<MaskDto>,
    pub images: Vec<ImageDto>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WobbleDto {
    pub amount: f32,
    pub style: MotionStyleDto,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionStyleDto {
    Classic,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerDto {
    pub id: u32,
    pub name: String,
    pub visible: bool,
    pub reference: bool,
    /// Exactly one of `paint` and `group` is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paint: Option<PaintDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<GroupDto>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaintDto {
    pub opacity: f32,
    pub blend: BlendDto,
    pub clip_to_below: bool,
    pub wobble: Option<WobbleDto>,
    pub initial_size: [u32; 2],
    pub ops: Vec<OpDto>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupDto {
    pub opacity: f32,
    pub blend: BlendDto,
    pub clip_to_below: bool,
    pub children: Vec<LayerDto>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendDto {
    Normal,
    Multiply,
    Screen,
    Overlay,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SamplingDto {
    Nearest,
    Smooth,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum OpDto {
    Paint {
        stroke: u32,
        clip: Option<u32>,
    },
    Erase {
        stroke: u32,
        clip: Option<u32>,
    },
    Fill {
        coverage: u32,
        color: String,
        antialias: bool,
        clip: Option<u32>,
    },
    Image {
        image: String,
        transform: [f64; 6],
        sampling: SamplingDto,
    },
    TransformSelection {
        mask: u32,
        transform: [f64; 6],
        sampling: SamplingDto,
        keep_source: bool,
    },
    ClearSelection {
        mask: u32,
    },
    Crop {
        offset: [i32; 2],
        size: [u32; 2],
    },
    Resample {
        size: [u32; 2],
        sampling: SamplingDto,
    },
    Isolated {
        opacity: f32,
        wobble: Option<WobbleDto>,
        ops: Vec<OpDto>,
    },
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrokeDto {
    pub id: u32,
    pub color: String,
    pub width: f32,
    pub brush: BrushDto,
    /// 16 hex digits: a u64 does not fit a JSON number exactly.
    pub seed: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrushDto {
    pub engine: EngineDto,
    pub tip: TipDto,
    pub opacity: f32,
    pub flow: f32,
    pub hardness: f32,
    pub spacing: f32,
    pub scatter: f32,
    pub particle_size: f32,
    pub density: f32,
    pub size_dynamics: f32,
    pub opacity_dynamics: f32,
    pub size_jitter: f32,
    pub animated_jitter: bool,
    pub wobble_scale: f32,
    pub antialias: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TipDto {
    Round,
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineDto {
    Line,
    Airbrush,
    Spray,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaskDto {
    pub id: u32,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageDto {
    /// SHA-256 of the PNG, 64 hex digits; also the entry name.
    pub id: String,
    pub size: [u32; 2],
}

pub fn color_hex(color: [u8; 4]) -> String {
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        color[0], color[1], color[2], color[3]
    )
}

pub fn parse_color(text: &str) -> Option<[u8; 4]> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 8 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut color = [0; 4];
    for (index, channel) in color.iter_mut().enumerate() {
        *channel = u8::from_str_radix(&digits[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(color)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn parse_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != N * 2
        || !text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return None;
    }
    let mut bytes = [0; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(bytes)
}
