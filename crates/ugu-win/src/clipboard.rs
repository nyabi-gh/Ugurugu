// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Images on the system clipboard, as PNG and DIBV5 so that other apps
//! read them with their transparency. Both calls can take a while for a
//! large image, so callers run them off the UI and render threads.

use ugu_platform_types::clipboard::{Encoded, Image, ReadError};
use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;

/// Changes whenever anything is put on the clipboard, by any app.
pub fn sequence() -> u32 {
    // SAFETY: no arguments; it only reads a counter.
    unsafe { GetClipboardSequenceNumber() }
}

/// Puts `image` on the clipboard; returns the clipboard's sequence number
/// after it.
pub fn write_image(Image { size, straight }: Image) -> Result<u32, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
    clipboard
        .set_image(arboard::ImageData {
            width: size[0] as usize,
            height: size[1] as usize,
            bytes: straight.into(),
        })
        .map_err(|error| error.to_string())?;
    Ok(sequence())
}

/// The image on the clipboard, PNG first as arboard reads it, if it is no
/// more than `limit` bytes; `None` when it holds none.
pub fn read_encoded(limit: usize) -> Result<Option<Encoded>, ReadError> {
    let system = |error: clipboard_win::ErrorCode| ReadError::System(error.to_string());
    let _open = clipboard_win::Clipboard::new_attempts(10).map_err(system)?;
    let png = clipboard_win::register_format("PNG")
        .map(u32::from)
        .filter(|&format| clipboard_win::is_format_avail(format));
    let (format, wrap): (u32, fn(Vec<u8>) -> Encoded) = match png {
        Some(format) => (format, Encoded::Png),
        None if clipboard_win::is_format_avail(clipboard_win::formats::CF_DIBV5) => {
            (clipboard_win::formats::CF_DIBV5, Encoded::Dib)
        }
        None => return Ok(None),
    };
    if clipboard_win::raw::size(format).is_some_and(|size| size.get() > limit) {
        return Err(ReadError::TooLarge);
    }
    let mut bytes = Vec::new();
    clipboard_win::raw::get_vec(format, &mut bytes).map_err(system)?;
    Ok(Some(wrap(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Replaces what is on the clipboard, so it runs only when asked.
    #[test]
    #[ignore]
    fn an_image_round_trips_with_its_transparency() {
        let pixels = vec![255, 0, 0, 255, 0, 0, 255, 128, 0, 0, 0, 0, 10, 200, 30, 64];
        let before = sequence();
        let after = write_image(Image {
            size: [2, 2],
            straight: pixels.clone(),
        })
        .unwrap();
        assert_ne!(after, before);
        let Some(Encoded::Png(png)) = read_encoded(1 << 20).unwrap() else {
            panic!("no PNG on the clipboard");
        };
        assert!(png.starts_with(b"\x89PNG"));
    }
}
