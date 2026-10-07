// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! A layer surface on a grid of square tiles from the canvas origin. Only
//! the rectangle of tiles that strokes reach has pixels, and tiles inside it
//! that nothing reaches are marked empty and skipped.

use vello_cpu::Pixmap;

/// Premultiplied RGBA8.
pub type Pixel = [u8; 4];

#[derive(Clone, Debug)]
pub struct TiledSurface {
    size: [u32; 2],
    edge: u32,
    grid: [u32; 2],
    /// Tile columns and rows the pixels cover: left, top, right, bottom.
    span: [u32; 4],
    /// Covers `span`; `None` when nothing is drawn.
    pixmap: Option<Pixmap>,
    /// Row-major over the whole grid.
    reached: Vec<bool>,
}

impl TiledSurface {
    /// Transparent everywhere. `edge` must be a multiple of 4, so tiles line
    /// up with Vello's own tiles.
    pub fn new(size: [u32; 2], edge: u32) -> Self {
        assert!(edge >= 4 && edge.is_multiple_of(4));
        let grid = size.map(|length| length.div_ceil(edge));
        Self {
            size,
            edge,
            grid,
            span: [0; 4],
            pixmap: None,
            reached: vec![false; (grid[0] * grid[1]) as usize],
        }
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn edge(&self) -> u32 {
        self.edge
    }

    /// Columns and rows of tiles.
    pub fn grid(&self) -> [u32; 2] {
        self.grid
    }

    /// Bytes of pixels held.
    pub fn bytes(&self) -> usize {
        self.pixmap.as_ref().map_or(0, |pixmap| {
            usize::from(pixmap.width()) * usize::from(pixmap.height()) * 4
        })
    }

    /// Tiles with something drawn in them.
    pub fn tile_count(&self) -> usize {
        self.reached.iter().filter(|reached| **reached).count()
    }

    /// The pixels of the reached tiles in `span` (tile left, top, right,
    /// bottom): where they start in the document and how large they are.
    pub fn area(&self, span: [u32; 4]) -> ([u32; 2], [u16; 2]) {
        let origin = [span[0] * self.edge, span[1] * self.edge];
        let extent = [
            ((span[2] * self.edge).min(self.size[0]) - origin[0]) as u16,
            ((span[3] * self.edge).min(self.size[1]) - origin[1]) as u16,
        ];
        (origin, extent)
    }

    /// Makes this surface empty and returns its pixmap for reuse.
    pub fn clear(&mut self) -> Option<Pixmap> {
        self.reached.fill(false);
        self.span = [0; 4];
        self.pixmap.take()
    }

    /// Takes `pixmap`, drawn over `span` (see `area`), with `reached` marking
    /// the tiles of the whole grid that hold anything.
    pub fn set(&mut self, span: [u32; 4], pixmap: Pixmap, reached: Vec<bool>) {
        let (_, extent) = self.area(span);
        assert_eq!([pixmap.width(), pixmap.height()], extent);
        assert_eq!(reached.len(), self.reached.len());
        self.span = span;
        self.pixmap = Some(pixmap);
        self.reached = reached;
    }

    /// Marks the tiles under `rect` (left, top, right, bottom pixels) as
    /// reached, growing the pixels to cover them; what is there stays.
    pub fn ensure(&mut self, rect: [u32; 4]) {
        let wanted = [
            rect[0] / self.edge,
            rect[1] / self.edge,
            rect[2].div_ceil(self.edge).min(self.grid[0]),
            rect[3].div_ceil(self.edge).min(self.grid[1]),
        ];
        if wanted[0] >= wanted[2] || wanted[1] >= wanted[3] {
            return;
        }
        for row in wanted[1]..wanted[3] {
            for column in wanted[0]..wanted[2] {
                self.reached[(row * self.grid[0] + column) as usize] = true;
            }
        }
        let span = match self.pixmap {
            Some(_) => [
                self.span[0].min(wanted[0]),
                self.span[1].min(wanted[1]),
                self.span[2].max(wanted[2]),
                self.span[3].max(wanted[3]),
            ],
            None => wanted,
        };
        if self.pixmap.is_some() && span == self.span {
            return;
        }
        let (origin, extent) = self.area(span);
        let mut grown = Pixmap::new(extent[0], extent[1]);
        if let Some(old) = self.pixmap.take() {
            let (old_origin, old_extent) = self.area(self.span);
            let width = usize::from(old_extent[0]) * 4;
            let target_row = usize::from(extent[0]) * 4;
            let target = grown.data_as_u8_slice_mut();
            for (y, line) in old.data_as_u8_slice().chunks_exact(width).enumerate() {
                let at = (old_origin[1] - origin[1]) as usize + y;
                let from = at * target_row + (old_origin[0] - origin[0]) as usize * 4;
                target[from..from + width].copy_from_slice(line);
            }
        }
        self.span = span;
        self.pixmap = Some(grown);
    }

    /// The rows of `rect`, which `ensure` covered, to change in place: each
    /// row's y and its pixels from `rect`'s left to right.
    pub fn rows_mut(&mut self, rect: [u32; 4]) -> impl Iterator<Item = (u32, &mut [Pixel])> {
        let (origin, extent) = self.area(self.span);
        let width = usize::from(extent[0]);
        let pixmap = self.pixmap.as_mut().expect("ensure covered the rows");
        let [left, top, right, bottom] = rect;
        assert!(left >= origin[0] && top >= origin[1]);
        assert!(right <= origin[0] + u32::from(extent[0]));
        assert!(bottom <= origin[1] + u32::from(extent[1]));
        pixmap
            .data_as_u8_slice_mut()
            .as_chunks_mut::<4>()
            .0
            .chunks_exact_mut(width)
            .enumerate()
            .skip((top - origin[1]) as usize)
            .take((bottom - top) as usize)
            .map(move |(row, line)| {
                (
                    origin[1] + row as u32,
                    &mut line[(left - origin[0]) as usize..(right - origin[0]) as usize],
                )
            })
    }

    /// The parts of pixel row `y` within `left..right` in reached tiles, as
    /// the x where each part starts and its pixels.
    pub fn row(&self, y: u32, left: u32, right: u32) -> impl Iterator<Item = (u32, &[Pixel])> {
        let row = y / self.edge;
        let pixmap = self
            .pixmap
            .as_ref()
            .filter(|_| (self.span[1]..self.span[3]).contains(&row));
        let (origin, extent) = self.area(self.span);
        let line = pixmap.map(|pixmap| {
            let width = usize::from(extent[0]);
            let from = (y - origin[1]) as usize * width;
            &pixmap.data_as_u8_slice().as_chunks::<4>().0[from..from + width]
        });
        let first = (left / self.edge).max(self.span[0]);
        let last = right.div_ceil(self.edge).min(self.span[2]);
        line.into_iter().flat_map(move |line| {
            (first..last).filter_map(move |column| {
                if !self.reached[(row * self.grid[0] + column) as usize] {
                    return None;
                }
                let tile_left = column * self.edge;
                let tile_right = (tile_left + self.edge).min(self.size[0]);
                let from = left.max(tile_left);
                let to = right.min(tile_right);
                Some((
                    from,
                    &line[(from - origin[0]) as usize..(to - origin[0]) as usize],
                ))
            })
        })
    }

    /// The whole surface as a pixmap.
    pub fn to_pixmap(&self) -> Pixmap {
        let [width, height] = self.size;
        let mut pixmap = Pixmap::new(width as u16, height as u16);
        let row_pixels = width as usize;
        let out = pixmap.data_as_u8_slice_mut().as_chunks_mut::<4>().0;
        for y in 0..height {
            for (x, part) in self.row(y, 0, width) {
                let from = y as usize * row_pixels + x as usize;
                out[from..from + part.len()].copy_from_slice(part);
            }
        }
        pixmap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_come_from_the_reached_tiles() {
        let mut surface = TiledSurface::new([10, 6], 4);
        assert_eq!(surface.grid(), [3, 2]);
        // Pixels over tile columns 1..3 and rows 0..2, all but the tile at
        // column 1, row 0 reached.
        let span = [1, 0, 3, 2];
        let (origin, extent) = surface.area(span);
        assert_eq!((origin, extent), ([4, 0], [6, 6]));
        let mut piece = Pixmap::new(extent[0], extent[1]);
        for (index, pixel) in piece
            .data_as_u8_slice_mut()
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .enumerate()
        {
            *pixel = [1, 2, 3, index as u8 + 1];
        }
        let mut reached = vec![true; 6];
        reached[0] = false;
        reached[1] = false;
        surface.set(span, piece, reached);
        assert_eq!(surface.tile_count(), 4);
        let parts: Vec<_> = surface
            .row(1, 2, 9)
            .map(|(x, part)| (x, part.len()))
            .collect();
        assert_eq!(parts, [(8, 1)]);
        let parts: Vec<_> = surface
            .row(5, 0, 10)
            .map(|(x, part)| (x, part[0][3]))
            .collect();
        assert_eq!(parts, [(4, 31), (8, 35)]);
        assert_eq!(surface.to_pixmap().data_as_u8_slice()[..4], [0; 4]);
        assert!(surface.clear().is_some());
        assert_eq!(surface.row(5, 0, 10).count(), 0);
    }

    #[test]
    fn ensuring_tiles_grows_the_pixels_and_keeps_them() {
        let mut surface = TiledSurface::new([12, 12], 4);
        surface.ensure([5, 5, 6, 6]);
        for (_, line) in surface.rows_mut([5, 5, 6, 6]) {
            line[0] = [9, 9, 9, 9];
        }
        assert_eq!(surface.tile_count(), 1);
        surface.ensure([0, 9, 2, 10]);
        assert_eq!(surface.tile_count(), 2);
        assert_eq!(surface.bytes(), 8 * 8 * 4);
        let pixmap = surface.to_pixmap();
        let at = |x: usize, y: usize| &pixmap.data_as_u8_slice()[(y * 12 + x) * 4..][..4];
        assert_eq!(at(5, 5), [9, 9, 9, 9]);
        assert_eq!(at(0, 9), [0, 0, 0, 0]);
        // Tiles in the grown area that nothing reached stay empty.
        assert_eq!(
            surface.row(5, 0, 12).map(|(x, _)| x).collect::<Vec<_>>(),
            [4]
        );
    }
}
