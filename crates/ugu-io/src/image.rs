// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Still image export.
//!
//! Frames are rendered premultiplied; PNG stores straight alpha. A fully
//! transparent pixel has no colour, so it is written as transparent black.

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

/// Exports to `target` without risking the file already there.
pub fn export_png(
    pixels: &[u8],
    size: [u32; 2],
    target: &Path,
    replace: Replace<'_>,
) -> Result<(), SaveError> {
    replace_with(target, replace, |file, _| {
        let mut out = BufWriter::new(file);
        encode_png(pixels, size, &mut out).map_err(|error| {
            SaveError::Write(WriteError::Io(std::io::Error::other(error.to_string())))
        })?;
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
}
