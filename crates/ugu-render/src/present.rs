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
use windows::Win32::System::Performance::QueryPerformanceCounter;
use windows::Win32::System::Threading::WaitForMultipleObjects;

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
    /// The QPC time just before the frame was presented.
    pub present_qpc: u64,
    /// The QPC time of the vertical blank that started showing it.
    pub display_qpc: u64,
}

pub enum Acquired {
    Frame(wgpu::SurfaceTexture),
    /// Nothing to draw into now, for example while minimised; try again later.
    Skip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pending {
    count: u32,
    input_qpc: u64,
    present_qpc: u64,
}

/// Waits until every swap chain can take another frame. Returns `false` on
/// timeout, in which case the caller should not draw yet. All are waited for
/// at once, so a timeout leaves no swap chain's frame slot taken.
pub fn wait_for_frames(presenters: &[&Presenter], timeout: Duration) -> bool {
    let handles: Vec<HANDLE> = presenters
        .iter()
        .filter_map(|presenter| presenter.waitable())
        .collect();
    if handles.is_empty() {
        return true;
    }
    let millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: the handles belong to the live swap chains of `presenters`.
    let result = unsafe { WaitForMultipleObjects(&handles, true, millis) };
    // With every handle awaited, any index in the signalled range means success.
    (WAIT_OBJECT_0.0..WAIT_OBJECT_0.0 + handles.len() as u32).contains(&result.0)
}

pub struct Presenter {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// Presents awaiting their display time, oldest first.
    pending: VecDeque<Pending>,
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
        tracing::info!(?format, ?size, "swap chain");
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
        let present_qpc = qpc();
        queue.present(frame);
        let (Some(input_qpc), Some(swap_chain)) = (input_qpc, self.swap_chain()) else {
            return;
        };
        // SAFETY: plain query on a live swap chain.
        if let Ok(count) = unsafe { swap_chain.GetLastPresentCount() } {
            if self.pending.len() == PENDING_LIMIT {
                self.pending.pop_front();
            }
            self.pending.push_back(Pending {
                count,
                input_qpc,
                present_qpc,
            });
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
fn settle(pending: &mut VecDeque<Pending>, shown: u32, display_qpc: u64) -> Vec<Displayed> {
    let mut displayed = Vec::new();
    while let Some(&front) = pending.front() {
        let ahead = front.count.wrapping_sub(shown) as i32;
        if ahead > 0 {
            break;
        }
        pending.pop_front();
        if ahead == 0 {
            displayed.push(Displayed {
                input_qpc: front.input_qpc,
                present_qpc: front.present_qpc,
                display_qpc,
            });
        }
    }
    displayed
}

fn qpc() -> u64 {
    let mut value = 0i64;
    // SAFETY: valid out pointer; the call cannot fail on supported Windows.
    let _ = unsafe { QueryPerformanceCounter(&mut value) };
    value as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(entries: &[(u32, u64)]) -> VecDeque<Pending> {
        entries
            .iter()
            .map(|&(count, input_qpc)| Pending {
                count,
                input_qpc,
                present_qpc: input_qpc + 1,
            })
            .collect()
    }

    #[test]
    fn the_shown_present_gets_the_display_time() {
        let mut queue = pending(&[(7, 100), (8, 200)]);
        assert_eq!(
            settle(&mut queue, 7, 900),
            [Displayed {
                input_qpc: 100,
                present_qpc: 101,
                display_qpc: 900
            }]
        );
        assert_eq!(queue, pending(&[(8, 200)]));
    }

    #[test]
    fn replaced_presents_are_dropped_without_a_time() {
        let mut queue = pending(&[(5, 1), (6, 2), (7, 3)]);
        assert_eq!(settle(&mut queue, 7, 50).len(), 1);
        assert!(queue.is_empty());
    }

    #[test]
    fn presents_not_shown_yet_stay_pending() {
        let mut queue = pending(&[(9, 1)]);
        assert!(settle(&mut queue, 8, 50).is_empty());
        assert_eq!(queue, pending(&[(9, 1)]));
    }

    #[test]
    fn present_counts_wrap() {
        let mut queue = pending(&[(u32::MAX, 1), (0, 2)]);
        assert_eq!(settle(&mut queue, 0, 50)[0].input_qpc, 2);
        assert!(queue.is_empty());
    }
}
