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
use crate::tile::TiledSurface;

use crate::tile::Pixel;

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

/// Rows a thread puts together at a time.
const STRIPE: usize = 16;

/// Puts `surfaces` (one per paint layer of `plan`, in plan order; `None` is
/// transparent) together over `background` within `rect` of `out`, on up to
/// `threads` threads.
pub fn evaluate(
    plan: &RenderPlan,
    background: Option<Pixel>,
    surfaces: &[Option<&TiledSurface>],
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
    let row_bytes = width * 4;
    let data = &mut out.data_as_u8_slice_mut()[top * row_bytes..bottom * row_bytes];
    let stripe = |first: usize, target: &mut [u8], levels: &mut Vec<Level>| {
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
                levels,
            );
            line.copy_from_slice(&levels[0].row);
        }
    };
    if threads <= 1 || bottom - top <= STRIPE {
        stripe(top, data, &mut Vec::new());
        return;
    }
    let stripes: Vec<std::sync::Mutex<&mut [u8]>> = data
        .chunks_mut(STRIPE * row_bytes)
        .map(std::sync::Mutex::new)
        .collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..threads.min(stripes.len()) {
            let (stripe, stripes, next) = (&stripe, &stripes, &next);
            scope.spawn(move || {
                let mut levels = Vec::new();
                loop {
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(target) = stripes.get(index) else {
                        return;
                    };
                    let mut target = target.lock().expect("one thread per stripe");
                    stripe(top + index * STRIPE, &mut target, &mut levels);
                }
            });
        }
    });
}

fn evaluate_row(
    plan: &RenderPlan,
    needed: &[bool],
    background: Option<Pixel>,
    surfaces: &[Option<&TiledSurface>],
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
                let parts = surface.into_iter().flat_map(|surface| {
                    surface
                        .row(y as u32, left as u32, right as u32)
                        .map(|(x, part)| (x as usize - left, part))
                });
                put(&mut levels[depth], parts, *composite, needed[index], span);
            }
            Step::End(_, composite) => {
                depth -= 1;
                let (lower, upper) = levels.split_at_mut(depth + 1);
                let parts = std::iter::once((0, upper[0].row.as_slice()));
                put(&mut lower[depth], parts, *composite, needed[index], span);
            }
        }
    }
}

/// Puts `parts` of a surface (where each starts in the span, and its
/// pixels; transparent elsewhere) over `level`.
fn put<'a>(
    level: &mut Level,
    parts: impl Iterator<Item = (usize, &'a [Pixel])>,
    composite: Composite,
    keep: bool,
    span: usize,
) {
    let opacity = opacity_level(composite.opacity);
    let base = if composite.clipped {
        level.base.take()
    } else {
        None
    };
    let mut kept = (keep && !composite.clipped).then(|| vec![[0; 4]; span]);
    for (start, part) in parts {
        if let Some(kept) = kept.as_mut() {
            kept[start..start + part.len()].copy_from_slice(part);
        }
        let targets = &mut level.row[start..start + part.len()];
        for (offset, (target, &pixel)) in targets.iter_mut().zip(part).enumerate() {
            if pixel[3] == 0 {
                continue;
            }
            let mut pixel = pixel;
            if let Some((base, base_opacity)) = &base {
                let cover = mul(base[start + offset][3], *base_opacity);
                pixel = pixel.map(|value| mul(value, cover));
            }
            blend(
                composite.blend,
                target,
                pixel.map(|value| mul(value, opacity)),
            );
        }
    }
    level.base = if composite.clipped {
        base
    } else {
        kept.map(|kept| (kept, opacity))
    };
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
