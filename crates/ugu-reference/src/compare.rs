// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Compares two sides' exports of every scene in the matrix and summarises
//! each metric per scene group, the distribution a tolerance is read from.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::Error;
use crate::image::Rgba;
use crate::metrics::{self, Coverage, PixelDifference, Point};
use crate::reference::{self, GeometryDifference, Matrix, SceneEntry, SceneFiles, StrokeKey};

/// The ink tools/ReferenceScenes.cpp draws airbrush accumulation with.
const ACCUMULATION_INK: [u8; 3] = [29, 33, 41];

#[derive(Clone, Debug, Serialize)]
pub struct FrameReport {
    pub frame: usize,
    pub difference: PixelDifference,
}

#[derive(Clone, Debug, Serialize)]
pub struct StrokeFrameReport {
    pub frame: usize,
    pub coverage: Coverage,
}

/// How the stroke's covered area moves from one exported frame to the next.
#[derive(Clone, Debug, Serialize)]
pub struct Movement {
    /// Largest distance between the two sides' centroids on one frame.
    pub max_centroid_offset: f64,
    /// Centroid travel between consecutive exported frames, per side.
    pub travel_a: Vec<f64>,
    pub travel_b: Vec<f64>,
    pub max_travel_difference: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct StrokeReport {
    pub stroke: StrokeKey,
    pub frames: Vec<StrokeFrameReport>,
    pub movement: Option<Movement>,
}

/// Ink opacity at the canvas centre of frame 0, where the dabs are stacked.
#[derive(Clone, Debug, Serialize)]
pub struct Accumulation {
    pub opacity_a: f64,
    pub opacity_b: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct RadialProfile {
    pub a: Vec<f64>,
    pub b: Vec<f64>,
    pub max_difference: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SceneReport {
    pub name: String,
    pub group: String,
    pub parameters: serde_json::Map<String, serde_json::Value>,
    pub frames: Vec<FrameReport>,
    pub strokes: Vec<StrokeReport>,
    pub geometry: Option<GeometryDifference>,
    pub accumulation: Option<Accumulation>,
    pub radial_profile: Option<RadialProfile>,
    /// Files only one side exported.
    pub unmatched_files: usize,
    /// The worst value of each metric in this scene, as a distance: 0 means
    /// the sides agree.
    pub worst: BTreeMap<&'static str, f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Statistics {
    pub scenes: usize,
    pub median: f64,
    pub p95: f64,
    pub max: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub a: String,
    pub b: String,
    pub scenes: Vec<SceneReport>,
    /// Per group, per metric.
    pub summary: BTreeMap<String, BTreeMap<&'static str, Statistics>>,
}

pub fn compare(matrix: &Matrix, a: &Path, b: &Path) -> Result<Report, Error> {
    let scenes = matrix
        .scenes
        .iter()
        .map(|entry| compare_scene(matrix, entry, &a.join(&entry.name), &b.join(&entry.name)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Report {
        a: a.display().to_string(),
        b: b.display().to_string(),
        summary: summarise(&scenes),
        scenes,
    })
}

fn compare_scene(
    matrix: &Matrix,
    entry: &SceneEntry,
    a: &Path,
    b: &Path,
) -> Result<SceneReport, Error> {
    let (files_a, files_b) = (reference::scene_files(a)?, reference::scene_files(b)?);
    let mut unmatched_files = unmatched(&files_a.frames, &files_b.frames);

    let mut frames = Vec::new();
    for (&frame, path_a) in &files_a.frames {
        if let Some(path_b) = files_b.frames.get(&frame) {
            let (image_a, image_b) = read_pair(path_a, path_b)?;
            frames.push(FrameReport {
                frame,
                difference: metrics::pixel_difference(&image_a, &image_b),
            });
        }
    }

    let mut strokes = Vec::new();
    for (&key, frames_a) in &files_a.strokes {
        let Some(frames_b) = files_b.strokes.get(&key) else {
            unmatched_files += frames_a.len();
            continue;
        };
        unmatched_files += unmatched(frames_a, frames_b);
        strokes.push(compare_stroke(key, frames_a, frames_b)?);
    }
    unmatched_files += files_b
        .strokes
        .keys()
        .filter(|key| !files_a.strokes.contains_key(key))
        .count();

    let geometry = match (&files_a.geometry, &files_b.geometry) {
        (Some(path_a), Some(path_b)) => Some(reference::compare_geometry(
            &reference::read_geometry(path_a)?,
            &reference::read_geometry(path_b)?,
        )),
        _ => None,
    };

    let center = (
        matrix.canvas_size[0] as f64 / 2.0,
        matrix.canvas_size[1] as f64 / 2.0,
    );
    let (accumulation, radial_profile) = if entry.group == "accumulation" {
        (
            accumulation(&files_a, &files_b, center)?,
            radial(entry, &files_a, &files_b, center, matrix)?,
        )
    } else {
        (None, None)
    };

    let mut report = SceneReport {
        name: entry.name.clone(),
        group: entry.group.clone(),
        parameters: entry.parameters.clone(),
        frames,
        strokes,
        geometry,
        accumulation,
        radial_profile,
        unmatched_files,
        worst: BTreeMap::new(),
    };
    report.worst = worst(&report);
    Ok(report)
}

fn compare_stroke(
    key: StrokeKey,
    frames_a: &BTreeMap<usize, std::path::PathBuf>,
    frames_b: &BTreeMap<usize, std::path::PathBuf>,
) -> Result<StrokeReport, Error> {
    let mut frames = Vec::new();
    let mut centroids = Vec::new();
    for (&frame, path_a) in frames_a {
        let Some(path_b) = frames_b.get(&frame) else {
            continue;
        };
        let (image_a, image_b) = read_pair(path_a, path_b)?;
        frames.push(StrokeFrameReport {
            frame,
            coverage: metrics::coverage(&image_a, &image_b),
        });
        centroids.push((metrics::centroid(&image_a), metrics::centroid(&image_b)));
    }
    Ok(StrokeReport {
        stroke: key,
        frames,
        movement: movement(&centroids),
    })
}

fn movement(centroids: &[(Option<Point>, Option<Point>)]) -> Option<Movement> {
    let pairs: Vec<(Point, Point)> = centroids
        .iter()
        .map(|&(a, b)| Some((a?, b?)))
        .collect::<Option<_>>()?;
    let distance = |p: Point, q: Point| (p.0 - q.0).hypot(p.1 - q.1);
    let travel = |side: fn(&(Point, Point)) -> Point| -> Vec<f64> {
        pairs
            .windows(2)
            .map(|window| distance(side(&window[0]), side(&window[1])))
            .collect()
    };
    let (travel_a, travel_b) = (travel(|pair| pair.0), travel(|pair| pair.1));
    Some(Movement {
        max_centroid_offset: pairs
            .iter()
            .map(|&(a, b)| distance(a, b))
            .fold(0.0, f64::max),
        max_travel_difference: travel_a
            .iter()
            .zip(&travel_b)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max),
        travel_a,
        travel_b,
    })
}

fn accumulation(
    a: &SceneFiles,
    b: &SceneFiles,
    center: Point,
) -> Result<Option<Accumulation>, Error> {
    let (Some(path_a), Some(path_b)) = (a.frames.get(&0), b.frames.get(&0)) else {
        return Ok(None);
    };
    let (image_a, image_b) = read_pair(path_a, path_b)?;
    let (x, y) = (center.0 as usize, center.1 as usize);
    Ok(Some(Accumulation {
        opacity_a: metrics::ink_opacity_over_white(image_a.pixel(x, y), ACCUMULATION_INK),
        opacity_b: metrics::ink_opacity_over_white(image_b.pixel(x, y), ACCUMULATION_INK),
    }))
}

/// The falloff of a single dab, from the stroke rendered alone.
fn radial(
    entry: &SceneEntry,
    a: &SceneFiles,
    b: &SceneFiles,
    center: Point,
    matrix: &Matrix,
) -> Result<Option<RadialProfile>, Error> {
    if entry.parameter("dabs").and_then(serde_json::Value::as_u64) != Some(1) {
        return Ok(None);
    }
    let first = StrokeKey {
        layer: 0,
        operation: 0,
    };
    let path = |files: &SceneFiles| {
        files
            .strokes
            .get(&first)
            .and_then(|frames| frames.get(&0))
            .cloned()
    };
    let (Some(path_a), Some(path_b)) = (path(a), path(b)) else {
        return Ok(None);
    };
    let (image_a, image_b) = read_pair(&path_a, &path_b)?;
    let radius = matrix.canvas_size[0].min(matrix.canvas_size[1]) / 2;
    let profile_a = metrics::radial_profile(&image_a, center, radius);
    let profile_b = metrics::radial_profile(&image_b, center, radius);
    let max_difference = profile_a
        .iter()
        .zip(&profile_b)
        .map(|(p, q)| (p - q).abs())
        .fold(0.0, f64::max);
    Ok(Some(RadialProfile {
        a: profile_a,
        b: profile_b,
        max_difference,
    }))
}

fn worst(report: &SceneReport) -> BTreeMap<&'static str, f64> {
    let mut worst = BTreeMap::new();
    let mut note = |name: &'static str, value: f64| {
        let entry = worst.entry(name).or_insert(0.0_f64);
        *entry = entry.max(value);
    };
    for frame in &report.frames {
        note("frame_visible_fraction", frame.difference.visible_fraction);
        note("frame_max_channel", f64::from(frame.difference.max_channel));
    }
    for stroke in &report.strokes {
        for frame in &stroke.frames {
            let coverage = &frame.coverage;
            note("stroke_binary_iou_loss", 1.0 - coverage.binary_iou);
            note("stroke_weighted_iou_loss", 1.0 - coverage.weighted_iou);
            note(
                "stroke_mean_alpha_difference",
                coverage.mean_alpha_difference,
            );
            // One side with no edge at all is as far apart as an edge can be.
            note(
                "stroke_max_edge_distance",
                coverage.max_edge_distance.unwrap_or(f64::INFINITY),
            );
        }
        if let Some(movement) = &stroke.movement {
            note("stroke_centroid_offset", movement.max_centroid_offset);
            note("stroke_travel_difference", movement.max_travel_difference);
        }
    }
    if let Some(geometry) = &report.geometry {
        note("geometry_max_position", geometry.max_position);
        note("geometry_max_pressure", geometry.max_pressure);
        let mismatches = geometry.unmatched
            + geometry.validity_mismatches
            + geometry.point_count_mismatches
            + geometry.segment_mismatches;
        note("geometry_mismatches", mismatches as f64);
    }
    if let Some(accumulation) = &report.accumulation {
        note(
            "accumulation_opacity_difference",
            (accumulation.opacity_a - accumulation.opacity_b).abs(),
        );
    }
    if let Some(radial) = &report.radial_profile {
        note("radial_profile_difference", radial.max_difference);
    }
    note("unmatched_files", report.unmatched_files as f64);
    worst
}

fn summarise(scenes: &[SceneReport]) -> BTreeMap<String, BTreeMap<&'static str, Statistics>> {
    let mut values: BTreeMap<String, BTreeMap<&'static str, Vec<f64>>> = BTreeMap::new();
    for scene in scenes {
        for (&metric, &value) in &scene.worst {
            for group in [scene.group.clone(), "all".to_owned()] {
                values
                    .entry(group)
                    .or_default()
                    .entry(metric)
                    .or_default()
                    .push(value);
            }
        }
    }
    values
        .into_iter()
        .map(|(group, metrics)| {
            let statistics = metrics
                .into_iter()
                .map(|(metric, mut values)| {
                    values.sort_by(f64::total_cmp);
                    let at = |fraction: f64| {
                        values[((values.len() - 1) as f64 * fraction).round() as usize]
                    };
                    (
                        metric,
                        Statistics {
                            scenes: values.len(),
                            median: at(0.5),
                            p95: at(0.95),
                            max: at(1.0),
                        },
                    )
                })
                .collect();
            (group, statistics)
        })
        .collect()
}

fn unmatched<T>(a: &BTreeMap<usize, T>, b: &BTreeMap<usize, T>) -> usize {
    a.keys().filter(|key| !b.contains_key(key)).count()
        + b.keys().filter(|key| !a.contains_key(key)).count()
}

fn read_pair(a: &Path, b: &Path) -> Result<(Rgba, Rgba), Error> {
    let (image_a, image_b) = (Rgba::read_png(a)?, Rgba::read_png(b)?);
    if !image_a.same_size(&image_b) {
        return Err(Error::new(format!(
            "{} is {}x{} but {} is {}x{}",
            a.display(),
            image_a.width,
            image_a.height,
            b.display(),
            image_b.width,
            image_b.height
        )));
    }
    Ok((image_a, image_b))
}
