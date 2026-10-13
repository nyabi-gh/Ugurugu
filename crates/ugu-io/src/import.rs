// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Still images brought into a document from a file or another app.
//!
//! A decoded image is turned upright by its EXIF orientation and made
//! smaller to fit the canvas and the asset limit before it becomes an asset,
//! so placing it needs only a whole-pixel move and any transform after that
//! resamples it once. Animated GIF and WebP give their first frame.
//!
//! The decoded image is the one large buffer: the file is read as it is
//! decoded, and the image is made smaller a band of rows at a time before it
//! is turned, so neither a copy of the file nor a full-size copy in another
//! pixel format or way up is made.

use std::borrow::Cow;
use std::io::{BufRead, Seek};

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader, Limits, RgbaImage};
use sha2::{Digest, Sha256};
use ugu_core::ops::AssetId;
use ugu_core::store::{Asset, limits};

/// The largest image read, each way.
pub const EDGE: u32 = 16_384;
/// What decoding may allocate, and the most of an encoded image taken from
/// the clipboard.
pub const ALLOCATION: u64 = 1024 * 1024 * 1024;

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

/// Decodes a png, jpg, webp, bmp, gif or tiff file, upright, as an asset no
/// larger than `fit`.
pub fn decode(file: impl BufRead + Seek, fit: [u32; 2]) -> Result<(AssetId, Asset), ImportError> {
    let mut reader = ImageReader::new(file)
        .with_guessed_format()
        .map_err(|error| ImportError::Damaged(error.to_string()))?;
    reader.limits(bounds());
    let mut decoder = reader.into_decoder().map_err(error)?;
    let orientation = decoder.orientation().map_err(error)?;
    let image = DynamicImage::from_decoder(decoder).map_err(error)?;
    to_asset(image, orientation, fit)
}

/// Decodes a `CF_DIBV5` clipboard image, as an asset no larger than `fit`.
pub fn decode_dib(mut dib: Vec<u8>, fit: [u32; 2]) -> Result<(AssetId, Asset), ImportError> {
    alpha_in_bi_rgb(&mut dib);
    let bmp =
        with_file_header(dib).ok_or_else(|| ImportError::Damaged("a broken bitmap".to_owned()))?;
    decode(std::io::Cursor::new(bmp), fit)
}

fn field(dib: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(*dib.get(at..at + 4)?.first_chunk()?))
}

/// The DIB as a BMP file, whose header says where the pixels start: after
/// the masks and colours, which writers lay out differently, and before an
/// embedded colour profile, so at the end of what is left. `None` for a
/// compressed or broken one.
fn with_file_header(dib: Vec<u8>) -> Option<Vec<u8>> {
    const BI_RGB: u32 = 0;
    const BI_BITFIELDS: u32 = 3;
    const PROFILE_EMBEDDED: u32 = u32::from_be_bytes(*b"MBED");
    let header = field(&dib, 0)? as usize;
    let width = field(&dib, 4)? as i32;
    let height = field(&dib, 8)? as i32;
    let bits = u32::from(u16::from_le_bytes(*dib.get(14..16)?.first_chunk()?));
    if header < 40 || header > dib.len() || ![BI_RGB, BI_BITFIELDS].contains(&field(&dib, 16)?) {
        return None;
    }
    let stride = (u64::from(width.unsigned_abs()) * u64::from(bits)).div_ceil(32) * 4;
    let pixels = stride.checked_mul(u64::from(height.unsigned_abs()))?;
    let profile =
        field(&dib, 112).filter(|_| header >= 124 && field(&dib, 60) == Some(PROFILE_EMBEDDED));
    let end = match profile {
        Some(at) if (at as usize) >= header && (at as usize) < dib.len() => at as usize,
        _ => dib.len(),
    };
    let start = (end as u64)
        .checked_sub(pixels)
        .filter(|&start| start >= header as u64)?;
    let mut bmp = Vec::with_capacity(14 + dib.len());
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&u32::try_from(14 + dib.len()).ok()?.to_le_bytes());
    bmp.extend_from_slice(&[0; 4]);
    bmp.extend_from_slice(&u32::try_from(14 + start).ok()?.to_le_bytes());
    bmp.extend_from_slice(&dib);
    Some(bmp)
}

/// Chromium and apps built on it put 32-bit `BI_RGB` images with an alpha
/// mask, whose high bytes the format says to ignore, on the clipboard with
/// their transparency in them; read as bit fields, as arboard does.
fn alpha_in_bi_rgb(dib: &mut [u8]) {
    const BI_RGB: u32 = 0;
    const BI_BITFIELDS: u32 = 3;
    let v5 = field(dib, 0).is_some_and(|size| size >= 124) && dib.len() >= 124;
    if !v5
        || u16::from_le_bytes([dib[14], dib[15]]) != 32
        || field(dib, 16) != Some(BI_RGB)
        || field(dib, 52) != Some(0xff00_0000)
    {
        return;
    }
    dib[16..20].copy_from_slice(&BI_BITFIELDS.to_le_bytes());
    if (40..52).step_by(4).all(|at| field(dib, at) == Some(0)) {
        for (at, mask) in [(40, 0x00ff_0000u32), (44, 0x0000_ff00), (48, 0x0000_00ff)] {
            dib[at..at + 4].copy_from_slice(&mask.to_le_bytes());
        }
    }
}

fn bounds() -> Limits {
    let mut bounds = Limits::default();
    bounds.max_image_width = Some(EDGE);
    bounds.max_image_height = Some(EDGE);
    bounds.max_alloc = Some(ALLOCATION);
    bounds
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
    to_asset(
        DynamicImage::ImageRgba8(image),
        Orientation::NoTransforms,
        fit,
    )
}

/// Whether `orientation` turns the image a quarter, swapping its sides.
fn swaps(orientation: Orientation) -> bool {
    matches!(
        orientation,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    )
}

/// `image` turned by `orientation` and fitted to `fit`, as an asset. It is
/// made smaller first, so it is turned at its final size.
fn to_asset(
    image: DynamicImage,
    orientation: Orientation,
    fit: [u32; 2],
) -> Result<(AssetId, Asset), ImportError> {
    let size = [image.width(), image.height()];
    if size.contains(&0) {
        return Err(ImportError::Damaged("the image is empty".to_owned()));
    }
    if size.iter().any(|&edge| edge > EDGE) {
        return Err(ImportError::TooLarge);
    }
    let upright = if swaps(orientation) {
        [size[1], size[0]]
    } else {
        size
    };
    let image = match fitted(upright, fit) {
        Some([width, height]) if swaps(orientation) => shrink(&image, [height, width]),
        Some(smaller) => shrink(&image, smaller),
        None => image.into_rgba8(),
    };
    let mut image = DynamicImage::ImageRgba8(image);
    image.apply_orientation(orientation);
    let image = image.into_rgba8();
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
/// in premultiplied colour so that transparent pixels lend no colour. An
/// image in another pixel format is read as RGBA a band of rows at a time.
fn shrink(image: &DynamicImage, size: [u32; 2]) -> RgbaImage {
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
    let line = width as usize * 4;
    let mut out = RgbaImage::new(size[0], size[1]);
    for (y, row) in rows.iter().enumerate() {
        let first = row.first().map_or(0, |&(pixel, _)| pixel);
        let last = row.last().map_or(0, |&(pixel, _)| pixel);
        let rows_of = |channels: usize, raw: &[u8]| -> Vec<u8> {
            let line = width as usize * channels;
            raw[first as usize * line..(last as usize + 1) * line].to_vec()
        };
        let count = last + 1 - first;
        let band: Cow<[u8]> = match image {
            DynamicImage::ImageRgba8(rgba) => {
                Cow::Borrowed(&rgba.as_raw()[first as usize * line..(last as usize + 1) * line])
            }
            DynamicImage::ImageRgb8(rgb) => Cow::Owned(
                DynamicImage::ImageRgb8(
                    image::RgbImage::from_raw(width, count, rows_of(3, rgb.as_raw()))
                        .expect("whole rows"),
                )
                .into_rgba8()
                .into_raw(),
            ),
            DynamicImage::ImageLuma8(grey) => Cow::Owned(
                DynamicImage::ImageLuma8(
                    image::GrayImage::from_raw(width, count, rows_of(1, grey.as_raw()))
                        .expect("whole rows"),
                )
                .into_rgba8()
                .into_raw(),
            ),
            other => Cow::Owned(
                other
                    .crop_imm(0, first, width, count)
                    .into_rgba8()
                    .into_raw(),
            ),
        };
        for (x, column) in columns.iter().enumerate() {
            let mut sum = [0.0f64; 4];
            for &(sy, wy) in row {
                for &(sx, wx) in column {
                    let at = (sy - first) as usize * line + sx as usize * 4;
                    let alpha = f64::from(band[at + 3]) / 255.0;
                    let weight = wx * wy;
                    for (channel, total) in sum.iter_mut().take(3).enumerate() {
                        *total += f64::from(band[at + channel]) * alpha * weight;
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
    use std::io::Cursor;

    fn picture(size: [u32; 2]) -> RgbaImage {
        RgbaImage::from_fn(size[0], size[1], |x, y| {
            image::Rgba([(x * 40) as u8, (y * 40) as u8, 90, 255])
        })
    }

    /// A `CF_DIBV5` of `size`, 32 bits a pixel, compressed as `compression`,
    /// with an alpha mask, then `pixels` (bottom row first, BGRA).
    fn dib(size: [i32; 2], compression: u32, pixels: &[u8]) -> Vec<u8> {
        let mut dib = vec![0u8; 124];
        let mut put = |at: usize, value: u32| dib[at..at + 4].copy_from_slice(&value.to_le_bytes());
        put(0, 124);
        put(4, size[0] as u32);
        put(8, size[1] as u32);
        put(12, 1 | 32 << 16);
        put(16, compression);
        put(52, 0xff00_0000);
        dib.extend_from_slice(pixels);
        dib
    }

    #[test]
    fn a_clipboard_image_too_large_or_broken_is_refused() {
        let wide = dib([EDGE as i32 + 1, 1], 0, &vec![0; (EDGE as usize + 1) * 4]);
        assert_eq!(decode_dib(wide, [64, 64]), Err(ImportError::TooLarge));
        // Fewer pixels than the header says.
        assert!(matches!(
            decode_dib(dib([2, 2], 0, &[0; 12]), [64, 64]),
            Err(ImportError::Damaged(_))
        ));
    }

    #[test]
    fn a_clipboard_image_keeps_the_transparency_chromium_puts_in_bi_rgb() {
        // Half-transparent red, then opaque green.
        let (_, asset) = decode_dib(dib([2, 1], 0, &[0, 0, 255, 128, 0, 255, 0, 255]), [64, 64])
            .expect("decodes");
        let read = pixels(&asset);
        assert_eq!(read.get_pixel(0, 0).0, [255, 0, 0, 128]);
        assert_eq!(read.get_pixel(1, 0).0, [0, 255, 0, 255]);
    }

    #[test]
    fn a_clipboard_image_with_its_masks_in_the_header_decodes() {
        // As arboard writes it: bit fields, the masks only in the header.
        let mut bitfields = dib([2, 1], 3, &[0, 0, 255, 128, 0, 255, 0, 255]);
        for (at, mask) in [(40, 0x00ff_0000u32), (44, 0x0000_ff00), (48, 0x0000_00ff)] {
            bitfields[at..at + 4].copy_from_slice(&mask.to_le_bytes());
        }
        let (_, asset) = decode_dib(bitfields, [64, 64]).expect("decodes");
        let read = pixels(&asset);
        assert_eq!(read.get_pixel(0, 0).0, [255, 0, 0, 128]);
        assert_eq!(read.get_pixel(1, 0).0, [0, 255, 0, 255]);
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
            let (id, asset) =
                decode(Cursor::new(bytes), [100, 100]).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(asset.size, [5, 3], "{name}");
            assert_eq!(pixels(&asset), original, "{name}");
            assert_eq!(id, AssetId(Sha256::digest(&asset.png).into()));
        }
        // The same pixels make the same asset whatever the file.
        let ids: Vec<_> = files
            .iter()
            .map(|(_, bytes)| decode(Cursor::new(bytes), [100, 100]).unwrap().0)
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
        assert_eq!(
            decode(Cursor::new(&jpeg), [100, 100]).unwrap().1.size,
            [5, 3]
        );
        let mut gif = Vec::new();
        codecs::gif::GifEncoder::new(&mut gif)
            .encode(raw, 5, 3, image::ExtendedColorType::Rgba8)
            .unwrap();
        assert_eq!(
            decode(Cursor::new(&gif), [100, 100]).unwrap().1.size,
            [5, 3]
        );
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
        let (_, asset) = decode(Cursor::new(&jpeg), [100, 100]).unwrap();
        assert_eq!(asset.size, [8, 16]);
        let upright = pixels(&asset);
        // Red was on the left; turned clockwise it is at the top.
        assert!(upright.get_pixel(4, 2)[0] > 200);
        assert!(upright.get_pixel(4, 13)[2] > 200);
    }

    #[test]
    fn a_larger_image_shrinks_to_fit_keeping_its_shape() {
        let (_, asset) = to_asset(
            picture([400, 100]).into(),
            Orientation::NoTransforms,
            [100, 100],
        )
        .unwrap();
        assert_eq!(asset.size, [100, 25]);
        let (_, asset) = to_asset(
            picture([40, 10]).into(),
            Orientation::NoTransforms,
            [100, 100],
        )
        .unwrap();
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
        let (_, asset) = to_asset(checker.into(), Orientation::NoTransforms, [4, 4]).unwrap();
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
        let (_, asset) = to_asset(half.into(), Orientation::NoTransforms, [1, 1]).unwrap();
        assert_eq!(pixels(&asset).get_pixel(0, 0).0, [200, 10, 10, 128]);
        // The asset limit applies even where the canvas would allow more.
        assert_eq!(fitted([8192, 8192], [8192, 8192]), Some([4096, 4096]));
    }

    #[test]
    fn shrinking_before_turning_gives_what_turning_first_did() {
        let image = RgbaImage::from_fn(37, 23, |x, y| {
            image::Rgba([
                (x * 7) as u8,
                (y * 11) as u8,
                ((x * y) % 256) as u8,
                (x * 9) as u8,
            ])
        });
        for orientation in [
            Orientation::NoTransforms,
            Orientation::Rotate90,
            Orientation::Rotate180,
            Orientation::Rotate270,
            Orientation::FlipHorizontal,
            Orientation::FlipVertical,
            Orientation::Rotate90FlipH,
            Orientation::Rotate270FlipH,
        ] {
            let mut turned = DynamicImage::ImageRgba8(image.clone());
            turned.apply_orientation(orientation);
            let (_, first) = to_asset(turned, Orientation::NoTransforms, [10, 10]).unwrap();
            let (_, later) = to_asset(image.clone().into(), orientation, [10, 10]).unwrap();
            assert_eq!(later.size, first.size, "{orientation:?}");
            for (x, y) in pixels(&first).pixels().zip(pixels(&later).pixels()) {
                for channel in 0..4 {
                    assert!(
                        x[channel].abs_diff(y[channel]) <= 1,
                        "{orientation:?}: {x:?} {y:?}"
                    );
                }
            }
            // Another pixel format, read a band at a time, shrinks the same.
            let opaque = DynamicImage::ImageRgba8(image.clone()).into_rgb8();
            let (_, rgb) = to_asset(opaque.clone().into(), orientation, [10, 10]).unwrap();
            let rgba = DynamicImage::ImageRgb8(opaque).into_rgba8();
            let (_, same) = to_asset(rgba.into(), orientation, [10, 10]).unwrap();
            assert_eq!(rgb.png, same.png, "{orientation:?}");
        }
    }

    #[test]
    fn what_is_not_an_image_or_too_large_is_refused() {
        assert_eq!(
            decode(Cursor::new(b"not an image"), [10, 10]).unwrap_err(),
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
            decode(Cursor::new(&truncated), [10, 10]),
            Err(ImportError::Damaged(_))
        ));
        // A header claiming a huge image is refused before decoding.
        let header = bmp_header(EDGE + 1, 1);
        assert_eq!(
            decode(Cursor::new(&header), [10, 10]).unwrap_err(),
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
