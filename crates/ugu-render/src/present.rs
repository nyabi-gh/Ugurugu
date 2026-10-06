// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Frame pacing and display timing for a DX12 swap chain.
//!
//! A frame waits for the swap chain's frame-latency object before it reads
//! input, so the input it shows is the newest one when a buffer is actually
//! free; acquiring then never blocks. Waiting after reading input instead
//! would show input one frame late. Display times come from DXGI frame
//! statistics, the only per-present display record available to the app.

use std::collections::VecDeque;
use std::time::Duration;

use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::Graphics::Dxgi::{DXGI_FRAME_STATISTICS, IDXGISwapChain3};
use windows::Win32::System::Threading::WaitForSingleObject;

/// Swap chain buffers the app may queue ahead of the display. One keeps
/// input-to-display at one refresh after the frame is ready.
const MAXIMUM_FRAME_LATENCY: u32 = 1;

/// Presents waiting for their display time are dropped after this many, so a
/// window that never reaches the screen cannot grow the queue.
const PENDING_LIMIT: usize = 64;

/// Backend options the presenter relies on: wgpu must not wait on the
/// frame-latency object itself, or the input read before acquiring is stale.
pub fn backend_options() -> wgpu::BackendOptions {
    wgpu::BackendOptions {
        dx12: wgpu::Dx12BackendOptions {
            latency_waitable_object: wgpu::Dx12UseFrameLatencyWaitableObject::DontWait,
            ..wgpu::Dx12BackendOptions::from_env_or_default()
        },
        ..wgpu::BackendOptions::from_env_or_default()
    }
}

/// A present whose display time is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Displayed {
    /// The QPC time of the oldest input the frame showed.
    pub input_qpc: u64,
    /// The QPC time of the vertical blank that started showing it.
    pub display_qpc: u64,
}

pub enum Acquired {
    Frame(wgpu::SurfaceTexture),
    /// Nothing to draw into now, for example while minimised; try again later.
    Skip,
}

pub struct Presenter {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// Present counts with the input time they showed, oldest first.
    pending: VecDeque<(u32, u64)>,
}

impl Presenter {
    pub fn new(
        surface: wgpu::Surface<'static>,
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        size: [u32; 2],
    ) -> Result<Self, String> {
        let capabilities = surface.get_capabilities(adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| !format.is_srgb())
            .or_else(|| capabilities.formats.first().copied())
            .ok_or("the surface supports no texture format")?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size[0].max(1),
            height: size[1].max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: MAXIMUM_FRAME_LATENCY,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            // V1 colour policy: SDR sRGB only.
            color_space: wgpu::SurfaceColorSpace::Srgb,
            view_formats: Vec::new(),
        };
        surface.configure(device, &config);
        Ok(Self {
            surface,
            config,
            pending: VecDeque::new(),
        })
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    pub fn size(&self) -> [u32; 2] {
        [self.config.width, self.config.height]
    }

    pub fn resize(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        if size[0] == 0 || size[1] == 0 || size == self.size() {
            return;
        }
        self.config.width = size[0];
        self.config.height = size[1];
        self.surface.configure(device, &self.config);
        // Present counts restart with a new buffer configuration.
        self.pending.clear();
    }

    /// Waits until the swap chain can take another frame. Returns `false` on
    /// timeout, in which case the caller should not draw yet.
    pub fn wait_for_frame(&self, timeout: Duration) -> bool {
        let Some(handle) = self.waitable() else {
            return true;
        };
        let millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: the handle belongs to the live swap chain of `self.surface`.
        unsafe { WaitForSingleObject(handle, millis) == WAIT_OBJECT_0 }
    }

    pub fn acquire(&mut self, device: &wgpu::Device) -> Acquired {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Acquired::Frame(frame),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(device, &self.config);
                self.pending.clear();
                Acquired::Skip
            }
            other => {
                tracing::debug!(?other, "no swap chain buffer this frame");
                Acquired::Skip
            }
        }
    }

    /// Presents `frame`, remembering the oldest input it shows so that its
    /// display time can be reported once known.
    pub fn present(
        &mut self,
        queue: &wgpu::Queue,
        frame: wgpu::SurfaceTexture,
        input_qpc: Option<u64>,
    ) {
        queue.present(frame);
        let (Some(input_qpc), Some(swap_chain)) = (input_qpc, self.swap_chain()) else {
            return;
        };
        // SAFETY: plain query on a live swap chain.
        if let Ok(count) = unsafe { swap_chain.GetLastPresentCount() } {
            if self.pending.len() == PENDING_LIMIT {
                self.pending.pop_front();
            }
            self.pending.push_back((count, input_qpc));
        }
    }

    pub fn has_pending_display_times(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Display times that became known since the last call. Statistics only
    /// describe the newest displayed present, so presents replaced before
    /// the call are dropped rather than given a guessed time.
    pub fn take_display_times(&mut self) -> Vec<Displayed> {
        let Some(swap_chain) = self.swap_chain() else {
            return Vec::new();
        };
        let mut stats = DXGI_FRAME_STATISTICS::default();
        // SAFETY: valid out pointer on a live swap chain. Disjoint or not yet
        // available statistics are an error and simply mean "nothing new".
        if unsafe { swap_chain.GetFrameStatistics(&mut stats) }.is_err() {
            return Vec::new();
        }
        settle(
            &mut self.pending,
            stats.PresentCount,
            stats.SyncQPCTime as u64,
        )
    }

    fn swap_chain(&self) -> Option<IDXGISwapChain3> {
        // SAFETY: the hal surface is only read here.
        let hal = unsafe { self.surface.as_hal::<wgpu::hal::api::Dx12>() }?;
        hal.swap_chain()
    }

    fn waitable(&self) -> Option<HANDLE> {
        // SAFETY: the hal surface is only read here.
        let hal = unsafe { self.surface.as_hal::<wgpu::hal::api::Dx12>() }?;
        // SAFETY: the swap chain is replaced only by `configure`, which needs
        // `&mut self`, so the handle stays valid while `self` is borrowed.
        unsafe { hal.waitable_handle() }
    }
}

/// Resolves pending presents against the newest displayed present: that one
/// gets `display_qpc`; older ones were replaced unseen and are dropped.
fn settle(pending: &mut VecDeque<(u32, u64)>, shown: u32, display_qpc: u64) -> Vec<Displayed> {
    let mut displayed = Vec::new();
    while let Some(&(count, input_qpc)) = pending.front() {
        let ahead = count.wrapping_sub(shown) as i32;
        if ahead > 0 {
            break;
        }
        pending.pop_front();
        if ahead == 0 {
            displayed.push(Displayed {
                input_qpc,
                display_qpc,
            });
        }
    }
    displayed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shown_present_gets_the_display_time() {
        let mut pending = VecDeque::from([(7, 100), (8, 200)]);
        assert_eq!(
            settle(&mut pending, 7, 900),
            [Displayed {
                input_qpc: 100,
                display_qpc: 900
            }]
        );
        assert_eq!(pending, [(8, 200)]);
    }

    #[test]
    fn replaced_presents_are_dropped_without_a_time() {
        let mut pending = VecDeque::from([(5, 1), (6, 2), (7, 3)]);
        assert_eq!(settle(&mut pending, 7, 50).len(), 1);
        assert!(pending.is_empty());
    }

    #[test]
    fn presents_not_shown_yet_stay_pending() {
        let mut pending = VecDeque::from([(9, 1)]);
        assert!(settle(&mut pending, 8, 50).is_empty());
        assert_eq!(pending, [(9, 1)]);
    }

    #[test]
    fn present_counts_wrap() {
        let mut pending = VecDeque::from([(u32::MAX, 1), (0, 2)]);
        assert_eq!(settle(&mut pending, 0, 50)[0].input_qpc, 2);
        assert!(pending.is_empty());
    }
}
