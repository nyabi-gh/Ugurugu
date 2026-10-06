// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Feel metrics between two renders of the same scene (docs/RUST_PORT_PLAN.md
//! §4.1, layer F). None of them asks for equal pixels: they measure how far
//! apart the covered area, its edge, its opacity and its motion are.

use serde::Serialize;

use crate::image::Rgba;

/// A position in pixels, x then y.
pub type Point = (f64, f64);

/// A channel difference above this counts as visible, the threshold the C++
/// GPU-vs-CPU display comparison already uses.
pub const VISIBLE_CHANNEL_DIFFERENCE: u8 = 8;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Coverage {
    /// Pixels with any alpha, intersection over union.
    pub binary_iou: f64,
    /// Σ min(αa, αb) / Σ max(αa, αb).
    pub weighted_iou: f64,
    /// Mean |αa − αb| / 255 over pixels either side covers.
    pub mean_alpha_difference: f64,
    /// Distance from each edge pixel to the nearest edge pixel of the other
    /// side, both ways, in pixels. None when only one side has an edge.
    pub mean_edge_distance: Option<f64>,
    pub max_edge_distance: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PixelDifference {
    pub max_channel: u8,
    /// Mean absolute channel difference over all channels, 0–255.
    pub mean_absolute: f64,
    /// Share of pixels with a channel apart by more than
    /// [`VISIBLE_CHANNEL_DIFFERENCE`].
    pub visible_fraction: f64,
}

pub fn coverage(a: &Rgba, b: &Rgba) -> Coverage {
    assert!(a.same_size(b), "coverage needs images of one size");
    let mut intersection = 0_u64;
    let mut union = 0_u64;
    let mut minimum = 0_u64;
    let mut maximum = 0_u64;
    let mut alpha_difference = 0_u64;
    for y in 0..a.height {
        for x in 0..a.width {
            let (alpha_a, alpha_b) = (a.alpha(x, y), b.alpha(x, y));
            if alpha_a > 0 && alpha_b > 0 {
                intersection += 1;
            }
            if alpha_a > 0 || alpha_b > 0 {
                union += 1;
                alpha_difference += u64::from(alpha_a.abs_diff(alpha_b));
            }
            minimum += u64::from(alpha_a.min(alpha_b));
            maximum += u64::from(alpha_a.max(alpha_b));
        }
    }
    let (mean_edge_distance, max_edge_distance) = edge_distance(&edge_pixels(a), &edge_pixels(b));
    Coverage {
        binary_iou: ratio(intersection, union),
        weighted_iou: ratio(minimum, maximum),
        mean_alpha_difference: if union == 0 {
            0.0
        } else {
            alpha_difference as f64 / union as f64 / 255.0
        },
        mean_edge_distance,
        max_edge_distance,
    }
}

pub fn pixel_difference(a: &Rgba, b: &Rgba) -> PixelDifference {
    assert!(a.same_size(b), "pixel difference needs images of one size");
    let mut max_channel = 0_u8;
    let mut total = 0_u64;
    let mut visible = 0_u64;
    for (pixel_a, pixel_b) in a
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.data.as_chunks::<4>().0)
    {
        let largest = pixel_a
            .iter()
            .zip(pixel_b)
            .map(|(&p, &q)| p.abs_diff(q))
            .max()
            .unwrap_or(0);
        total += pixel_a
            .iter()
            .zip(pixel_b)
            .map(|(&p, &q)| u64::from(p.abs_diff(q)))
            .sum::<u64>();
        max_channel = max_channel.max(largest);
        if largest > VISIBLE_CHANNEL_DIFFERENCE {
            visible += 1;
        }
    }
    let pixels = (a.width * a.height) as u64;
    PixelDifference {
        max_channel,
        mean_absolute: ratio(total, pixels * 4),
        visible_fraction: ratio(visible, pixels),
    }
}

/// The per-pixel difference, every channel scaled by four so small
/// differences show, on opaque black.
pub fn difference_image(a: &Rgba, b: &Rgba) -> Rgba {
    assert!(a.same_size(b), "difference image needs images of one size");
    let mut image = Rgba::new(a.width, a.height);
    for y in 0..a.height {
        for x in 0..a.width {
            let (p, q) = (a.pixel(x, y), b.pixel(x, y));
            let channel = |index: usize| p[index].abs_diff(q[index]).saturating_mul(4);
            let alpha = channel(3);
            image.set_pixel(
                x,
                y,
                [
                    channel(0).max(alpha),
                    channel(1).max(alpha),
                    channel(2).max(alpha),
                    255,
                ],
            );
        }
    }
    image
}

/// The alpha-weighted centre of the covered area, if anything is covered.
pub fn centroid(image: &Rgba) -> Option<Point> {
    let (mut sum, mut sum_x, mut sum_y) = (0.0, 0.0, 0.0);
    for y in 0..image.height {
        for x in 0..image.width {
            let alpha = f64::from(image.alpha(x, y));
            sum += alpha;
            sum_x += alpha * (x as f64 + 0.5);
            sum_y += alpha * (y as f64 + 0.5);
        }
    }
    (sum > 0.0).then(|| (sum_x / sum, sum_y / sum))
}

/// Mean alpha / 255 of the pixels whose centres fall in each one-pixel ring
/// around `center`, out to `radius`.
pub fn radial_profile(image: &Rgba, center: Point, radius: usize) -> Vec<f64> {
    let mut sums = vec![0.0; radius];
    let mut counts = vec![0_u32; radius];
    for y in 0..image.height {
        for x in 0..image.width {
            let distance = (x as f64 + 0.5 - center.0).hypot(y as f64 + 0.5 - center.1);
            let ring = distance as usize;
            if ring < radius {
                sums[ring] += f64::from(image.alpha(x, y)) / 255.0;
                counts[ring] += 1;
            }
        }
    }
    sums.iter()
        .zip(&counts)
        .map(|(&sum, &count)| {
            if count == 0 {
                0.0
            } else {
                sum / f64::from(count)
            }
        })
        .collect()
}

/// Opacity of ink of colour `ink` composited over white, read back from the
/// channel where ink and white are furthest apart.
pub fn ink_opacity_over_white(pixel: [u8; 4], ink: [u8; 3]) -> f64 {
    let channel = (0..3).max_by_key(|&index| 255 - ink[index]).unwrap_or(0);
    let span = f64::from(255 - ink[channel]);
    if span == 0.0 {
        return 0.0;
    }
    (f64::from(255 - pixel[channel]) / span).clamp(0.0, 1.0)
}

fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        1.0
    } else {
        numerator as f64 / denominator as f64
    }
}

/// Covered pixels with an uncovered 4-neighbour or the image border.
fn edge_pixels(image: &Rgba) -> Vec<(i64, i64)> {
    let covered = |x: i64, y: i64| {
        x >= 0
            && y >= 0
            && (x as usize) < image.width
            && (y as usize) < image.height
            && image.alpha(x as usize, y as usize) > 0
    };
    let mut edges = Vec::new();
    for y in 0..image.height as i64 {
        for x in 0..image.width as i64 {
            if covered(x, y)
                && [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .any(|(dx, dy)| !covered(x + dx, y + dy))
            {
                edges.push((x, y));
            }
        }
    }
    edges
}

fn edge_distance(a: &[(i64, i64)], b: &[(i64, i64)]) -> (Option<f64>, Option<f64>) {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return (Some(0.0), Some(0.0)),
        (true, false) | (false, true) => return (None, None),
        (false, false) => {}
    }
    let nearest = |point: &(i64, i64), others: &[(i64, i64)]| {
        others
            .iter()
            .map(|other| ((point.0 - other.0).pow(2) + (point.1 - other.1).pow(2)) as f64)
            .fold(f64::INFINITY, f64::min)
            .sqrt()
    };
    let distances: Vec<f64> = a
        .iter()
        .map(|point| nearest(point, b))
        .chain(b.iter().map(|point| nearest(point, a)))
        .collect();
    let max = distances.iter().copied().fold(0.0, f64::max);
    let mean = distances.iter().sum::<f64>() / distances.len() as f64;
    (Some(mean), Some(max))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disc(center: Point, radius: f64, alpha: u8) -> Rgba {
        let mut image = Rgba::new(40, 40);
        for y in 0..40 {
            for x in 0..40 {
                if (x as f64 + 0.5 - center.0).hypot(y as f64 + 0.5 - center.1) <= radius {
                    image.set_pixel(x, y, [0, 0, 0, alpha]);
                }
            }
        }
        image
    }

    #[test]
    fn identical_images_agree_completely() {
        let image = disc((20.0, 20.0), 8.0, 200);
        let measured = coverage(&image, &image);
        assert_eq!(measured.binary_iou, 1.0);
        assert_eq!(measured.weighted_iou, 1.0);
        assert_eq!(measured.mean_alpha_difference, 0.0);
        assert_eq!(measured.max_edge_distance, Some(0.0));
        assert_eq!(pixel_difference(&image, &image).max_channel, 0);
    }

    #[test]
    fn a_shifted_edge_measures_the_shift() {
        let measured = coverage(&disc((20.0, 20.0), 8.0, 255), &disc((22.0, 20.0), 8.0, 255));
        assert_eq!(measured.max_edge_distance, Some(2.0));
        assert!(measured.binary_iou < 1.0 && measured.binary_iou > 0.6);
    }

    #[test]
    fn opacity_differences_show_in_alpha_and_weighted_iou_only() {
        let measured = coverage(&disc((20.0, 20.0), 8.0, 255), &disc((20.0, 20.0), 8.0, 51));
        assert_eq!(measured.binary_iou, 1.0);
        assert!((measured.weighted_iou - 0.2).abs() < 1e-12);
        assert!((measured.mean_alpha_difference - 0.8).abs() < 1e-12);
    }

    #[test]
    fn one_empty_side_has_no_edge_distance() {
        let measured = coverage(&disc((20.0, 20.0), 8.0, 255), &Rgba::new(40, 40));
        assert_eq!(measured.binary_iou, 0.0);
        assert_eq!(measured.max_edge_distance, None);
    }

    #[test]
    fn centroid_and_profile_follow_the_disc() {
        let image = disc((20.0, 20.0), 6.0, 255);
        let (x, y) = centroid(&image).unwrap();
        assert!((x - 20.0).abs() < 1e-9 && (y - 20.0).abs() < 1e-9);
        let profile = radial_profile(&image, (20.0, 20.0), 10);
        assert_eq!(profile[0], 1.0);
        assert_eq!(profile[9], 0.0);
    }

    #[test]
    fn ink_opacity_reads_back_a_blend_over_white() {
        let ink = [29, 33, 41];
        assert_eq!(ink_opacity_over_white([255, 255, 255, 255], ink), 0.0);
        assert_eq!(ink_opacity_over_white([29, 33, 41, 255], ink), 1.0);
        let half = 255 - (255 - 29) / 2;
        assert!((ink_opacity_over_white([half, 0, 0, 255], ink) - 0.5).abs() < 0.01);
    }
}
