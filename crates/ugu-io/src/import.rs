// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Still images brought into a document from a file or another app.
//!
//! A decoded image is turned upright by its EXIF orientation and made
//! smaller to fit the canvas and the asset limit before it becomes an asset,
//! so placing it needs only a whole-pixel move and any transform after that
//! resamples it once. Animated GIF and WebP give their first frame.

use std::io::Cursor;

use image::{DynamicImage, ImageDecoder, ImageReader, Limits, RgbaImage};
use sha2::{Digest, Sha256};
use ugu_core::ops::AssetId;
use ugu_core::store::{Asset, limits};

/// The largest image read, each way.
pub const EDGE: u32 = 16_384;
/// What decoding may allocate.
const ALLOCATION: u64 = 1024 * 1024 * 1024;

#[derive(Debug, PartialEq)]
pub enum ImportError {
    /// Not a format 3.0 reads, or not an image.
    Unsupported,
    /// Larger than `EDGE` each way or than decoding may allocate.
    TooLarge,
    Damaged(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => write!(f, "the image format is not supported"),
            Self::TooLarge => write!(f, "the image is larger than {EDGE}x{EDGE}"),
            Self::Damaged(reason) => write!(f, "the image could not be decoded: {reason}"),
        }
    }
}

impl std::error::Error for ImportError {}

/// Decodes a png, jpg, webp, bmp, gif or tiff file's bytes, upright, as
/// an asset no larger than `fit`.
pub fn decode(bytes: &[u8], fit: [u32; 2]) -> Result<(AssetId, Asset), ImportError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| ImportError::Damaged(error.to_string()))?;
    let mut bounds = Limits::default();
    bounds.max_image_width = Some(EDGE);
    bounds.max_image_height = Some(EDGE);
    bounds.max_alloc = Some(ALLOCATION);
    reader.limits(bounds);
    let mut decoder = reader.into_decoder().map_err(error)?;
    let orientation = decoder.orientation().map_err(error)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(error)?;
    image.apply_orientation(orientation);
    to_asset(image.into_rgba8(), fit)
}

fn error(error: image::ImageError) -> ImportError {
    match error {
        image::ImageError::Unsupported(_) => ImportError::Unsupported,
        image::ImageError::Limits(_) => ImportError::TooLarge,
        other => ImportError::Damaged(other.to_string()),
    }
}

/// Straight RGBA pixels of `size` as an asset no larger than `fit`.
pub fn from_rgba(
    size: [u32; 2],
    straight: Vec<u8>,
    fit: [u32; 2],
) -> Result<(AssetId, Asset), ImportError> {
    if size.iter().any(|&edge| edge > EDGE) {
        return Err(ImportError::TooLarge);
    }
    let image = RgbaImage::from_raw(size[0], size[1], straight)
        .ok_or_else(|| ImportError::Damaged("the pixels do not match the size".to_owned()))?;
    to_asset(image, fit)
}

fn to_asset(image: RgbaImage, fit: [u32; 2]) -> Result<(AssetId, Asset), ImportError> {
    let size = [image.width(), image.height()];
    if size.contains(&0) {
        return Err(ImportError::Damaged("the image is empty".to_owned()));
    }
    if size.iter().any(|&edge| edge > EDGE) {
        return Err(ImportError::TooLarge);
    }
    let image = match fitted(size, fit) {
        Some(smaller) => shrink(&image, smaller),
        None => image,
    };
    let size = [image.width(), image.height()];
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, size[0], size[1]);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|error| ImportError::Damaged(error.to_string()))?;
    writer
        .write_image_data(image.as_raw())
        .map_err(|error| ImportError::Damaged(error.to_string()))?;
    writer
        .finish()
        .map_err(|error| ImportError::Damaged(error.to_string()))?;
    let id = AssetId(Sha256::digest(&png).into());
    Ok((
        id,
        Asset {
            size,
            png: png.into(),
        },
    ))
}

/// The size `size` shrinks to so that it fits `fit` and the asset limit,
/// keeping its shape; `None` when it fits as it is.
fn fitted(size: [u32; 2], fit: [u32; 2]) -> Option<[u32; 2]> {
    let [width, height] = size.map(f64::from);
    let pixels = (limits::ASSET_PIXELS as f64 / (width * height)).sqrt();
    let scale = (f64::from(fit[0]) / width)
        .min(f64::from(fit[1]) / height)
        .min(pixels);
    (scale < 1.0).then(|| size.map(|edge| ((f64::from(edge) * scale).floor() as u32).max(1)))
}

/// Shrinks with each target pixel the average of the source area it covers,
/// in premultiplied colour so that transparent pixels lend no colour.
fn shrink(image: &RgbaImage, size: [u32; 2]) -> RgbaImage {
    let [width, height] = [image.width(), image.height()];
    let scale = [
        f64::from(width) / f64::from(size[0]),
        f64::from(height) / f64::from(size[1]),
    ];
    // How much of each source pixel along one axis falls in each target pixel.
    let spans = |source: u32, target: u32, scale: f64| -> Vec<Vec<(u32, f64)>> {
        (0..target)
            .map(|index| {
                let (from, to) = (f64::from(index) * scale, f64::from(index + 1) * scale);
                (from.floor() as u32..(to.ceil() as u32).min(source))
                    .map(|pixel| {
                        let share =
                            (to.min(f64::from(pixel + 1)) - from.max(f64::from(pixel))) / scale;
                        (pixel, share)
                    })
                    .filter(|&(_, share)| share > 0.0)
                    .collect()
            })
            .collect()
    };
    let columns = spans(width, size[0], scale[0]);
    let rows = spans(height, size[1], scale[1]);
    let source = image.as_raw();
    let mut out = RgbaImage::new(size[0], size[1]);
    for (y, row) in rows.iter().enumerate() {
        for (x, column) in columns.iter().enumerate() {
            let mut sum = [0.0f64; 4];
            for &(sy, wy) in row {
                for &(sx, wx) in column {
                    let at = (sy as usize * width as usize + sx as usize) * 4;
                    let alpha = f64::from(source[at + 3]) / 255.0;
                    let weight = wx * wy;
                    for (channel, total) in sum.iter_mut().take(3).enumerate() {
                        *total += f64::from(source[at + channel]) * alpha * weight;
                    }
                    sum[3] += alpha * weight;
                }
            }
            let pixel = if sum[3] > 0.0 {
                let alpha = sum[3];
                [
                    (sum[0] / alpha).round() as u8,
                    (sum[1] / alpha).round() as u8,
                    (sum[2] / alpha).round() as u8,
                    (alpha * 255.0).round() as u8,
                ]
            } else {
                [0; 4]
            };
            out.put_pixel(x as u32, y as u32, image::Rgba(pixel));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageEncoder, codecs};

    fn picture(size: [u32; 2]) -> RgbaImage {
        RgbaImage::from_fn(size[0], size[1], |x, y| {
            image::Rgba([(x * 40) as u8, (y * 40) as u8, 90, 255])
        })
    }

    fn pixels(asset: &Asset) -> RgbaImage {
        image::load_from_memory(&asset.png).unwrap().into_rgba8()
    }

    #[test]
    fn each_format_decodes_to_the_same_pixels() {
        let original = picture([5, 3]);
        let raw = original.as_raw();
        let mut files: Vec<(&str, Vec<u8>)> = Vec::new();
        let mut encode = |name, encoder: &dyn Fn(&mut Vec<u8>)| {
            let mut bytes = Vec::new();
            encoder(&mut bytes);
            files.push((name, bytes));
        };
        encode("png", &|out| {
            codecs::png::PngEncoder::new(out)
                .write_image(raw, 5, 3, image::ExtendedColorType::Rgba8)
                .unwrap();
        });
        encode("bmp", &|out| {
            codecs::bmp::BmpEncoder::new(out)
                .write_image(raw, 5, 3, image::ExtendedColorType::Rgba8)
                .unwrap();
        });
        encode("tiff", &|out| {
            codecs::tiff::TiffEncoder::new(Cursor::new(out))
                .write_image(raw, 5, 3, image::ExtendedColorType::Rgba8)
                .unwrap();
        });
        encode("webp", &|out| {
            codecs::webp::WebPEncoder::new_lossless(out)
                .write_image(raw, 5, 3, image::ExtendedColorType::Rgba8)
                .unwrap();
        });
        for (name, bytes) in &files {
            let (id, asset) = decode(bytes, [100, 100]).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(asset.size, [5, 3], "{name}");
            assert_eq!(pixels(&asset), original, "{name}");
            assert_eq!(id, AssetId(Sha256::digest(&asset.png).into()));
        }
        // The same pixels make the same asset whatever the file.
        let ids: Vec<_> = files
            .iter()
            .map(|(_, bytes)| decode(bytes, [100, 100]).unwrap().0)
            .collect();
        assert!(ids.windows(2).all(|pair| pair[0] == pair[1]));
        // Lossy formats decode too.
        let mut jpeg = Vec::new();
        codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .write_image(
                &DynamicImage::ImageRgba8(original.clone()).into_rgb8(),
                5,
                3,
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
        assert_eq!(decode(&jpeg, [100, 100]).unwrap().1.size, [5, 3]);
        let mut gif = Vec::new();
        codecs::gif::GifEncoder::new(&mut gif)
            .encode(raw, 5, 3, image::ExtendedColorType::Rgba8)
            .unwrap();
        assert_eq!(decode(&gif, [100, 100]).unwrap().1.size, [5, 3]);
    }

    #[test]
    fn an_exif_orientation_turns_the_image_upright() {
        // A 2x1 JPEG with orientation 6 (turned a quarter clockwise to view).
        let mut jpeg = Vec::new();
        let mut encoder = codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 100);
        let mut exif = b"MM\0\x2a\0\0\0\x08\0\x01".to_vec();
        exif.extend_from_slice(&[0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, 6, 0, 0, 0, 0, 0, 0]);
        encoder.set_exif_metadata(exif).unwrap();
        let wide = image::RgbImage::from_fn(16, 8, |x, _| {
            if x < 8 {
                image::Rgb([255, 0, 0])
            } else {
                image::Rgb([0, 0, 255])
            }
        });
        encoder
            .write_image(wide.as_raw(), 16, 8, image::ExtendedColorType::Rgb8)
            .unwrap();
        let (_, asset) = decode(&jpeg, [100, 100]).unwrap();
        assert_eq!(asset.size, [8, 16]);
        let upright = pixels(&asset);
        // Red was on the left; turned clockwise it is at the top.
        assert!(upright.get_pixel(4, 2)[0] > 200);
        assert!(upright.get_pixel(4, 13)[2] > 200);
    }

    #[test]
    fn a_larger_image_shrinks_to_fit_keeping_its_shape() {
        let (_, asset) = to_asset(picture([400, 100]), [100, 100]).unwrap();
        assert_eq!(asset.size, [100, 25]);
        let (_, asset) = to_asset(picture([40, 10]), [100, 100]).unwrap();
        assert_eq!(asset.size, [40, 10]);
        // Shrinking averages: a checkerboard becomes grey, and transparent
        // pixels lend no colour.
        let checker = RgbaImage::from_fn(8, 8, |x, y| {
            if (x + y) % 2 == 0 {
                image::Rgba([255, 255, 255, 255])
            } else {
                image::Rgba([0, 0, 0, 255])
            }
        });
        let (_, asset) = to_asset(checker, [4, 4]).unwrap();
        assert!(
            pixels(&asset)
                .pixels()
                .all(|pixel| pixel.0 == [128, 128, 128, 255])
        );
        let half = RgbaImage::from_fn(2, 1, |x, _| {
            if x == 0 {
                image::Rgba([200, 10, 10, 255])
            } else {
                image::Rgba([0, 255, 0, 0])
            }
        });
        let (_, asset) = to_asset(half, [1, 1]).unwrap();
        assert_eq!(pixels(&asset).get_pixel(0, 0).0, [200, 10, 10, 128]);
        // The asset limit applies even where the canvas would allow more.
        assert_eq!(fitted([8192, 8192], [8192, 8192]), Some([4096, 4096]));
    }

    #[test]
    fn what_is_not_an_image_or_too_large_is_refused() {
        assert_eq!(
            decode(b"not an image", [10, 10]).unwrap_err(),
            ImportError::Unsupported
        );
        let mut truncated = Vec::new();
        codecs::png::PngEncoder::new(&mut truncated)
            .write_image(
                picture([5, 3]).as_raw(),
                5,
                3,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        truncated.truncate(truncated.len() / 2);
        assert!(matches!(
            decode(&truncated, [10, 10]),
            Err(ImportError::Damaged(_))
        ));
        // A header claiming a huge image is refused before decoding.
        let header = bmp_header(EDGE + 1, 1);
        assert_eq!(
            decode(&header, [10, 10]).unwrap_err(),
            ImportError::TooLarge
        );
        assert_eq!(
            from_rgba([0, 4], Vec::new(), [10, 10]).unwrap_err(),
            ImportError::Damaged("the image is empty".to_owned())
        );
        assert!(matches!(
            from_rgba([2, 2], vec![0; 4], [10, 10]),
            Err(ImportError::Damaged(_))
        ));
    }

    /// A BMP header for a 32-bit image of `width` x `height` with no pixels.
    fn bmp_header(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"BM".to_vec();
        bytes.extend_from_slice(&(54u32 + width * height * 4).to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&54u32.to_le_bytes());
        bytes.extend_from_slice(&40u32.to_le_bytes());
        bytes.extend_from_slice(&(width as i32).to_le_bytes());
        bytes.extend_from_slice(&(height as i32).to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&32u16.to_le_bytes());
        bytes.extend_from_slice(&[0; 24]);
        bytes
    }
}
