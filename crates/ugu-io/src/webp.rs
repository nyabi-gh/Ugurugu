// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Animated WebP through libwebp 1.6.0, coded as 2.2.13's `WebPWriter`
//! codes it: lossless at preset 6 with alpha quality 100, looping endlessly
//! on a transparent background, each frame as small as the encoder can make
//! it (`minimize_size`), timed in milliseconds. Frames come in straight
//! alpha. libwebp is used here and nowhere else.
//!
//! libwebp codes an animation's frames one after another on one thread, so
//! a long animation is cut into runs coded at once, each by an encoder of
//! its own, then joined. A run after the first starts with its first frame
//! whole, as the encoder would code it with nothing before it but covering
//! the canvas, so that nothing of the run before shows through. An animation
//! coded as one run is 2.2.13's file byte for byte.

use std::ffi::{CStr, c_int, c_void};
use std::io::{self, Write};
use std::ops::Range;
use std::ptr::NonNull;

use libwebp_sys as webp;

/// Frames to a run at the least, so that a short animation is one run.
const SHORTEST_RUN: usize = 4;

/// How `frames` are cut into runs to code on up to `threads` threads.
pub fn runs(frames: usize, threads: usize) -> Vec<Range<usize>> {
    let count = (frames / SHORTEST_RUN).clamp(1, threads.max(1));
    (0..count)
        .map(|run| run * frames / count..(run + 1) * frames / count)
        .collect()
}

fn failed(reason: impl Into<String>) -> io::Error {
    io::Error::other(reason.into())
}

fn cancelled_error() -> io::Error {
    io::ErrorKind::Interrupted.into()
}

type Cancelled<'a> = &'a (dyn Fn() -> bool + Sync);

/// Asks libwebp to stop once the export is cancelled, which it checks
/// several times while coding a frame.
unsafe extern "C" fn progress(_percent: c_int, picture: *const webp::WebPPicture) -> c_int {
    // SAFETY: `user_data` is the run's boxed `cancelled`, which outlives
    // every picture the run codes.
    let cancelled = unsafe { &*((*picture).user_data as *const Cancelled) };
    c_int::from(!cancelled())
}

/// A frame as libwebp takes it, freed when dropped.
struct Picture(webp::WebPPicture);

impl Picture {
    fn new(frame: &[u8], size: [u32; 2], cancelled: &Cancelled) -> io::Result<Self> {
        let [width, height] = size.map(|edge| edge as c_int);
        assert_eq!(frame.len(), width as usize * height as usize * 4);
        // SAFETY: set up by libwebp, which takes its own copy of `frame`.
        unsafe {
            let mut picture = Self(std::mem::zeroed());
            if webp::WebPPictureInitInternal(
                &mut picture.0,
                webp::WEBP_ENCODER_ABI_VERSION as c_int,
            ) == 0
            {
                return Err(failed("the WebP encoder could not be set up"));
            }
            picture.0.use_argb = 1;
            picture.0.width = width;
            picture.0.height = height;
            picture.0.progress_hook = Some(progress);
            picture.0.user_data = cancelled as *const Cancelled as *mut c_void;
            if webp::WebPPictureImportRGBA(&mut picture.0, frame.as_ptr(), width * 4) == 0 {
                return Err(failed("an animation frame could not be coded"));
            }
            Ok(picture)
        }
    }
}

impl Drop for Picture {
    fn drop(&mut self) {
        // SAFETY: set up by `WebPPictureInitInternal`.
        unsafe { webp::WebPPictureFree(&mut self.0) };
    }
}

/// Bytes libwebp made, freed when dropped.
struct Data(webp::WebPData);

impl Data {
    fn empty() -> Self {
        Self(webp::WebPData {
            bytes: std::ptr::null(),
            size: 0,
        })
    }

    fn bytes(&self) -> &[u8] {
        if self.0.bytes.is_null() {
            &[]
        } else {
            // SAFETY: libwebp's own allocation of `size` bytes.
            unsafe { std::slice::from_raw_parts(self.0.bytes, self.0.size) }
        }
    }
}

impl Drop for Data {
    fn drop(&mut self) {
        // SAFETY: allocated by libwebp, or null.
        unsafe { webp::WebPFree(self.0.bytes as *mut c_void) };
    }
}

/// A run of an animation's frames, coded on its own.
pub struct Run<'a> {
    encoder: NonNull<webp::WebPAnimEncoder>,
    config: webp::WebPConfig,
    size: [u32; 2],
    /// Whether the run starts the animation.
    starts: bool,
    /// The first frame whole, for a run that does not start the animation.
    whole: Option<Vec<u8>>,
    /// Where the next frame starts, in milliseconds.
    timestamp: c_int,
    /// Boxed so that the progress hook can find it at a fixed address.
    cancelled: Box<Cancelled<'a>>,
}

// A run is only ever used by one thread at a time.
unsafe impl Send for Run<'_> {}

/// A run coded, for `join`.
pub struct Coded {
    animation: Vec<u8>,
    whole: Option<Vec<u8>>,
    /// Milliseconds.
    duration: c_int,
}

impl<'a> Run<'a> {
    /// A run of frames of `size`, the first of the animation if `starts`,
    /// which stops with an `Interrupted` error once `cancelled` says so.
    pub fn new(size: [u32; 2], starts: bool, cancelled: Cancelled<'a>) -> io::Result<Self> {
        let [width, height] = size.map(|edge| c_int::try_from(edge).unwrap_or(c_int::MAX));
        if width <= 0 || height <= 0 || width > 16383 || height > 16383 {
            return Err(failed(format!(
                "WebP cannot hold {width} × {height} frames"
            )));
        }
        // SAFETY: plain C structs set up by libwebp's own initialisers.
        unsafe {
            let mut options = std::mem::zeroed::<webp::WebPAnimEncoderOptions>();
            if webp::WebPAnimEncoderOptionsInitInternal(
                &mut options,
                webp::WEBP_MUX_ABI_VERSION as c_int,
            ) == 0
            {
                return Err(failed("the WebP encoder could not be set up"));
            }
            options.anim_params.loop_count = 0;
            options.anim_params.bgcolor = 0;
            options.minimize_size = 1;
            let encoder = NonNull::new(webp::WebPAnimEncoderNewInternal(
                width,
                height,
                &options,
                webp::WEBP_MUX_ABI_VERSION as c_int,
            ))
            .ok_or_else(|| failed("the WebP encoder could not be set up"))?;
            let mut config = std::mem::zeroed::<webp::WebPConfig>();
            let ready = webp::WebPConfigInitInternal(
                &mut config,
                webp::WebPPreset::WEBP_PRESET_DEFAULT,
                75.0,
                webp::WEBP_ENCODER_ABI_VERSION as c_int,
            ) != 0
                && webp::WebPConfigLosslessPreset(&mut config, 6) != 0
                && webp::WebPValidateConfig(&config) != 0;
            if !ready {
                webp::WebPAnimEncoderDelete(encoder.as_ptr());
                return Err(failed("the WebP encoder settings are invalid"));
            }
            config.alpha_quality = 100;
            Ok(Self {
                encoder,
                config,
                size,
                starts,
                whole: None,
                timestamp: 0,
                cancelled: Box::new(cancelled),
            })
        }
    }

    fn cancelled(&self) -> bool {
        (self.cancelled)()
    }

    /// Codes the next frame, straight RGBA of the run's size, shown for
    /// `duration` milliseconds.
    pub fn add(&mut self, frame: &[u8], duration: u32) -> io::Result<()> {
        if self.cancelled() {
            return Err(cancelled_error());
        }
        let next = c_int::try_from(duration)
            .ok()
            .filter(|&duration| duration > 0)
            .and_then(|duration| self.timestamp.checked_add(duration))
            .ok_or_else(|| failed("the animation timings are invalid"))?;
        let mut picture = Picture::new(frame, self.size, &self.cancelled)?;
        if !self.starts && self.whole.is_none() {
            self.whole = Some(self.whole(&mut picture)?);
        }
        // SAFETY: libwebp copies what it keeps of the picture.
        let added = unsafe {
            webp::WebPAnimEncoderAdd(
                self.encoder.as_ptr(),
                &mut picture.0,
                self.timestamp,
                &self.config,
            )
        } != 0;
        if !added {
            return Err(self.error());
        }
        self.timestamp = next;
        Ok(())
    }

    /// `picture` coded alone, covering the canvas.
    fn whole(&self, picture: &mut Picture) -> io::Result<Vec<u8>> {
        // SAFETY: the writer collects what `WebPEncode` writes and is
        // cleared before returning.
        unsafe {
            let mut writer = std::mem::zeroed::<webp::WebPMemoryWriter>();
            webp::WebPMemoryWriterInit(&mut writer);
            picture.0.writer = Some(webp::WebPMemoryWrite);
            picture.0.custom_ptr = &mut writer as *mut webp::WebPMemoryWriter as *mut c_void;
            let coded = webp::WebPEncode(&self.config, &mut picture.0) != 0;
            picture.0.writer = None;
            picture.0.custom_ptr = std::ptr::null_mut();
            let bytes = coded.then(|| std::slice::from_raw_parts(writer.mem, writer.size).to_vec());
            webp::WebPMemoryWriterClear(&mut writer);
            bytes.ok_or_else(|| {
                if self.cancelled() {
                    cancelled_error()
                } else {
                    failed("an animation frame could not be coded")
                }
            })
        }
    }

    /// Ends the run.
    pub fn finish(self) -> io::Result<Coded> {
        if self.cancelled() {
            return Err(cancelled_error());
        }
        let mut data = Data::empty();
        // SAFETY: `data` is filled by libwebp and freed by its own call.
        let assembled = unsafe {
            webp::WebPAnimEncoderAdd(
                self.encoder.as_ptr(),
                std::ptr::null_mut(),
                self.timestamp,
                std::ptr::null(),
            ) != 0
                && webp::WebPAnimEncoderAssemble(self.encoder.as_ptr(), &mut data.0) != 0
        };
        if !assembled || data.bytes().is_empty() {
            return Err(self.error());
        }
        Ok(Coded {
            animation: data.bytes().to_vec(),
            whole: self.whole.clone(),
            duration: self.timestamp,
        })
    }

    fn error(&self) -> io::Error {
        if self.cancelled() {
            return cancelled_error();
        }
        // SAFETY: libwebp keeps the message as long as the encoder.
        let message = unsafe { webp::WebPAnimEncoderGetError(self.encoder.as_ptr()) };
        let message = if message.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(message) }
                .to_string_lossy()
                .into_owned()
        };
        if message.is_empty() {
            failed("the WebP encoder failed")
        } else {
            failed(message)
        }
    }
}

impl Drop for Run<'_> {
    fn drop(&mut self) {
        // SAFETY: made by `WebPAnimEncoderNewInternal` and deleted once.
        unsafe { webp::WebPAnimEncoderDelete(self.encoder.as_ptr()) };
    }
}

/// A mux, deleted when dropped.
struct Mux(NonNull<webp::WebPMux>);

impl Mux {
    fn checked(error: webp::WebPMuxError) -> io::Result<()> {
        if error == webp::WebPMuxError::WEBP_MUX_OK {
            Ok(())
        } else {
            Err(failed(format!(
                "the WebP frames could not be joined ({error:?})"
            )))
        }
    }
}

impl Drop for Mux {
    fn drop(&mut self) {
        // SAFETY: made by libwebp and deleted once.
        unsafe { webp::WebPMuxDelete(self.0.as_ptr()) };
    }
}

/// Writes the animation of `runs`, in order, of frames of `size`.
pub fn join(runs: Vec<Coded>, size: [u32; 2], mut out: impl Write) -> io::Result<()> {
    if let [run] = &runs[..] {
        out.write_all(&run.animation)?;
        return out.flush();
    }
    let new_mux = |data: Option<&webp::WebPData>| {
        // SAFETY: a read mux borrows `data`, which outlives it.
        let mux = unsafe {
            match data {
                Some(data) => {
                    webp::WebPMuxCreateInternal(data, 0, webp::WEBP_MUX_ABI_VERSION as c_int)
                }
                None => webp::WebPNewInternal(webp::WEBP_MUX_ABI_VERSION as c_int),
            }
        };
        NonNull::new(mux)
            .map(Mux)
            .ok_or_else(|| failed("the WebP frames could not be joined"))
    };
    let joined = new_mux(None)?;
    for run in &runs {
        let data = webp::WebPData {
            bytes: run.animation.as_ptr(),
            size: run.animation.len(),
        };
        let read = new_mux(Some(&data))?;
        // SAFETY: frames read from `read` are pushed as copies before it
        // is dropped.
        unsafe {
            let mut flags = 0;
            Mux::checked(webp::WebPMuxGetFeatures(read.0.as_ptr(), &mut flags))?;
            let mut frames = 1;
            if flags & webp::WebPFeatureFlags::ANIMATION_FLAG as u32 != 0 {
                Mux::checked(webp::WebPMuxNumChunks(
                    read.0.as_ptr(),
                    webp::WebPChunkId::WEBP_CHUNK_ANMF,
                    &mut frames,
                ))?;
            }
            for nth in 1..=frames as u32 {
                let mut frame = std::mem::zeroed::<webp::WebPMuxFrameInfo>();
                Mux::checked(webp::WebPMuxGetFrame(read.0.as_ptr(), nth, &mut frame))?;
                // libwebp's copy, freed after the frame is pushed.
                let mut read_bitstream = frame.bitstream;
                if frames == 1 {
                    // libwebp writes a run of frames that do not change as
                    // a still image, without a duration.
                    frame.duration = run.duration;
                    frame.dispose_method = webp::WebPMuxAnimDispose::WEBP_MUX_DISPOSE_NONE;
                }
                if let (1, Some(whole)) = (nth, &run.whole) {
                    frame.bitstream = webp::WebPData {
                        bytes: whole.as_ptr(),
                        size: whole.len(),
                    };
                    frame.x_offset = 0;
                    frame.y_offset = 0;
                    frame.blend_method = webp::WebPMuxAnimBlend::WEBP_MUX_NO_BLEND;
                }
                frame.id = webp::WebPChunkId::WEBP_CHUNK_ANMF;
                let pushed = webp::WebPMuxPushFrame(joined.0.as_ptr(), &frame, 1);
                webp::WebPDataClear(&mut read_bitstream);
                Mux::checked(pushed)?;
            }
        }
    }
    let [width, height] = size.map(|edge| edge as c_int);
    let mut data = Data::empty();
    // SAFETY: `data` is filled by libwebp and freed by its own call.
    unsafe {
        Mux::checked(webp::WebPMuxSetCanvasSize(joined.0.as_ptr(), width, height))?;
        Mux::checked(webp::WebPMuxSetAnimationParams(
            joined.0.as_ptr(),
            &webp::WebPMuxAnimParams {
                bgcolor: 0,
                loop_count: 0,
            },
        ))?;
        Mux::checked(webp::WebPMuxAssemble(joined.0.as_ptr(), &mut data.0))?;
    }
    out.write_all(data.bytes())?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gif::tests::probe_frames;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// `frames` coded in up to `threads` runs, one after another.
    fn coded(size: [u32; 2], frames: &[Vec<u8>], delays: &[u32], threads: usize) -> Vec<u8> {
        let runs = runs(frames.len(), threads)
            .into_iter()
            .map(|range| {
                let mut run = Run::new(size, range.start == 0, &|| false).unwrap();
                for index in range {
                    run.add(&frames[index], delays[index]).unwrap();
                }
                run.finish().unwrap()
            })
            .collect();
        let mut bytes = Vec::new();
        join(runs, size, &mut bytes).unwrap();
        bytes
    }

    fn encoded(name: &str) -> (Vec<u8>, [u32; 2], Vec<u32>, Vec<Vec<u8>>) {
        let (size, delays, frames) = probe_frames(name);
        let delays: Vec<u32> = delays.iter().map(|&delay| u32::from(delay) * 10).collect();
        let bytes = coded(size, &frames, &delays, 1);
        (bytes, size, delays, frames)
    }

    /// The same frames as `tools/AnimationGoldenProbe.cpp` gives 2.2.13.
    #[test]
    fn the_file_is_2_2_13_s_byte_for_byte() {
        for (name, golden) in [
            ("opaque", &include_bytes!("../tests/webp/opaque.webp")[..]),
            (
                "transparent",
                include_bytes!("../tests/webp/transparent.webp"),
            ),
            ("few", include_bytes!("../tests/webp/few.webp")),
            ("large", include_bytes!("../tests/webp/large.webp")),
        ] {
            assert!(encoded(name).0 == golden, "{name}");
        }
    }

    /// libwebp's own decoder: each frame shown and the time it ends.
    fn decoded(bytes: &[u8], size: [u32; 2]) -> Vec<(Vec<u8>, i32)> {
        let length = size[0] as usize * size[1] as usize * 4;
        let mut frames = Vec::new();
        // SAFETY: the decoder reads `bytes`, which outlive it, and lends
        // each frame until the next call.
        unsafe {
            let mut options = std::mem::zeroed();
            assert_ne!(webp::WebPAnimDecoderOptionsInit(&mut options), 0);
            options.color_mode = webp::WEBP_CSP_MODE::MODE_RGBA;
            let data = webp::WebPData {
                bytes: bytes.as_ptr(),
                size: bytes.len(),
            };
            let decoder = webp::WebPAnimDecoderNew(&data, &options);
            assert!(!decoder.is_null());
            let mut pixels = std::ptr::null_mut();
            let mut end = 0;
            while webp::WebPAnimDecoderGetNext(decoder, &mut pixels, &mut end) != 0 {
                frames.push((std::slice::from_raw_parts(pixels, length).to_vec(), end));
            }
            webp::WebPAnimDecoderDelete(decoder);
        }
        frames
    }

    /// Lossless: every frame decodes back to what went in, where it is not
    /// fully transparent, at its time.
    #[test]
    fn decodes_to_the_frames_and_delays() {
        for name in ["opaque", "transparent", "few", "large"] {
            let (bytes, size, delays, frames) = encoded(name);
            let decoded = decoded(&bytes, size);
            assert_eq!(decoded.len(), frames.len(), "{name}");
            let mut end = 0;
            for ((got, expected), &delay) in decoded.iter().zip(&frames).zip(&delays) {
                end += delay as i32;
                assert_eq!(got.1, end, "{name}");
                for (got, wanted) in got
                    .0
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(expected.as_chunks::<4>().0)
                {
                    if wanted[3] == 0 {
                        assert_eq!(got[3], 0, "{name}");
                    } else {
                        assert_eq!(got, wanted, "{name}");
                    }
                }
            }
        }
    }

    /// Another decoder reads as many frames with the same delays. Its
    /// pixels are not compared: image-webp 0.2.4 blends opaque pixels too
    /// and comes out one lower.
    #[test]
    fn image_reads_the_frames_and_delays() {
        use image::AnimationDecoder;
        for name in ["opaque", "transparent"] {
            let (bytes, size, delays, _) = encoded(name);
            let decoder =
                image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(&bytes)).unwrap();
            let frames: Vec<image::Frame> = decoder.into_frames().collect_frames().unwrap();
            let read: Vec<u32> = frames
                .iter()
                .map(|frame| {
                    assert_eq!(frame.buffer().dimensions(), (size[0], size[1]));
                    let (numerator, denominator) = frame.delay().numer_denom_ms();
                    numerator / denominator
                })
                .collect();
            assert_eq!(read, delays, "{name}");
        }
    }

    /// Frames that move, some still and some partly transparent, cut into
    /// runs: each frame still decodes to what went in, at its time.
    #[test]
    fn runs_join_into_the_same_frames() {
        let (size, _, opaque) = probe_frames("large");
        let (_, _, moving) = probe_frames("transparent");
        let small = [64usize, 48];
        // Partly transparent frames, then still ones, then opaque ones.
        let mut frames: Vec<Vec<u8>> = Vec::new();
        for index in 0..30 {
            let mut frame = vec![0u8; size[0] as usize * size[1] as usize * 4];
            let source = match index {
                0..10 => &moving[index % moving.len()],
                10..20 => &moving[0],
                _ => &opaque[index % opaque.len()],
            };
            if index < 20 {
                // A small frame at an offset that moves, on transparency.
                let at = index.min(10);
                for y in 0..small[1] {
                    let row = (y + at) * size[0] as usize + at * 3;
                    frame[row * 4..(row + small[0]) * 4]
                        .copy_from_slice(&source[y * small[0] * 4..(y + 1) * small[0] * 4]);
                }
            } else {
                frame.copy_from_slice(source);
            }
            frames.push(frame);
        }
        let delays: Vec<u32> = (0..30).map(|index| 40 + index % 3).collect();
        assert_eq!(runs(30, 4).len(), 4);
        let bytes = coded(size, &frames, &delays, 4);
        let decoded = decoded(&bytes, size);
        // Still frames within a run are one longer frame.
        let mut expected = Vec::new();
        let mut end = 0;
        for (index, frame) in frames.iter().enumerate() {
            end += delays[index] as i32;
            let merged = index > 0
                && frames[index - 1] == *frame
                && !runs(30, 4).iter().any(|run| run.start == index);
            if merged {
                expected.pop();
            }
            expected.push((frame, end));
        }
        assert_eq!(decoded.len(), expected.len());
        for ((got, got_end), (wanted, wanted_end)) in decoded.iter().zip(expected) {
            assert_eq!(*got_end, wanted_end);
            for (got, wanted) in got.as_chunks::<4>().0.iter().zip(wanted.as_chunks::<4>().0) {
                if wanted[3] == 0 {
                    assert_eq!(got[3], 0);
                } else {
                    assert_eq!(got, wanted);
                }
            }
        }
    }

    #[test]
    fn short_animations_are_one_run() {
        assert_eq!(runs(3, 8), vec![0..3]);
        assert_eq!(runs(7, 8), vec![0..7]);
        assert_eq!(runs(15, 8), [0..5, 5..10, 10..15]);
        assert_eq!(runs(30, 8).len(), 7);
        assert_eq!(runs(100, 8).len(), 8);
        assert_eq!(runs(30, 1), vec![0..30]);
    }

    #[test]
    fn loops_endlessly() {
        let (bytes, ..) = encoded("opaque");
        let anim = bytes.windows(4).position(|chunk| chunk == b"ANIM").unwrap();
        // Background colour (4 bytes), then the loop count.
        assert_eq!(&bytes[anim + 12..anim + 14], &[0, 0]);
    }

    #[test]
    fn stops_inside_a_frame_once_cancelled() {
        let (size, _, frames) = probe_frames("large");
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let cancel = AtomicBool::new(false);
        // Cancelled by the encoder's own progress calls, after the check
        // before the frame starts.
        let cancelled = || {
            let called = calls.fetch_add(1, Ordering::Relaxed);
            called > 0 && cancel.load(Ordering::Relaxed)
        };
        let mut encoder = Run::new(size, true, &cancelled).unwrap();
        encoder.add(&frames[0], 70).unwrap();
        cancel.store(true, Ordering::Relaxed);
        calls.store(0, Ordering::Relaxed);
        let error = encoder.add(&frames[1], 60).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(calls.load(Ordering::Relaxed) > 1);
    }

    #[test]
    fn refuses_bad_timings_and_sizes() {
        assert!(Run::new([0, 10], true, &|| false).is_err());
        assert!(Run::new([16384, 10], true, &|| false).is_err());
        let mut encoder = Run::new([2, 2], true, &|| false).unwrap();
        assert!(encoder.add(&[0; 16], 0).is_err());
    }
}
