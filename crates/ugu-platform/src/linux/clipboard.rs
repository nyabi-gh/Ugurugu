// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 NewfolderGames

//! Wayland (`wl_data_device`) and X11 (`CLIPBOARD` / `INCR`) image clipboard.

use std::sync::Arc;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use ugu_platform_types::clipboard::{Encoded, Image, ReadError};
use x11rb::connection::Connection as _;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, Property, WindowClass,
};
use x11rb::{COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT, NONE};

use crate::linux::window::{self, DisplayKind};

const X11_TIMEOUT: Duration = Duration::from_secs(3);

/// Wayland and X11 expose no global clipboard change counter; callers on Linux
/// compare content hashes via [`pixel_sha256`] and [`png_pixel_sha256`].
pub fn sequence() -> u32 {
    0
}

/// Writes `image` to the active Wayland or X11 clipboard.
pub fn write_image(image: Image) -> Result<u32, String> {
    match window::active_display_kind() {
        DisplayKind::Wayland => {
            let png = encode_png(&image)?;
            window::wayland_write_png(Arc::from(png))?;
            Ok(0)
        }
        DisplayKind::X11 => {
            let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
            clipboard
                .set_image(arboard::ImageData {
                    width: image.size[0] as usize,
                    height: image.size[1] as usize,
                    bytes: image.straight.into(),
                })
                .map_err(|error| error.to_string())?;
            Ok(0)
        }
        DisplayKind::Headless => Err("no display server for the clipboard".to_owned()),
    }
}

/// Reads an encoded PNG or TIFF image from the active Wayland or X11 clipboard
/// without buffering more than `limit` encoded bytes.
pub fn read_encoded(limit: usize) -> Result<Option<Encoded>, ReadError> {
    match window::active_display_kind() {
        DisplayKind::Wayland => window::wayland_read_encoded(limit),
        DisplayKind::X11 => x11_read_encoded(limit),
        DisplayKind::Headless => Err(ReadError::System(
            "no display server for the clipboard".to_owned(),
        )),
    }
}

/// SHA-256 of `image`'s dimensions and straight-alpha RGBA8 pixels.
pub fn pixel_sha256(image: &Image) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(image.size[0].to_le_bytes());
    hasher.update(image.size[1].to_le_bytes());
    hasher.update(&image.straight);
    hasher.finalize().into()
}

/// Streams `png_bytes` row-by-row to compute the same `(width, height, RGBA8)`
/// SHA-256 as [`pixel_sha256`], surviving zlib re-compression by X11 clipboard
/// managers or KDE Klipper while allocating only a single scanline.
pub fn png_pixel_sha256(png_bytes: &[u8], expected_size: [u32; 2]) -> Option<[u8; 32]> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let (color_type, bit_depth) = reader.output_color_type();
    if color_type != png::ColorType::Rgba || bit_depth != png::BitDepth::Eight {
        return None;
    }
    let info = reader.info();
    if [info.width, info.height] != expected_size || info.interlaced {
        return None;
    }
    let mut hasher = Sha256::new();
    hasher.update(info.width.to_le_bytes());
    hasher.update(info.height.to_le_bytes());
    while let Some(row) = reader.next_row().ok()? {
        hasher.update(row.data());
    }
    Some(hasher.finalize().into())
}

pub(crate) fn encode_png(image: &Image) -> Result<Vec<u8>, String> {
    let expected = (image.size[0] as usize)
        .checked_mul(image.size[1] as usize)
        .and_then(|px| px.checked_mul(4))
        .ok_or_else(|| "clipboard image dimensions overflow".to_owned())?;
    if image.straight.len() != expected {
        return Err("clipboard image buffer length does not match dimensions".to_owned());
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, image.size[0], image.size[1]);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
        writer
            .write_image_data(&image.straight)
            .map_err(|error| error.to_string())?;
    }
    Ok(out)
}

fn x11_read_encoded(limit: usize) -> Result<Option<Encoded>, ReadError> {
    let (conn, screen_num) =
        x11rb::connect(None).map_err(|error| ReadError::System(error.to_string()))?;
    let root = conn.setup().roots[screen_num].root;
    let win = conn
        .generate_id()
        .map_err(|error| ReadError::System(error.to_string()))?;
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        win,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_ONLY,
        COPY_FROM_PARENT,
        &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )
    .map_err(|error| ReadError::System(error.to_string()))?;

    let result = x11_read_on_window(&conn, win, limit);
    let _ = conn.destroy_window(win);
    let _ = conn.flush();
    result
}

fn intern<C: x11rb::connection::Connection>(conn: &C, name: &[u8]) -> Result<Atom, ReadError> {
    conn.intern_atom(false, name)
        .map_err(|error| ReadError::System(error.to_string()))?
        .reply()
        .map(|reply| reply.atom)
        .map_err(|error| ReadError::System(error.to_string()))
}

fn x11_read_on_window<C: x11rb::connection::Connection>(
    conn: &C,
    win: u32,
    limit: usize,
) -> Result<Option<Encoded>, ReadError> {
    let clipboard = intern(conn, b"CLIPBOARD")?;
    let targets = intern(conn, b"TARGETS")?;
    let incr = intern(conn, b"INCR")?;
    let prop = intern(conn, b"UGURUGU_CLIPBOARD")?;
    let png_atom = intern(conn, b"image/png")?;
    let tiff_atom = intern(conn, b"image/tiff")?;

    let owner = conn
        .get_selection_owner(clipboard)
        .map_err(|error| ReadError::System(error.to_string()))?
        .reply()
        .map_err(|error| ReadError::System(error.to_string()))?
        .owner;
    if owner == NONE {
        return Ok(None);
    }

    let _ = conn.delete_property(win, prop);
    conn.convert_selection(win, clipboard, targets, prop, x11rb::CURRENT_TIME)
        .map_err(|error| ReadError::System(error.to_string()))?;
    conn.flush()
        .map_err(|error| ReadError::System(error.to_string()))?;

    if !wait_selection_notify(conn, win, clipboard, targets)? {
        return Ok(None);
    }

    let target_reply = conn
        .get_property(true, win, prop, AtomEnum::ANY, 0, 1024)
        .map_err(|error| ReadError::System(error.to_string()))?
        .reply()
        .map_err(|error| ReadError::System(error.to_string()))?;
    let offered: Vec<Atom> = target_reply
        .value32()
        .map(|iter| iter.collect())
        .unwrap_or_default();

    let (chosen_atom, wrap): (Atom, fn(Vec<u8>) -> Encoded) = if offered.contains(&png_atom) {
        (png_atom, Encoded::Png)
    } else if offered.contains(&tiff_atom) {
        (tiff_atom, Encoded::Tiff)
    } else {
        return Ok(None);
    };

    let _ = conn.delete_property(win, prop);
    conn.convert_selection(win, clipboard, chosen_atom, prop, x11rb::CURRENT_TIME)
        .map_err(|error| ReadError::System(error.to_string()))?;
    conn.flush()
        .map_err(|error| ReadError::System(error.to_string()))?;

    if !wait_selection_notify(conn, win, clipboard, chosen_atom)? {
        return Ok(None);
    }

    let head = conn
        .get_property(false, win, prop, AtomEnum::ANY, 0, 0)
        .map_err(|error| ReadError::System(error.to_string()))?
        .reply()
        .map_err(|error| ReadError::System(error.to_string()))?;

    if head.type_ == incr {
        let _ = conn.delete_property(win, prop);
        conn.flush()
            .map_err(|error| ReadError::System(error.to_string()))?;
        let bytes = read_x11_incr(conn, win, prop, limit)?;
        return Ok(Some(wrap(bytes)));
    }

    let total = head.bytes_after as usize;
    if total > limit {
        let _ = conn.delete_property(win, prop);
        return Err(ReadError::TooLarge);
    }

    let mut data = Vec::with_capacity(total);
    let mut offset_words = 0u32;
    loop {
        let reply = conn
            .get_property(false, win, prop, AtomEnum::ANY, offset_words, 65_536)
            .map_err(|error| ReadError::System(error.to_string()))?
            .reply()
            .map_err(|error| ReadError::System(error.to_string()))?;
        if data.len().saturating_add(reply.value.len()) > limit {
            let _ = conn.delete_property(win, prop);
            return Err(ReadError::TooLarge);
        }
        data.extend_from_slice(&reply.value);
        if reply.bytes_after == 0 || reply.value.is_empty() {
            break;
        }
        offset_words = offset_words.saturating_add((reply.value.len() as u32).div_ceil(4));
    }
    let _ = conn.delete_property(win, prop);
    Ok(Some(wrap(data)))
}

fn wait_selection_notify<C: x11rb::connection::Connection>(
    conn: &C,
    win: u32,
    selection: Atom,
    target: Atom,
) -> Result<bool, ReadError> {
    let deadline = Instant::now() + X11_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(event) = conn
            .poll_for_event()
            .map_err(|error| ReadError::System(error.to_string()))?
            && let Event::SelectionNotify(ev) = event
            && ev.requestor == win
            && ev.selection == selection
            && ev.target == target
        {
            return Ok(ev.property != NONE);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Err(ReadError::System(
        "timed out waiting for X11 SelectionNotify".to_owned(),
    ))
}

fn read_x11_incr<C: x11rb::connection::Connection>(
    conn: &C,
    win: u32,
    prop: Atom,
    limit: usize,
) -> Result<Vec<u8>, ReadError> {
    let mut data = Vec::new();
    let mut deadline = Instant::now() + X11_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(event) = conn
            .poll_for_event()
            .map_err(|error| ReadError::System(error.to_string()))?
        {
            if let Event::PropertyNotify(ev) = event
                && ev.window == win
                && ev.atom == prop
                && ev.state == Property::NEW_VALUE
            {
                let chunk = conn
                    .get_property(true, win, prop, AtomEnum::ANY, 0, u32::MAX / 4)
                    .map_err(|error| ReadError::System(error.to_string()))?
                    .reply()
                    .map_err(|error| ReadError::System(error.to_string()))?;
                if chunk.value.is_empty() {
                    return Ok(data);
                }
                if data.len().saturating_add(chunk.value.len()) > limit {
                    return Err(ReadError::TooLarge);
                }
                data.extend_from_slice(&chunk.value);
                deadline = Instant::now() + X11_TIMEOUT;
            }
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    Err(ReadError::System(
        "timed out reading X11 INCR clipboard transfer".to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_encoding_preserves_pixel_sha256_across_compression_levels() {
        let image = Image {
            size: [2, 2],
            straight: vec![
                255, 0, 0, 255, 0, 128, 255, 128, 0, 0, 0, 0, 10, 200, 30, 64,
            ],
        };
        let expected = pixel_sha256(&image);
        let fast_png = encode_png(&image).unwrap();
        assert_eq!(png_pixel_sha256(&fast_png, [2, 2]), Some(expected));

        // Simulate a clipboard manager (such as Klipper) re-encoding the same
        // pixels with maximum zlib compression.
        let mut recompressed = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut recompressed, 2, 2);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_compression(png::Compression::High);
            let mut writer = enc.write_header().unwrap();
            writer.write_image_data(&image.straight).unwrap();
        }
        assert_ne!(fast_png, recompressed);
        assert_eq!(png_pixel_sha256(&recompressed, [2, 2]), Some(expected));

        // Wrong dimensions or altered pixels must not match.
        assert_eq!(png_pixel_sha256(&recompressed, [4, 1]), None);
        let mut altered = image.clone();
        altered.straight[0] ^= 1;
        let altered_png = encode_png(&altered).unwrap();
        assert_ne!(png_pixel_sha256(&altered_png, [2, 2]), Some(expected));
    }

    #[test]
    #[ignore]
    fn live_display_clipboard_round_trips() {
        let image = Image {
            size: [2, 2],
            straight: vec![255, 0, 0, 255, 0, 0, 255, 128, 0, 0, 0, 0, 10, 200, 30, 64],
        };
        write_image(image.clone()).unwrap();
        let Some(Encoded::Png(bytes)) = read_encoded(1 << 20).unwrap() else {
            panic!("expected PNG from live clipboard");
        };
        assert_eq!(png_pixel_sha256(&bytes, [2, 2]), Some(pixel_sha256(&image)));
        assert_eq!(read_encoded(4), Err(ReadError::TooLarge));
    }
}
