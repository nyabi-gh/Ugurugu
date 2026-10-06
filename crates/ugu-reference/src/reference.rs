// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Reads the scene matrix and one side's reference export of a scene, the
//! layout tools/ReferenceExport.cpp writes (see tests/reference/README.md).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Error;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Matrix {
    pub canvas_size: [usize; 2],
    pub frames: usize,
    pub scenes: Vec<SceneEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SceneEntry {
    pub name: String,
    pub group: String,
    #[serde(flatten)]
    pub parameters: serde_json::Map<String, serde_json::Value>,
}

impl SceneEntry {
    pub fn parameter(&self, key: &str) -> Option<&serde_json::Value> {
        self.parameters.get(key)
    }
}

pub fn read_matrix(path: &Path) -> Result<Matrix, Error> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

/// Identifies one stroke rendered alone: its layer and operation index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct StrokeKey {
    pub layer: usize,
    pub operation: usize,
}

/// The files one side exported for a scene.
#[derive(Clone, Debug, Default)]
pub struct SceneFiles {
    pub frames: BTreeMap<usize, PathBuf>,
    pub strokes: BTreeMap<StrokeKey, BTreeMap<usize, PathBuf>>,
    pub geometry: Option<PathBuf>,
}

pub fn scene_files(directory: &Path) -> Result<SceneFiles, Error> {
    let mut files = SceneFiles::default();
    for entry in read_directory(&directory.join("frames"))? {
        if let Some(frame) = entry
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| {
                name.strip_prefix("frame-")?
                    .strip_suffix(".png")?
                    .parse()
                    .ok()
            })
        {
            files.frames.insert(frame, entry);
        }
    }
    for entry in read_directory(&directory.join("strokes"))? {
        if let Some((key, frame)) = entry
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(parse_stroke_name)
        {
            files.strokes.entry(key).or_default().insert(frame, entry);
        }
    }
    let geometry = directory.join("geometry.jsonl");
    files.geometry = geometry.is_file().then_some(geometry);
    Ok(files)
}

fn read_directory(directory: &Path) -> Result<Vec<PathBuf>, Error> {
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    Ok(paths)
}

/// `LNN-SNNNN-fNNNN.png`
fn parse_stroke_name(name: &str) -> Option<(StrokeKey, usize)> {
    let stem = name.strip_suffix(".png")?;
    let mut parts = stem.split('-');
    let layer = parts.next()?.strip_prefix('L')?.parse().ok()?;
    let operation = parts.next()?.strip_prefix('S')?.parse().ok()?;
    let frame = parts.next()?.strip_prefix('f')?.parse().ok()?;
    parts
        .next()
        .is_none()
        .then_some((StrokeKey { layer, operation }, frame))
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeometryLine {
    pub layer: usize,
    pub operation: usize,
    pub frame: usize,
    pub valid: bool,
    pub width: f64,
    pub points: Vec<[f64; 3]>,
    pub visible_segments: Vec<u8>,
}

pub fn read_geometry(path: &Path) -> Result<Vec<GeometryLine>, Error> {
    fs::read_to_string(path)?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| Ok(serde_json::from_str(line)?))
        .collect()
}

/// How far apart two exports' prepared stroke geometry is (layer M).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct GeometryDifference {
    pub compared: usize,
    /// Lines one side has and the other does not.
    pub unmatched: usize,
    pub validity_mismatches: usize,
    pub point_count_mismatches: usize,
    pub segment_mismatches: usize,
    pub max_position: f64,
    pub max_pressure: f64,
    pub max_width: f64,
}

pub fn compare_geometry(a: &[GeometryLine], b: &[GeometryLine]) -> GeometryDifference {
    let key = |line: &GeometryLine| (line.layer, line.operation, line.frame);
    let index: BTreeMap<_, _> = b.iter().map(|line| (key(line), line)).collect();
    let mut difference = GeometryDifference {
        unmatched: b
            .iter()
            .filter(|line| !a.iter().any(|other| key(other) == key(line)))
            .count(),
        ..GeometryDifference::default()
    };
    for line in a {
        let Some(other) = index.get(&key(line)) else {
            difference.unmatched += 1;
            continue;
        };
        difference.compared += 1;
        if line.valid != other.valid {
            difference.validity_mismatches += 1;
        }
        if line.visible_segments != other.visible_segments {
            difference.segment_mismatches += 1;
        }
        difference.max_width = difference.max_width.max((line.width - other.width).abs());
        if line.points.len() != other.points.len() {
            difference.point_count_mismatches += 1;
            continue;
        }
        for (p, q) in line.points.iter().zip(&other.points) {
            difference.max_position = difference
                .max_position
                .max((p[0] - q[0]).hypot(p[1] - q[1]));
            difference.max_pressure = difference.max_pressure.max((p[2] - q[2]).abs());
        }
    }
    difference
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stroke_file_names_parse() {
        assert_eq!(
            parse_stroke_name("L01-S0012-f0009.png"),
            Some((
                StrokeKey {
                    layer: 1,
                    operation: 12
                },
                9
            ))
        );
        assert_eq!(parse_stroke_name("frame-0000.png"), None);
        assert_eq!(parse_stroke_name("L01-S0012-f0009-x.png"), None);
    }

    fn line(frame: usize, x: f64) -> GeometryLine {
        GeometryLine {
            layer: 0,
            operation: 0,
            frame,
            valid: true,
            width: 6.0,
            points: vec![[x, 1.0, 0.5], [x + 1.0, 2.0, 0.5]],
            visible_segments: vec![1],
        }
    }

    #[test]
    fn geometry_differences_are_measured_per_matching_line() {
        let difference = compare_geometry(
            &[line(0, 1.0), line(3, 1.0)],
            &[line(0, 1.25), line(6, 1.0)],
        );
        assert_eq!(difference.compared, 1);
        assert_eq!(difference.unmatched, 2);
        assert!((difference.max_position - 0.25).abs() < 1e-12);
        assert_eq!(difference.point_count_mismatches, 0);
    }
}
