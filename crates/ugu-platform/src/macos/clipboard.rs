// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Images on the general pasteboard. Both calls can take a while for a
//! large image, so callers run them off the UI and render threads.

use objc2_app_kit::{NSPasteboard, NSPasteboardTypePNG, NSPasteboardTypeTIFF};
use objc2_foundation::NSArray;
use ugu_platform_types::clipboard::{Encoded, Image, ReadError};

/// The pasteboard's change count, which changes whenever anything is put
/// on it, by any app. Only compared for equality, so it is cut to 32 bits.
pub fn sequence() -> u32 {
    NSPasteboard::generalPasteboard().changeCount() as u32
}

/// Puts `image` on the pasteboard; returns the change count after it.
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

/// The image on the pasteboard, PNG first, else TIFF, which is what most
/// macOS apps put there; `None` when it holds neither. The pasteboard gives
/// no size before the data is read, so one over `limit` is read and then
/// refused, but never decoded.
pub fn read_encoded(limit: usize) -> Result<Option<Encoded>, ReadError> {
    let pasteboard = NSPasteboard::generalPasteboard();
    // SAFETY: the type names are AppKit's constants, set before any call.
    let (png, tiff) = unsafe { (NSPasteboardTypePNG, NSPasteboardTypeTIFF) };
    let Some(available) = pasteboard.availableTypeFromArray(&NSArray::from_slice(&[png, tiff]))
    else {
        return Ok(None);
    };
    let wrap: fn(Vec<u8>) -> Encoded = if *available == *png {
        Encoded::Png
    } else {
        Encoded::Tiff
    };
    let data = pasteboard
        .dataForType(&available)
        .ok_or_else(|| ReadError::System("the pasteboard gave no data".to_owned()))?;
    if data.len() > limit {
        return Err(ReadError::TooLarge);
    }
    Ok(Some(wrap(data.to_vec())))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Replaces what is on the pasteboard, so it runs only when asked.
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
        assert!(matches!(
            read_encoded(1 << 20).unwrap(),
            Some(Encoded::Png(_) | Encoded::Tiff(_))
        ));
        assert_eq!(read_encoded(4), Err(ReadError::TooLarge));
    }
}
