// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Raster assets as pixels to draw: each stored PNG decoded once into a
//! premultiplied pixmap of the size the document records for it.

use ugu_core::store::Asset;
use vello_cpu::Pixmap;

use crate::compose::premultiplied;

#[derive(Debug, PartialEq, Eq)]
pub enum ImageError {
    Decode(String),
    /// The PNG is not 8-bit RGBA, as `.ugurugu` stores images.
    Format,
    /// The PNG's size is not the size the document records.
    Size([u32; 2]),
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decode(error) => write!(f, "the image cannot be read: {error}"),
            Self::Format => f.write_str("the image is not 8-bit RGBA"),
            Self::Size([width, height]) => {
                write!(f, "the image is {width}×{height}, not the size recorded")
            }
        }
    }
}

/// `asset`'s pixels, premultiplied.
pub fn decode(asset: &Asset) -> Result<Pixmap, ImageError> {
    let decoder = png::Decoder::new(std::io::Cursor::new(&asset.png[..]));
    let mut reader = decoder
        .read_info()
        .map_err(|error| ImageError::Decode(error.to_string()))?;
    let info = reader.info();
    let size = [info.width, info.height];
    if (info.color_type, info.bit_depth) != (png::ColorType::Rgba, png::BitDepth::Eight) {
        return Err(ImageError::Format);
    }
    // Checked before the buffer is made, so a file cannot ask for more than
    // the size the store has already limited.
    if size != asset.size {
        return Err(ImageError::Size(size));
    }
    let [width, height] = size.map(|edge| u16::try_from(edge).map_err(|_| ImageError::Size(size)));
    let (width, height) = (width?, height?);
    let mut straight = vec![0; reader.output_buffer_size().ok_or(ImageError::Size(size))?];
    reader
        .next_frame(&mut straight)
        .map_err(|error| ImageError::Decode(error.to_string()))?;
    let mut pixmap = Pixmap::new(width, height);
    for (to, from) in pixmap
        .data_as_u8_slice_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(straight.as_chunks::<4>().0)
    {
        *to = premultiplied(*from);
    }
    Ok(pixmap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn png(size: [u32; 2], pixels: &[[u8; 4]]) -> Arc<[u8]> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, size[0], size[1]);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&pixels.iter().flatten().copied().collect::<Vec<u8>>())
            .unwrap();
        writer.finish().unwrap();
        Arc::from(bytes)
    }

    #[test]
    fn an_asset_decodes_premultiplied_at_its_recorded_size() {
        let pixels = [
            [255, 0, 0, 255],
            [0, 255, 0, 128],
            [0, 0, 255, 0],
            [10, 20, 30, 40],
        ];
        let asset = Asset {
            size: [2, 2],
            png: png([2, 2], &pixels),
        };
        let decoded = decode(&asset).unwrap();
        let got: Vec<[u8; 4]> = decoded.data_as_u8_slice().as_chunks::<4>().0.to_vec();
        let expected: Vec<[u8; 4]> = pixels.iter().map(|&pixel| premultiplied(pixel)).collect();
        assert_eq!(got, expected);
        let wrong = Asset {
            size: [4, 1],
            ..asset
        };
        assert_eq!(decode(&wrong).unwrap_err(), ImageError::Size([2, 2]));
        let broken = Asset {
            size: [2, 2],
            png: Arc::from(&b"not a png"[..]),
        };
        assert!(matches!(decode(&broken), Err(ImageError::Decode(_))));
    }
}
