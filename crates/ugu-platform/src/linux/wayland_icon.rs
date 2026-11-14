// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 NewfolderGames

//! Wayland `xdg_toplevel_icon_v1` pixel icon attachment.
//!
//! `winit` 0.30 leaves `set_window_icon` empty on Wayland, so a binary started
//! outside an installed `.desktop` entry (such as `cargo run`, a local release
//! build, or a portable AppImage) falls back to the compositor's generic
//! Wayland placeholder. When the compositor advertises
//! `xdg_toplevel_icon_manager_v1` (KWin 6.1+, wlroots), this module attaches
//! premultiplied `wl_shm` `Argb8888` mip buffers directly to winit's
//! `xdg_toplevel` before the first frame commits.

use std::ffi::c_void;
use std::io::Write;
use std::os::fd::{AsFd, FromRawFd, OwnedFd};

use wayland_backend::client::ObjectId;
use wayland_client::protocol::{
    wl_buffer::WlBuffer,
    wl_registry::WlRegistry,
    wl_shm::{self, WlShm},
    wl_shm_pool::WlShmPool,
};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;
use wayland_protocols::xdg::toplevel_icon::v1::client::{
    xdg_toplevel_icon_manager_v1::{self, XdgToplevelIconManagerV1},
    xdg_toplevel_icon_v1::XdgToplevelIconV1,
};

use super::window::WaylandState;

const MIN_MIP_EDGE: u32 = 16;

pub(crate) struct WaylandIcon {
    toplevel: Option<XdgToplevel>,
    source: Option<(u32, Vec<u8>)>,
    shm: Option<WlShm>,
    manager: Option<XdgToplevelIconManagerV1>,
    pool: Option<WlShmPool>,
    buffers: Vec<WlBuffer>,
    icon: Option<XdgToplevelIconV1>,
    applied: bool,
}

impl WaylandIcon {
    /// Wraps winit's foreign `xdg_toplevel` (if present) without taking
    /// ownership of its Wayland lifecycle.
    ///
    /// # Safety
    /// If `toplevel` is `Some`, it must be a valid `*mut xdg_toplevel` on
    /// `conn` that outlives `LinuxWindow`.
    pub(crate) unsafe fn new(
        conn: &Connection,
        toplevel: Option<*mut c_void>,
        source: Option<(u32, Vec<u8>)>,
    ) -> Self {
        let toplevel = toplevel.and_then(|ptr| {
            // SAFETY: `ptr` is winit's live `xdg_toplevel` on `conn`.
            let id = unsafe { ObjectId::from_ptr(XdgToplevel::interface(), ptr.cast()) }.ok()?;
            XdgToplevel::from_id(conn, id).ok()
        });
        Self {
            toplevel,
            source,
            shm: None,
            manager: None,
            pool: None,
            buffers: Vec::new(),
            icon: None,
            applied: false,
        }
    }

    pub(crate) fn bind_global(
        &mut self,
        interface: &str,
        name: u32,
        version: u32,
        registry: &WlRegistry,
        qh: &QueueHandle<WaylandState>,
    ) -> bool {
        match interface {
            "wl_shm" if self.shm.is_none() => {
                self.shm = Some(registry.bind::<WlShm, _, _>(name, version.min(1), qh, ()));
                true
            }
            "xdg_toplevel_icon_manager_v1" if self.manager.is_none() => {
                self.manager = Some(registry.bind::<XdgToplevelIconManagerV1, _, _>(
                    name,
                    version.min(1),
                    qh,
                    (),
                ));
                true
            }
            _ => false,
        }
    }

    /// Uploads premultiplied `Argb8888` square mip buffers and assigns the icon
    /// to `xdg_toplevel`. The state is double-buffered and takes effect on the
    /// window's next `wl_surface.commit`.
    pub(crate) fn apply(&mut self, qh: &QueueHandle<WaylandState>) {
        if self.applied {
            return;
        }
        let (Some(toplevel), Some(shm), Some(manager), Some((edge, rgba))) = (
            self.toplevel.as_ref(),
            self.shm.as_ref(),
            self.manager.as_ref(),
            self.source.take(),
        ) else {
            return;
        };
        self.applied = true;

        let mips = build_argb8888_mips(edge, &rgba);
        if mips.is_empty() {
            return;
        }
        let total_bytes: usize = mips.iter().map(|(_, bytes)| bytes.len()).sum();
        let Ok(total_i32) = i32::try_from(total_bytes) else {
            return;
        };
        let Some(fd) = create_shm_fd(&mips) else {
            return;
        };

        let pool = shm.create_pool(fd.as_fd(), total_i32, qh, ());
        let icon = manager.create_icon(qh, ());
        let mut buffers = Vec::new();
        let mut offset: i32 = 0;

        for (size, bytes) in &mips {
            let Ok(side) = i32::try_from(*size) else {
                continue;
            };
            let Some(stride) = side.checked_mul(4) else {
                continue;
            };
            let buf_1x =
                pool.create_buffer(offset, side, side, stride, wl_shm::Format::Argb8888, qh, ());
            icon.add_buffer(&buf_1x, 1);
            buffers.push(buf_1x);

            if *size >= MIN_MIP_EDGE * 2 && size.is_multiple_of(2) {
                let buf_2x = pool.create_buffer(
                    offset,
                    side,
                    side,
                    stride,
                    wl_shm::Format::Argb8888,
                    qh,
                    (),
                );
                icon.add_buffer(&buf_2x, 2);
                buffers.push(buf_2x);
            }

            let Ok(len_i32) = i32::try_from(bytes.len()) else {
                break;
            };
            offset = offset.saturating_add(len_i32);
        }

        manager.set_icon(toplevel, Some(&icon));
        self.icon = Some(icon);
        self.buffers = buffers;
        self.pool = Some(pool);
    }

    pub(crate) fn cleanup(&mut self) {
        // `xdg_toplevel_icon_v1` raises `no_buffer` if any `wl_buffer` is
        // destroyed while the icon object still exists, so destroy `icon` first.
        if let Some(icon) = self.icon.take() {
            icon.destroy();
        }
        for buffer in self.buffers.drain(..) {
            buffer.destroy();
        }
        if let Some(pool) = self.pool.take() {
            pool.destroy();
        }
        if let Some(manager) = self.manager.take() {
            manager.destroy();
        }
    }
}

fn create_shm_fd(mips: &[(u32, Vec<u8>)]) -> Option<OwnedFd> {
    // SAFETY: static NUL-terminated name with `MFD_CLOEXEC` creates a fresh
    // anonymous memory descriptor owned by the caller.
    let raw = unsafe { libc::memfd_create(c"ugurugu-icon".as_ptr(), libc::MFD_CLOEXEC) };
    if raw < 0 {
        return None;
    }
    // SAFETY: `raw >= 0` is a newly created descriptor.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let mut file = std::fs::File::from(fd);
    for (_, bytes) in mips {
        file.write_all(bytes).ok()?;
    }
    Some(OwnedFd::from(file))
}

/// Halves a square straight-RGBA8 icon down to at most `max_edge` so X11
/// `_NET_WM_ICON` and Wayland `wl_shm` pools carry compact 256 px sources
/// rather than multi-megabyte 1024 px bitmaps.
pub fn shrink_square_rgba(
    mut edge: u32,
    mut rgba: Vec<u8>,
    max_edge: u32,
) -> Option<(u32, Vec<u8>)> {
    let expected = (edge as usize)
        .checked_mul(edge as usize)
        .and_then(|px| px.checked_mul(4))?;
    if edge < MIN_MIP_EDGE || rgba.len() != expected {
        return None;
    }
    while edge > max_edge && edge >= MIN_MIP_EDGE * 2 && edge.is_multiple_of(2) {
        rgba = halve_square_rgba(edge, &rgba);
        edge /= 2;
    }
    Some((edge, rgba))
}

/// Builds premultiplied `WL_SHM_FORMAT_ARGB8888` square mip buffers starting at
/// `(edge, rgba)` and halving by 2x2 premultiplied box average down to 16 px.
fn build_argb8888_mips(edge: u32, rgba: &[u8]) -> Vec<(u32, Vec<u8>)> {
    let expected = (edge as usize)
        .checked_mul(edge as usize)
        .and_then(|px| px.checked_mul(4));
    if edge < MIN_MIP_EDGE || expected != Some(rgba.len()) {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut current_edge = edge;
    let mut current_rgba = rgba.to_vec();
    loop {
        out.push((current_edge, rgba_to_premultiplied_argb8888(&current_rgba)));
        if current_edge < MIN_MIP_EDGE * 2 || !current_edge.is_multiple_of(2) {
            break;
        }
        current_rgba = halve_square_rgba(current_edge, &current_rgba);
        current_edge /= 2;
    }
    out
}

fn rgba_to_premultiplied_argb8888(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len());
    for px in rgba.as_chunks::<4>().0 {
        let a = u32::from(px[3]);
        let premul = |c: u8| ((u32::from(c) * a + 127) / 255) as u8;
        let word = (a << 24)
            | (u32::from(premul(px[0])) << 16)
            | (u32::from(premul(px[1])) << 8)
            | u32::from(premul(px[2]));
        out.extend_from_slice(&word.to_ne_bytes());
    }
    out
}

/// Halves a square straight-RGBA8 image with alpha-weighted box filtering so
/// transparent silhouettes keep clean colours at smaller taskbar sizes.
pub(crate) fn halve_square_rgba(edge: u32, rgba: &[u8]) -> Vec<u8> {
    let next = (edge / 2) as usize;
    let stride = (edge as usize) * 4;
    let mut out = vec![0_u8; next * next * 4];

    for y in 0..next {
        let row0 = (y * 2) * stride;
        let row1 = row0 + stride;
        for x in 0..next {
            let col0 = (x * 2) * 4;
            let samples = [
                &rgba[row0 + col0..row0 + col0 + 4],
                &rgba[row0 + col0 + 4..row0 + col0 + 8],
                &rgba[row1 + col0..row1 + col0 + 4],
                &rgba[row1 + col0 + 4..row1 + col0 + 8],
            ];
            let mut sum_r = 0_u32;
            let mut sum_g = 0_u32;
            let mut sum_b = 0_u32;
            let mut sum_a = 0_u32;
            for px in samples {
                let a = u32::from(px[3]);
                sum_r += u32::from(px[0]) * a;
                sum_g += u32::from(px[1]) * a;
                sum_b += u32::from(px[2]) * a;
                sum_a += a;
            }
            let dst = (y * next + x) * 4;
            let avg = |sum_c: u32| {
                (sum_c + sum_a / 2)
                    .checked_div(sum_a)
                    .map_or(0, |v| v.min(255) as u8)
            };
            out[dst] = avg(sum_r);
            out[dst + 1] = avg(sum_g);
            out[dst + 2] = avg(sum_b);
            out[dst + 3] = ((sum_a + 2) / 4) as u8;
        }
    }
    out
}

impl Dispatch<WlShm, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlShm,
        _: wl_shm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlShmPool, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlShmPool,
        _: <WlShmPool as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlBuffer, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlBuffer,
        _: <WlBuffer as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<XdgToplevelIconManagerV1, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &XdgToplevelIconManagerV1,
        _: xdg_toplevel_icon_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<XdgToplevelIconV1, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &XdgToplevelIconV1,
        _: <XdgToplevelIconV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiplied_argb8888_packs_native_words_and_builds_mips_to_16px() {
        let mut rgba = vec![0_u8; 128 * 128 * 4];
        rgba[0..4].copy_from_slice(&[200, 100, 50, 128]);

        let mips = build_argb8888_mips(128, &rgba);
        let sizes: Vec<u32> = mips.iter().map(|(edge, _)| *edge).collect();
        assert_eq!(sizes, vec![128, 64, 32, 16]);

        let word = u32::from_ne_bytes(mips[0].1[0..4].try_into().unwrap());
        assert_eq!(word >> 24, 128);
        assert_eq!((word >> 16) & 0xff, 100);
        assert_eq!((word >> 8) & 0xff, 50);
        assert_eq!(word & 0xff, 25);
    }

    #[test]
    fn shrink_square_rgba_halves_down_to_max_edge() {
        let rgba = vec![255_u8; 1024 * 1024 * 4];
        let (edge, shrunk) = shrink_square_rgba(1024, rgba, 256).expect("valid square icon");
        assert_eq!(edge, 256);
        assert_eq!(shrunk.len(), 256 * 256 * 4);
    }

    #[test]
    fn alpha_weighted_halving_keeps_foreground_colour_against_transparent_neighbours() {
        let mut rgba = vec![0_u8; 4 * 4 * 4];
        rgba[0..4].copy_from_slice(&[240, 180, 60, 255]);
        let half = halve_square_rgba(4, &rgba);
        assert_eq!(&half[0..4], &[240, 180, 60, 64]);
    }
}
