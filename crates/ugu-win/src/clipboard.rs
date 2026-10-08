// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Images on the system clipboard, as PNG and DIBV5 so that other apps
//! read them with their transparency. Both calls can take a while for a
//! large image, so callers run them off the UI and render threads.

use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;

/// Straight RGBA pixels, row by row.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub size: [u32; 2],
    pub straight: Vec<u8>,
}

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

/// The image on the clipboard; `None` when it holds none.
pub fn read_image() -> Result<Option<Image>, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
    match clipboard.get_image() {
        Ok(image) => Ok(Some(Image {
            size: [image.width as u32, image.height as u32],
            straight: image.bytes.into_owned(),
        })),
        Err(arboard::Error::ContentNotAvailable) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
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
        let read = read_image().unwrap().unwrap();
        assert_eq!(read.size, [2, 2]);
        for (got, want) in read.straight.chunks(4).zip(pixels.chunks(4)) {
            assert_eq!(got[3], want[3]);
            if want[3] == 255 {
                assert_eq!(got, want);
            }
        }
    }
}
