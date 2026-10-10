// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Still image export.
//!
//! Frames are rendered premultiplied; PNG stores straight alpha. A fully
//! transparent pixel has no colour, so it is written as transparent black.
//! JPEG keeps no alpha: what is left transparent is put over white, the
//! paper drawn on, as 2.2.13 does, at 2.2.13's quality.

use std::io::{BufWriter, Write};
use std::path::Path;

use crate::save::{Replace, SaveError, replace_with};
use crate::write::WriteError;

/// One premultiplied pixel as straight alpha, rounded to nearest.
pub fn unpremultiply([r, g, b, a]: [u8; 4]) -> [u8; 4] {
    if a == 0 {
        return [0; 4];
    }
    let straight =
        |channel: u8| ((u32::from(channel) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8;
    [straight(r), straight(g), straight(b), a]
}

/// Writes premultiplied RGBA8 rows of `size` as an 8-bit RGBA PNG.
pub fn encode_png(
    pixels: &[u8],
    size: [u32; 2],
    out: impl Write,
) -> Result<(), png::EncodingError> {
    let mut encoder = png::Encoder::new(out, size[0], size[1]);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder.write_header()?;
    let straight: Vec<u8> = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&pixel| unpremultiply(pixel))
        .collect();
    writer.write_image_data(&straight)?;
    writer.finish()
}

pub const JPEG_QUALITY: u8 = 92;

/// For each pixel along an edge of `to`, the first pixel of `from` it
/// covers and how much of each, in parts of 2^16 of the whole.
fn spans(from: u32, to: u32) -> Vec<(usize, Vec<u32>)> {
    let scale = f64::from(from) / f64::from(to);
    (0..to)
        .map(|index| {
            let start = f64::from(index) * scale;
            let end = start + scale;
            let first = start.floor() as usize;
            let last = (end.ceil() as usize).min(from as usize);
            let mut weights: Vec<u32> = (first..last)
                .map(|pixel| {
                    let covered = (end.min(pixel as f64 + 1.0) - start.max(pixel as f64)).max(0.0);
                    (covered / scale * 65536.0).round() as u32
                })
                .collect();
            // Rounding leaves the sum a little off; the largest part takes it.
            let sum: u32 = weights.iter().sum();
            if let Some(largest) = weights.iter_mut().max() {
                *largest = (*largest + 65536).saturating_sub(sum);
            }
            (first, weights)
        })
        .collect()
}

/// Premultiplied RGBA8 rows of `from` made `to`, no larger, each pixel the
/// average of the area it covers. Bands of rows are shrunk on several
/// threads, each holding only the few source rows its next row covers.
pub fn shrink(pixels: Vec<u8>, from: [u32; 2], to: [u32; 2]) -> Vec<u8> {
    if from == to {
        return pixels;
    }
    let columns = spans(from[0], to[0]);
    let rows = spans(from[1], to[1]);
    let source = pixels.as_chunks::<4>().0;
    let row_bytes = to[0] as usize * 4;
    let mut out = vec![0; row_bytes * to[1] as usize];
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let band = rows.len().div_ceil(threads);
    std::thread::scope(|scope| {
        for (rows, out) in rows.chunks(band).zip(out.chunks_mut(band * row_bytes)) {
            let columns = &columns;
            scope.spawn(move || shrink_rows(source, from[0] as usize, columns, rows, out));
        }
    });
    out
}

/// `rows` of the shrunk image into `out`.
fn shrink_rows(
    source: &[[u8; 4]],
    from_width: usize,
    columns: &[(usize, Vec<u32>)],
    rows: &[(usize, Vec<u32>)],
    out: &mut [u8],
) {
    let width = columns.len();
    // Source rows from `top` on, summed across in 2^16 parts.
    let mut across: Vec<[u32; 4]> = Vec::new();
    let mut top = 0;
    for ((first, weights), out) in rows.iter().zip(out.chunks_exact_mut(width * 4)) {
        let passed = first.saturating_sub(top).min(across.len() / width);
        across.drain(..passed * width);
        top = if across.is_empty() {
            *first
        } else {
            top + passed
        };
        while top + across.len() / width < first + weights.len() {
            let y = top + across.len() / width;
            let row = &source[y * from_width..][..from_width];
            across.extend(columns.iter().map(|(first, weights)| {
                let mut sum = [0u32; 4];
                for (pixel, &weight) in row[*first..].iter().zip(weights) {
                    for channel in 0..4 {
                        sum[channel] += u32::from(pixel[channel]) * weight;
                    }
                }
                sum.map(|channel| (channel + 128) >> 8)
            }));
        }
        let start = (first - top) * width;
        for (x, out) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let mut sum = [0u64; 4];
            for (offset, &weight) in weights.iter().enumerate() {
                let pixel = across[start + offset * width + x];
                for channel in 0..4 {
                    sum[channel] += u64::from(pixel[channel]) * u64::from(weight);
                }
            }
            // 2^8 parts across times 2^16 down.
            *out = sum.map(|channel| ((channel + (1 << 23)) >> 24).min(255) as u8);
        }
    }
}

/// The file formats a still image is exported as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
}

impl Format {
    /// By the file name's extension; `None` for one that is neither.
    pub fn of(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            _ => None,
        }
    }
}

/// Premultiplied RGBA8 over opaque white, as RGB8.
pub fn over_white(pixels: &[u8]) -> Vec<u8> {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| {
            let paper = 255 - a;
            [r, g, b].map(|channel| channel.saturating_add(paper))
        })
        .collect()
}

/// Writes premultiplied RGBA8 rows of `size` as a JPEG over white.
pub fn encode_jpeg(
    pixels: &[u8],
    size: [u32; 2],
    out: impl Write,
) -> Result<(), ::image::ImageError> {
    let rgb = over_white(pixels);
    ::image::codecs::jpeg::JpegEncoder::new_with_quality(out, JPEG_QUALITY).encode(
        &rgb,
        size[0],
        size[1],
        ::image::ExtendedColorType::Rgb8,
    )
}

/// Exports to `target` without risking the file already there.
pub fn export(
    pixels: &[u8],
    size: [u32; 2],
    format: Format,
    target: &Path,
    replace: Replace<'_>,
) -> Result<(), SaveError> {
    let failed = |error: String| SaveError::Write(WriteError::Io(std::io::Error::other(error)));
    replace_with(target, replace, |file, _| {
        let mut out = BufWriter::new(file);
        match format {
            Format::Png => {
                encode_png(pixels, size, &mut out).map_err(|error| failed(error.to_string()))?;
            }
            Format::Jpeg => {
                encode_jpeg(pixels, size, &mut out).map_err(|error| failed(error.to_string()))?;
            }
        }
        out.flush()?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn premultiply([r, g, b, a]: [u8; 4]) -> [u8; 4] {
        let times = |channel: u8| ((u32::from(channel) * u32::from(a) + 127) / 255) as u8;
        [times(r), times(g), times(b), a]
    }

    #[test]
    fn straight_alpha_round_trips_within_a_level() {
        for a in 1..=255u8 {
            for c in (0..=255u8).step_by(5) {
                let premultiplied = premultiply([c, 255 - c, c / 2, a]);
                let back = premultiply(unpremultiply(premultiplied));
                for channel in 0..4 {
                    assert!(back[channel].abs_diff(premultiplied[channel]) <= 1);
                }
            }
        }
        assert_eq!(unpremultiply([10, 20, 30, 255]), [10, 20, 30, 255]);
        assert_eq!(unpremultiply([0, 0, 0, 0]), [0; 4]);
    }

    #[test]
    fn the_png_decodes_to_the_same_image() {
        let size = [3, 2];
        let pixels: Vec<u8> = [
            [255, 0, 0, 255],
            [0, 0, 0, 0],
            [64, 32, 0, 128],
            [10, 20, 30, 255],
            [1, 1, 1, 1],
            [200, 100, 50, 200],
        ]
        .concat();
        let mut bytes = Vec::new();
        encode_png(&pixels, size, &mut bytes).unwrap();
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap();
        let mut decoded = vec![0; decoder.output_buffer_size().unwrap()];
        let info = decoder.next_frame(&mut decoded).unwrap();
        assert_eq!([info.width, info.height], size);
        assert_eq!(info.color_type, png::ColorType::Rgba);
        for (straight, premultiplied) in decoded.chunks(4).zip(pixels.chunks(4)) {
            let premultiplied: [u8; 4] = premultiplied.try_into().unwrap();
            assert_eq!(straight, unpremultiply(premultiplied));
        }
    }

    #[test]
    fn transparency_goes_over_white() {
        let pixels = [[0, 0, 0, 0], [64, 32, 0, 128], [10, 20, 30, 255]].concat();
        assert_eq!(
            over_white(&pixels),
            [255, 255, 255, 191, 159, 127, 10, 20, 30]
        );
    }

    #[test]
    fn the_jpeg_decodes_close_to_the_image_over_white() {
        // Flat areas, which JPEG keeps within a few levels.
        let size = [32, 16];
        let pixels: Vec<u8> = (0..size[0] * size[1])
            .flat_map(|index| {
                if index % size[0] < 16 {
                    [200, 40, 40, 255]
                } else {
                    [0, 0, 0, 0]
                }
            })
            .collect();
        let mut bytes = Vec::new();
        encode_jpeg(&pixels, size, &mut bytes).unwrap();
        let decoded = ::image::load_from_memory(&bytes).unwrap().to_rgb8();
        assert_eq!(decoded.dimensions(), (32, 16));
        let expected = over_white(&pixels);
        // Away from the edge between the halves.
        for y in 0..16 {
            for x in (0..6).chain(26..32) {
                let at = (y * 32 + x) as usize * 3;
                for channel in 0..3 {
                    let difference =
                        decoded.as_raw()[at + channel].abs_diff(expected[at + channel]);
                    assert!(difference <= 6, "{difference} at {x}, {y}");
                }
            }
        }
    }

    #[test]
    fn shrinking_averages_the_area_each_pixel_covers() {
        let flat: Vec<u8> = [[40, 80, 120, 200]; 12].concat();
        assert_eq!(
            shrink(flat.clone(), [4, 3], [2, 1]),
            [[40, 80, 120, 200]; 2].concat()
        );
        // Two by two into one.
        let square = [
            [0, 0, 0, 0],
            [100, 0, 0, 100],
            [0, 200, 0, 200],
            [255, 255, 255, 255],
        ]
        .concat();
        assert_eq!(shrink(square, [2, 2], [1, 1]), [89, 114, 64, 139]);
        // Three into two: the middle one is shared half and half.
        let row = [[0, 0, 0, 255], [90, 90, 90, 255], [180, 180, 180, 255]].concat();
        assert_eq!(
            shrink(row, [3, 1], [2, 1]),
            [[30, 30, 30, 255], [150, 150, 150, 255]].concat()
        );
        assert_eq!(shrink(flat.clone(), [4, 3], [4, 3]), flat);
    }

    /// The whole image summed across, then down, as one pass.
    fn shrink_whole(pixels: &[u8], from: [u32; 2], to: [u32; 2]) -> Vec<u8> {
        let columns = spans(from[0], to[0]);
        let rows = spans(from[1], to[1]);
        let source = pixels.as_chunks::<4>().0;
        let mut across = vec![[0u32; 4]; (to[0] * from[1]) as usize];
        for y in 0..from[1] as usize {
            let row = &source[y * from[0] as usize..][..from[0] as usize];
            for (x, (first, weights)) in columns.iter().enumerate() {
                let mut sum = [0u32; 4];
                for (pixel, &weight) in row[*first..].iter().zip(weights) {
                    for channel in 0..4 {
                        sum[channel] += u32::from(pixel[channel]) * weight;
                    }
                }
                across[y * to[0] as usize + x] = sum.map(|channel| (channel + 128) >> 8);
            }
        }
        let mut out = Vec::new();
        for (first, weights) in &rows {
            for x in 0..to[0] as usize {
                let mut sum = [0u64; 4];
                for (offset, &weight) in weights.iter().enumerate() {
                    let pixel = across[(first + offset) * to[0] as usize + x];
                    for channel in 0..4 {
                        sum[channel] += u64::from(pixel[channel]) * u64::from(weight);
                    }
                }
                out.extend(sum.map(|channel| ((channel + (1 << 23)) >> 24).min(255) as u8));
            }
        }
        out
    }

    #[test]
    fn shrinking_in_bands_is_shrinking_the_whole() {
        let from = [301, 257];
        let pixels: Vec<u8> = (0..from[0] * from[1])
            .flat_map(|index| {
                let alpha = (index * 7 % 256) as u8;
                let colour = |seed: u32| ((index * seed % 251) as u8).min(alpha);
                [colour(13), colour(29), colour(47), alpha]
            })
            .collect();
        for percent in [75, 50, 33, 25, 1] {
            let to = from.map(|edge| (edge * percent / 100).max(1));
            assert_eq!(
                shrink(pixels.clone(), from, to),
                shrink_whole(&pixels, from, to),
                "{percent}%"
            );
        }
    }

    #[test]
    fn the_format_follows_the_extension() {
        assert_eq!(Format::of(Path::new("a.PNG")), Some(Format::Png));
        assert_eq!(Format::of(Path::new("a.jpeg")), Some(Format::Jpeg));
        assert_eq!(Format::of(Path::new("a.JPG")), Some(Format::Jpeg));
        assert_eq!(Format::of(Path::new("a.bmp")), None);
        assert_eq!(Format::of(Path::new("a")), None);
    }
}
