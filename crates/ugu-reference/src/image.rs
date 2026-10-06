// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

use crate::Error;

/// An 8-bit RGBA image with straight alpha, rows top to bottom, no padding.
/// Reference exports write PNGs this way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl Rgba {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            data: vec![0; width * height * 4],
        }
    }

    pub fn read_png(path: &Path) -> Result<Self, Error> {
        let mut decoder = png::Decoder::new(BufReader::new(File::open(path)?));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info()?;
        let size = reader
            .output_buffer_size()
            .ok_or_else(|| Error::new(format!("{}: image too large", path.display())))?;
        let mut buffer = vec![0; size];
        let info = reader.next_frame(&mut buffer)?;
        let (width, height) = (info.width as usize, info.height as usize);
        let pixels = &buffer[..info.buffer_size()];
        let data = match info.color_type {
            png::ColorType::Rgba => pixels.to_vec(),
            png::ColorType::Rgb => pixels
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            png::ColorType::GrayscaleAlpha => pixels
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[0], p[0], p[1]])
                .collect(),
            png::ColorType::Grayscale => pixels.iter().flat_map(|&v| [v, v, v, 255]).collect(),
            png::ColorType::Indexed => {
                return Err(Error::new(format!(
                    "{}: palette left unexpanded",
                    path.display()
                )));
            }
        };
        Ok(Self {
            width,
            height,
            data,
        })
    }

    pub fn write_png(&self, path: &Path) -> Result<(), Error> {
        let mut encoder = png::Encoder::new(
            BufWriter::new(File::create(path)?),
            u32::try_from(self.width).map_err(|_| Error::new("image too wide"))?,
            u32::try_from(self.height).map_err(|_| Error::new("image too tall"))?,
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&self.data)?;
        writer.finish()?;
        Ok(())
    }

    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let index = (y * self.width + x) * 4;
        [
            self.data[index],
            self.data[index + 1],
            self.data[index + 2],
            self.data[index + 3],
        ]
    }

    pub fn alpha(&self, x: usize, y: usize) -> u8 {
        self.data[(y * self.width + x) * 4 + 3]
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, pixel: [u8; 4]) {
        let index = (y * self.width + x) * 4;
        self.data[index..index + 4].copy_from_slice(&pixel);
    }

    pub fn same_size(&self, other: &Self) -> bool {
        self.width == other.width && self.height == other.height
    }
}
