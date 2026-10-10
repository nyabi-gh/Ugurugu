// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Animated GIF, written as 2.2.13's `GifWriter` writes it, byte for byte:
//! one palette for the whole animation by median cut over an RGB555
//! histogram of the opaque pixels, index 0 transparent when any pixel's
//! alpha is under 128, no dithering, endless looping, each frame replacing
//! the last (disposal 2), and its own LZW coder. Frames come in straight
//! alpha. The first pass keeps each pixel's 15-bit colour key, two bytes a
//! pixel, as 2.2.13 does; the second codes the frames on several threads.

use std::io::{self, Write};

const KEYS: usize = 1 << 15;
/// Marks a key whose pixel GIF transparency drops.
const TRANSPARENT: u16 = 0x8000;
const LARGEST_CODE: usize = 4096;

#[derive(Clone, Copy, Default)]
struct Bucket {
    count: u64,
    red: u64,
    green: u64,
    blue: u64,
}

/// A colour of the histogram: its key and the sums of what fell in it.
#[derive(Clone, Copy)]
struct Entry {
    key: u16,
    bucket: Bucket,
}

impl Entry {
    fn average(&self) -> [i32; 3] {
        let Bucket {
            count,
            red,
            green,
            blue,
        } = self.bucket;
        [red, green, blue].map(|sum| (sum / count) as i32)
    }
}

/// The first pass over the frames, in order.
pub struct Quantizer {
    size: [u32; 2],
    histogram: Vec<Bucket>,
    frames: Vec<Vec<u16>>,
    transparency: bool,
}

/// The palette and every frame's colour keys, ready to be written.
pub struct Quantized {
    size: [u32; 2],
    palette: Vec<[u8; 3]>,
    /// Palette index of each key.
    map: Vec<u8>,
    frames: Vec<Vec<u16>>,
    transparency: bool,
}

/// RGB555, so median cut works over at most 32768 colours.
fn key([red, green, blue]: [u8; 3]) -> u16 {
    (u16::from(red >> 3) << 10) | (u16::from(green >> 3) << 5) | u16::from(blue >> 3)
}

impl Quantizer {
    pub fn new(size: [u32; 2]) -> Self {
        Self {
            size,
            histogram: vec![Bucket::default(); KEYS],
            frames: Vec::new(),
            transparency: false,
        }
    }

    /// Takes the next frame: straight RGBA8 rows of the size given.
    pub fn add(&mut self, frame: &[u8]) {
        let pixels = frame.as_chunks::<4>().0;
        debug_assert_eq!(pixels.len(), (self.size[0] * self.size[1]) as usize);
        let mut keys = Vec::with_capacity(pixels.len());
        for &[red, green, blue, alpha] in pixels {
            let key = key([red, green, blue]);
            // All or nothing: partial alpha collapses at a threshold.
            if alpha < 128 {
                self.transparency = true;
                keys.push(key | TRANSPARENT);
                continue;
            }
            keys.push(key);
            let bucket = &mut self.histogram[usize::from(key)];
            bucket.count += 1;
            bucket.red += u64::from(red);
            bucket.green += u64::from(green);
            bucket.blue += u64::from(blue);
        }
        self.frames.push(keys);
    }

    pub fn finish(self) -> Quantized {
        let entries: Vec<Entry> = self
            .histogram
            .iter()
            .enumerate()
            .filter(|(_, bucket)| bucket.count > 0)
            .map(|(key, &bucket)| Entry {
                key: key as u16,
                bucket,
            })
            .collect();
        let palette = palette(&entries, self.transparency);
        let map = color_map(&entries, &palette, self.transparency);
        Quantized {
            size: self.size,
            palette,
            map,
            frames: self.frames,
            transparency: self.transparency,
        }
    }
}

/// A box of histogram entries and the range of their averages.
struct ColorBox {
    entries: Vec<usize>,
    population: u64,
    minimum: [i32; 3],
    maximum: [i32; 3],
}

impl ColorBox {
    fn new(entries: Vec<usize>, all: &[Entry]) -> Self {
        let first = all[entries[0]].average();
        let mut color_box = Self {
            entries,
            population: 0,
            minimum: first,
            maximum: first,
        };
        for &index in &color_box.entries {
            let average = all[index].average();
            color_box.population += all[index].bucket.count;
            for (axis, value) in average.into_iter().enumerate() {
                color_box.minimum[axis] = color_box.minimum[axis].min(value);
                color_box.maximum[axis] = color_box.maximum[axis].max(value);
            }
        }
        color_box
    }

    fn range(&self, axis: usize) -> i32 {
        self.maximum[axis] - self.minimum[axis]
    }

    fn split_axis(&self) -> usize {
        let [red, green, blue] = [0, 1, 2].map(|axis| self.range(axis));
        if red >= green && red >= blue {
            0
        } else if green >= blue {
            1
        } else {
            2
        }
    }
}

fn palette(entries: &[Entry], transparency: bool) -> Vec<[u8; 3]> {
    // Index 0 is kept for transparency when the animation needs it.
    let most = if transparency { 255 } else { 256 };
    let mut boxes = Vec::new();
    if !entries.is_empty() {
        boxes.push(ColorBox::new((0..entries.len()).collect(), entries));
    }
    while boxes.len() < most {
        // The box with the widest range times population, the first on ties.
        let mut selected = None;
        let mut best = -1.0f64;
        for (index, color_box) in boxes.iter().enumerate() {
            if color_box.entries.len() < 2 {
                continue;
            }
            let range = color_box
                .range(0)
                .max(color_box.range(1))
                .max(color_box.range(2));
            // Exact in f64 for any image GIF can hold, as 2.2.13's long double.
            let score = f64::from(range + 1) * color_box.population as f64;
            if score > best {
                selected = Some(index);
                best = score;
            }
        }
        let Some(selected) = selected else {
            break;
        };
        let mut source = std::mem::replace(
            &mut boxes[selected],
            ColorBox {
                entries: Vec::new(),
                population: 0,
                minimum: [0; 3],
                maximum: [0; 3],
            },
        );
        let axis = source.split_axis();
        source
            .entries
            .sort_by_key(|&index| (entries[index].average()[axis], entries[index].key));
        let mut cumulative = 0;
        let mut division = 1;
        for (position, &index) in source.entries[..source.entries.len() - 1]
            .iter()
            .enumerate()
        {
            cumulative += entries[index].bucket.count;
            division = position + 1;
            if cumulative >= source.population.div_ceil(2) {
                break;
            }
        }
        let right = source.entries.split_off(division);
        if source.entries.is_empty() || right.is_empty() {
            break;
        }
        boxes[selected] = ColorBox::new(source.entries, entries);
        boxes.push(ColorBox::new(right, entries));
    }
    let mut palette = Vec::with_capacity(boxes.len() + 1);
    if transparency {
        palette.push([0; 3]);
    }
    for color_box in &boxes {
        let mut sum = Bucket::default();
        for &index in &color_box.entries {
            let bucket = entries[index].bucket;
            sum.count += bucket.count;
            sum.red += bucket.red;
            sum.green += bucket.green;
            sum.blue += bucket.blue;
        }
        if sum.count > 0 {
            palette.push([sum.red, sum.green, sum.blue].map(|total| (total / sum.count) as u8));
        }
    }
    if palette.is_empty() {
        palette.push([0; 3]);
    }
    palette
}

/// The nearest opaque palette entry of each key in the histogram.
fn color_map(entries: &[Entry], palette: &[[u8; 3]], transparency: bool) -> Vec<u8> {
    let first = usize::from(transparency);
    let mut map = vec![0u8; KEYS];
    for entry in entries {
        let average = entry.average();
        let mut best = first;
        let mut best_distance = i32::MAX;
        for (index, color) in palette.iter().enumerate().skip(first) {
            let distance: i32 = (0..3)
                .map(|axis| {
                    let difference = average[axis] - i32::from(color[axis]);
                    difference * difference
                })
                .sum();
            if distance < best_distance {
                best_distance = distance;
                best = index;
            }
        }
        map[usize::from(entry.key)] = best as u8;
    }
    map
}

/// Packs codes of growing width, least significant bit first.
#[derive(Default)]
struct Bits {
    bytes: Vec<u8>,
    buffer: u32,
    count: u32,
}

impl Bits {
    fn write(&mut self, code: usize, width: u32) {
        self.buffer |= (code as u32) << self.count;
        self.count += width;
        while self.count >= 8 {
            self.bytes.push(self.buffer as u8);
            self.buffer >>= 8;
            self.count -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 {
            self.bytes.push(self.buffer as u8);
        }
        self.bytes
    }
}

/// GIF's LZW: clear and end codes just above the indices, codes widening as
/// the dictionary grows, and a clear when it reaches 4096 codes. Each code
/// of the dictionary is looked up by prefix and suffix in a table marked
/// with the round since the last clear, so clearing costs nothing.
struct Lzw {
    /// Per prefix and suffix: the round in the high bits, the code in the
    /// low twelve.
    table: Vec<u32>,
    round: u32,
}

impl Lzw {
    fn new() -> Self {
        Self {
            table: vec![0; LARGEST_CODE * 256],
            round: 0,
        }
    }

    fn clear(&mut self) {
        self.round += 1;
        if self.round == 1 << 20 {
            self.table.fill(0);
            self.round = 1;
        }
    }

    fn compress(&mut self, indices: &[u8], minimum_code_size: u32) -> Vec<u8> {
        let clear_code = 1usize << minimum_code_size;
        let end_code = clear_code + 1;
        let mut next_code = end_code + 1;
        let mut code_size = minimum_code_size + 1;
        let mut emitted_since_clear = 0;
        let mut bits = Bits::default();
        self.clear();
        bits.write(clear_code, code_size);
        let Some((&first, rest)) = indices.split_first() else {
            bits.write(end_code, code_size);
            return bits.finish();
        };
        let mut prefix = usize::from(first);
        for &suffix in rest {
            let slot = prefix * 256 + usize::from(suffix);
            let found = self.table[slot];
            if found >> 12 == self.round {
                prefix = (found & 0xfff) as usize;
                continue;
            }
            bits.write(prefix, code_size);
            emitted_since_clear += 1;
            if next_code < LARGEST_CODE {
                self.table[slot] = (self.round << 12) | next_code as u32;
                next_code += 1;
                if code_size < 12 && next_code > 1 << code_size {
                    code_size += 1;
                }
            } else {
                bits.write(clear_code, code_size);
                self.clear();
                next_code = end_code + 1;
                code_size = minimum_code_size + 1;
                emitted_since_clear = 0;
            }
            prefix = usize::from(suffix);
        }
        bits.write(prefix, code_size);
        if emitted_since_clear > 0 && code_size < 12 && next_code == 1 << code_size {
            code_size += 1;
        }
        bits.write(end_code, code_size);
        bits.finish()
    }
}

impl Quantized {
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Writes the GIF, coding up to `threads` frames at once and handing
    /// each frame's number to `progress` as it is written. Stops with an
    /// `Interrupted` error once `cancelled` says so.
    pub fn write(
        mut self,
        mut out: impl Write,
        delays_centiseconds: &[u16],
        threads: usize,
        cancelled: &(dyn Fn() -> bool + Sync),
        progress: &mut dyn FnMut(usize),
    ) -> io::Result<()> {
        assert_eq!(delays_centiseconds.len(), self.frames.len());
        let [width, height] = self.size.map(|edge| edge as u16);
        // A power of two long, stored as log2(entries) - 1.
        let mut table_size = 2;
        let mut table_bits = 1;
        while table_size < self.palette.len() {
            table_size <<= 1;
            table_bits += 1;
        }
        let mut header = Vec::with_capacity(13 + table_size * 3 + 19);
        header.extend_from_slice(b"GIF89a");
        header.extend_from_slice(&width.to_le_bytes());
        header.extend_from_slice(&height.to_le_bytes());
        header.extend_from_slice(&[0x80 | 0x70 | (table_bits - 1) as u8, 0, 0]);
        for index in 0..table_size {
            header.extend_from_slice(&self.palette.get(index).copied().unwrap_or([0; 3]));
        }
        header.extend_from_slice(&[0x21, 0xff, 0x0b]);
        header.extend_from_slice(b"NETSCAPE2.0");
        header.extend_from_slice(&[0x03, 0x01, 0, 0, 0]);
        out.write_all(&header)?;

        let minimum_code_size = table_bits.max(2);
        let threads = threads.max(1);
        let frames = std::mem::take(&mut self.frames);
        let mut coders: Vec<Lzw> = Vec::new();
        let mut written = 0;
        let mut pending = frames.into_iter();
        loop {
            let batch: Vec<Vec<u16>> = pending.by_ref().take(threads).collect();
            if batch.is_empty() {
                break;
            }
            if cancelled() {
                return Err(io::ErrorKind::Interrupted.into());
            }
            while coders.len() < batch.len() {
                coders.push(Lzw::new());
            }
            let this = &self;
            let coded: Vec<Vec<u8>> = std::thread::scope(|scope| {
                let handles: Vec<_> = batch
                    .into_iter()
                    .zip(coders.iter_mut())
                    .map(|(keys, coder)| {
                        scope.spawn(move || {
                            let indices = this.indices(&keys);
                            drop(keys);
                            coder.compress(&indices, minimum_code_size)
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|handle| handle.join().expect("a GIF coder thread panicked"))
                    .collect()
            });
            for data in coded {
                let delay = delays_centiseconds[written];
                let mut frame = Vec::with_capacity(data.len() + data.len() / 255 + 32);
                frame.extend_from_slice(&[0x21, 0xf9, 0x04, 0x08 | u8::from(self.transparency)]);
                frame.extend_from_slice(&delay.to_le_bytes());
                frame.extend_from_slice(&[0, 0, 0x2c, 0, 0, 0, 0]);
                frame.extend_from_slice(&width.to_le_bytes());
                frame.extend_from_slice(&height.to_le_bytes());
                frame.extend_from_slice(&[0, minimum_code_size as u8]);
                for block in data.chunks(255) {
                    frame.push(block.len() as u8);
                    frame.extend_from_slice(block);
                }
                frame.push(0);
                out.write_all(&frame)?;
                written += 1;
                progress(written);
            }
        }
        out.write_all(&[0x3b])?;
        out.flush()
    }

    fn indices(&self, keys: &[u16]) -> Vec<u8> {
        keys.iter()
            .map(|&key| {
                if self.transparency && key & TRANSPARENT != 0 {
                    0
                } else {
                    self.map[usize::from(key & !TRANSPARENT)]
                }
            })
            .collect()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The frames of `tools/AnimationGoldenProbe.cpp`, from the same formulas.
    pub(crate) fn probe_frames(name: &str) -> ([u32; 2], Vec<u16>, Vec<Vec<u8>>) {
        let (size, delays) = if name == "large" {
            ([300, 200], vec![7, 6])
        } else {
            ([64, 48], vec![4, 4, 5])
        };
        let frames = (0..delays.len() as i32)
            .map(|frame| {
                let mut pixels = Vec::new();
                for y in 0..size[1] as i32 {
                    for x in 0..size[0] as i32 {
                        let pixel = if name == "few" {
                            [
                                (x / 16) * 80 % 256,
                                (y / 16) * 90 % 256,
                                frame * 60 % 256,
                                255,
                            ]
                        } else {
                            [
                                (x * 7 + y * 3 + frame * 40 + ((x * y) % 13) * 5) % 256,
                                (x * 2 + y * 9 + frame * 17 + ((x ^ y) & 31)) % 256,
                                (x * y + frame * 91) % 256,
                                if name == "transparent" {
                                    ((x + y * 2 + frame * 5) % 5) * 60 + 15
                                } else {
                                    255
                                },
                            ]
                        };
                        pixels.extend(pixel.map(|channel| channel as u8));
                    }
                }
                pixels
            })
            .collect();
        (size, delays, frames)
    }

    fn written(name: &str, threads: usize) -> Vec<u8> {
        let (size, delays, frames) = probe_frames(name);
        let mut quantizer = Quantizer::new(size);
        for frame in &frames {
            quantizer.add(frame);
        }
        let mut bytes = Vec::new();
        let mut seen = Vec::new();
        quantizer
            .finish()
            .write(&mut bytes, &delays, threads, &|| false, &mut |frame| {
                seen.push(frame)
            })
            .unwrap();
        assert_eq!(seen, (1..=delays.len()).collect::<Vec<_>>());
        bytes
    }

    #[test]
    fn the_file_is_2_2_13_s_byte_for_byte() {
        for (name, golden) in [
            ("opaque", &include_bytes!("../tests/gif/opaque.gif")[..]),
            (
                "transparent",
                include_bytes!("../tests/gif/transparent.gif"),
            ),
            ("few", include_bytes!("../tests/gif/few.gif")),
            // Long enough for the LZW dictionary to fill and clear.
            ("large", include_bytes!("../tests/gif/large.gif")),
        ] {
            for threads in [1, 2, 8] {
                assert!(
                    written(name, threads) == golden,
                    "{name} on {threads} threads"
                );
            }
        }
    }

    #[test]
    fn the_file_decodes_to_the_frames_within_the_palette() {
        let (size, delays, frames) = probe_frames("transparent");
        let bytes = written("transparent", 4);
        let mut options = ::image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes))
            .map(::image::AnimationDecoder::into_frames)
            .unwrap();
        for (index, original) in frames.iter().enumerate() {
            let frame = options.next().unwrap().unwrap();
            let (numerator, denominator) = frame.delay().numer_denom_ms();
            assert_eq!(numerator / denominator, u32::from(delays[index]) * 10);
            let decoded = frame.into_buffer();
            assert_eq!(decoded.dimensions(), (size[0], size[1]));
            for (got, want) in decoded
                .as_raw()
                .as_chunks::<4>()
                .0
                .iter()
                .zip(original.as_chunks::<4>().0)
            {
                if want[3] < 128 {
                    assert_eq!(got[3], 0);
                } else {
                    assert_eq!(got[3], 255);
                }
            }
        }
        assert!(options.next().is_none());
    }

    #[test]
    fn cancelling_stops_between_frames() {
        let (size, delays, frames) = probe_frames("opaque");
        let mut quantizer = Quantizer::new(size);
        for frame in &frames {
            quantizer.add(frame);
        }
        let error = quantizer
            .finish()
            .write(Vec::new(), &delays, 1, &|| true, &mut |_| {})
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    }
}
