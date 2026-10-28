// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

/// Straight RGBA pixels, row by row.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub size: [u32; 2],
    pub straight: Vec<u8>,
}

/// An image on the clipboard as the app that put it there encoded it, to be
/// decoded within the limits files are.
#[derive(Debug, PartialEq)]
pub enum Encoded {
    Png(Vec<u8>),
    /// `CF_DIBV5`: a `BITMAPV5HEADER` and the pixels, without a file header.
    Dib(Vec<u8>),
}

#[derive(Debug, PartialEq)]
pub enum ReadError {
    /// More than the caller would take, left on the clipboard.
    TooLarge,
    System(String),
}
