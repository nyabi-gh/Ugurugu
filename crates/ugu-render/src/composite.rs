// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Puts layer surfaces together as a `RenderPlan` says, one row at a time,
//! in premultiplied RGBA8.
//!
//! Normal rounds as Vello's u8 pipeline does, so a layer drawn on its own
//! surface and put over the one below gives the same bytes as Vello drawing
//! both in one scene. Multiply, Screen and Overlay are the W3C separable
//! formulas on premultiplied values, rounded to the nearest level.

use ugu_core::ops::Blend;
use vello_cpu::Pixmap;

use crate::compose::src_over;
use crate::plan::{Composite, RenderPlan, Step};
use crate::raster::PixelRect;

type Pixel = [u8; 4];

/// `a·b/255` as Vello's u8 pipeline rounds it.
fn mul(a: u8, b: u8) -> u8 {
    ((u16::from(a) * u16::from(b) + 255) >> 8) as u8
}

fn div255(value: u32) -> u8 {
    ((value + 127) / 255) as u8
}

fn opacity_level(opacity: f32) -> u8 {
    (opacity.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Premultiplied `source` over `target` with `mode`.
pub fn blend(mode: Blend, target: &mut Pixel, source: Pixel) {
    if source[3] == 0 {
        return;
    }
    if mode == Blend::Normal {
        src_over(target, &source);
        return;
    }
    let (sa, ba) = (u32::from(source[3]), u32::from(target[3]));
    let alpha = div255(255 * sa + 255 * ba - sa * ba);
    for channel in 0..3 {
        let (s, b) = (u32::from(source[channel]), u32::from(target[channel]));
        // Each term stays within 255², and the sum is at most 255·alpha.
        let value = match mode {
            Blend::Normal => unreachable!(),
            Blend::Multiply => s * (255 - ba) + b * (255 - sa) + s * b,
            Blend::Screen => 255 * s + 255 * b - s * b,
            Blend::Overlay => {
                let rest = s * (255 - ba) + b * (255 - sa);
                if 2 * b <= ba {
                    rest + 2 * s * b
                } else {
                    rest + sa * ba - 2 * (ba - b) * (sa - s)
                }
            }
        };
        target[channel] = div255(value).min(alpha);
    }
    target[3] = alpha;
}

/// One surface being put together: the document, or a group.
struct Level {
    row: Vec<Pixel>,
    /// The latest unclipped surface and its opacity, while a clipped
    /// sibling still follows.
    base: Option<(Vec<Pixel>, u8)>,
}

/// For each step, whether a clipped sibling follows it before the next
/// unclipped one, so its row must be kept as the base.
fn bases_needed(steps: &[Step]) -> Vec<bool> {
    let mut needed = vec![false; steps.len()];
    let mut last_unclipped: Vec<Option<usize>> = vec![None];
    for (index, step) in steps.iter().enumerate() {
        let composite = match step {
            Step::Begin(_) => {
                last_unclipped.push(None);
                continue;
            }
            Step::End(_, composite) => {
                last_unclipped.pop();
                composite
            }
            Step::Paint(_, composite) => composite,
        };
        let last = last_unclipped.last_mut().expect("the document level");
        if composite.clipped {
            if let Some(base) = *last {
                needed[base] = true;
            }
        } else {
            *last = Some(index);
        }
    }
    needed
}

/// Puts `surfaces` (one per paint layer of `plan`, in plan order; `None` is
/// transparent) together over `background` within `rect` of `out`, on up to
/// `threads` threads.
pub fn evaluate(
    plan: &RenderPlan,
    background: Option<Pixel>,
    surfaces: &[Option<&Pixmap>],
    rect: PixelRect,
    out: &mut Pixmap,
    threads: usize,
) {
    assert_eq!(surfaces.len(), plan.layers.len());
    let width = usize::from(out.width());
    let [left, top, right, bottom] = rect.map(|value| value as usize);
    if left >= right || top >= bottom {
        return;
    }
    let needed = bases_needed(&plan.steps);
    let rows = (bottom - top).div_ceil(threads.max(1));
    let row_bytes = width * 4;
    let data = &mut out.data_as_u8_slice_mut()[top * row_bytes..bottom * row_bytes];
    let band = |first: usize, target: &mut [u8]| {
        let mut levels: Vec<Level> = Vec::new();
        for (offset, line) in target.chunks_mut(row_bytes).enumerate() {
            let y = first + offset;
            let line = &mut line.as_chunks_mut::<4>().0[left..right];
            evaluate_row(
                plan,
                &needed,
                background,
                surfaces,
                [left, right],
                y,
                &mut levels,
            );
            line.copy_from_slice(&levels[0].row);
        }
    };
    if threads <= 1 || bottom - top < 2 {
        band(top, data);
        return;
    }
    std::thread::scope(|scope| {
        for (index, chunk) in data.chunks_mut(rows * row_bytes).enumerate() {
            let band = &band;
            scope.spawn(move || band(top + index * rows, chunk));
        }
    });
}

fn evaluate_row(
    plan: &RenderPlan,
    needed: &[bool],
    background: Option<Pixel>,
    surfaces: &[Option<&Pixmap>],
    [left, right]: [usize; 2],
    y: usize,
    levels: &mut Vec<Level>,
) {
    let span = right - left;
    let mut depth = 0;
    let start = |depth: usize, levels: &mut Vec<Level>, fill: Pixel| {
        if levels.len() == depth {
            levels.push(Level {
                row: Vec::new(),
                base: None,
            });
        }
        let level = &mut levels[depth];
        level.row.clear();
        level.row.resize(span, fill);
        level.base = None;
    };
    start(0, levels, background.unwrap_or([0; 4]));
    let mut layer = 0;
    for (index, step) in plan.steps.iter().enumerate() {
        match step {
            Step::Begin(_) => {
                depth += 1;
                start(depth, levels, [0; 4]);
            }
            Step::Paint(_, composite) => {
                let surface = surfaces[layer];
                layer += 1;
                let empty = [[0; 4]; 0];
                let source = match surface {
                    Some(surface) => {
                        let from = (y * usize::from(surface.width()) + left) * 4;
                        surface.data_as_u8_slice()[from..from + span * 4]
                            .as_chunks::<4>()
                            .0
                    }
                    None => &empty[..],
                };
                put(&mut levels[depth], source, *composite, needed[index], span);
            }
            Step::End(_, composite) => {
                depth -= 1;
                let (lower, upper) = levels.split_at_mut(depth + 1);
                put(
                    &mut lower[depth],
                    &upper[0].row,
                    *composite,
                    needed[index],
                    span,
                );
            }
        }
    }
}

/// Puts `source` (empty for a transparent surface) over `level`.
fn put(level: &mut Level, source: &[Pixel], composite: Composite, keep: bool, span: usize) {
    let opacity = opacity_level(composite.opacity);
    let base = if composite.clipped {
        level.base.take()
    } else {
        None
    };
    for (at, target) in level.row.iter_mut().enumerate() {
        let Some(&pixel) = source.get(at) else {
            break;
        };
        if pixel[3] == 0 {
            continue;
        }
        let mut pixel = pixel;
        if let Some((base, base_opacity)) = &base {
            let cover = mul(base[at][3], *base_opacity);
            pixel = pixel.map(|value| mul(value, cover));
        }
        blend(
            composite.blend,
            target,
            pixel.map(|value| mul(value, opacity)),
        );
    }
    if composite.clipped {
        level.base = base;
    } else if keep {
        let mut kept = vec![[0; 4]; span];
        kept[..source.len()].copy_from_slice(source);
        level.base = Some((kept, opacity));
    } else {
        level.base = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The W3C formula in f32 on premultiplied values.
    fn reference(mode: Blend, source: Pixel, backdrop: Pixel) -> [f32; 4] {
        let s = source.map(|value| f32::from(value) / 255.0);
        let b = backdrop.map(|value| f32::from(value) / 255.0);
        let (sa, ba) = (s[3], b[3]);
        let mut out = [0.0; 4];
        for c in 0..3 {
            let mixed = match mode {
                Blend::Normal => s[c] * ba,
                Blend::Multiply => s[c] * b[c],
                Blend::Screen => s[c] * ba + b[c] * sa - s[c] * b[c],
                Blend::Overlay if 2.0 * b[c] <= ba => 2.0 * s[c] * b[c],
                Blend::Overlay => sa * ba - 2.0 * (ba - b[c]) * (sa - s[c]),
            };
            out[c] = s[c] * (1.0 - ba) + b[c] * (1.0 - sa) + mixed;
        }
        out[3] = sa + ba - sa * ba;
        out.map(|value| value * 255.0)
    }

    /// A premultiplied pixel from a counter.
    fn pixel(seed: u32) -> Pixel {
        let mut state = seed.wrapping_mul(2_654_435_761) ^ 0x9e37_79b9;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state >> 8) as u8
        };
        let alpha = next();
        [
            mul(next(), alpha),
            mul(next(), alpha),
            mul(next(), alpha),
            alpha,
        ]
    }

    #[test]
    fn blend_modes_are_within_one_level_of_the_formulas() {
        for mode in [
            Blend::Normal,
            Blend::Multiply,
            Blend::Screen,
            Blend::Overlay,
        ] {
            for seed in 0..20_000 {
                let (source, backdrop) = (pixel(seed), pixel(seed + 1_000_000));
                let mut target = backdrop;
                blend(mode, &mut target, source);
                let expected = reference(mode, source, backdrop);
                for channel in 0..4 {
                    let off = (f32::from(target[channel]) - expected[channel]).abs();
                    assert!(
                        off <= 1.0,
                        "{mode:?} {source:?} over {backdrop:?}: {target:?} vs {expected:?}"
                    );
                }
                assert!(target[..3].iter().all(|&value| value <= target[3]));
            }
        }
    }

    #[test]
    fn a_base_is_kept_only_while_clipped_siblings_follow() {
        use ugu_core::document::LayerId;
        let paint = |id, clipped| {
            Step::Paint(
                LayerId(id),
                Composite {
                    blend: Blend::Normal,
                    opacity: 1.0,
                    clipped,
                },
            )
        };
        let steps = [
            paint(1, false),
            paint(2, true),
            Step::Begin(LayerId(10)),
            paint(3, false),
            paint(4, false),
            Step::End(
                LayerId(10),
                Composite {
                    blend: Blend::Normal,
                    opacity: 1.0,
                    clipped: false,
                },
            ),
            paint(5, true),
            paint(6, false),
        ];
        assert_eq!(
            bases_needed(&steps),
            [true, false, false, false, false, true, false, false]
        );
    }
}
