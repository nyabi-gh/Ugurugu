// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Opening a DX12 device for the window, falling back to WARP, and noticing
//! when the device is lost so the caller can open a new one.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Graphics::Direct3D12::ID3D12Device5;
use windows::core::Interface;

/// Which adapters to try.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdapterChoice {
    /// The high-performance hardware adapter, or WARP when none can show the
    /// window.
    Hardware,
    /// WARP only, for testing the software path.
    Warp,
}

impl AdapterChoice {
    /// `UGURUGU_ADAPTER=warp` forces WARP; anything else is the default.
    pub fn from_env() -> Self {
        match std::env::var("UGURUGU_ADAPTER") {
            Ok(value) if value.eq_ignore_ascii_case("warp") => Self::Warp,
            _ => Self::Hardware,
        }
    }
}

pub struct Gpu {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub summary: String,
    lost: Arc<AtomicBool>,
}

impl Gpu {
    /// Opens the adapter `choice` asks for that can show `surface`.
    pub fn open(
        instance: &wgpu::Instance,
        surface: &wgpu::Surface<'_>,
        choice: AdapterChoice,
    ) -> Result<Self, String> {
        Self::with_adapter(request_adapter(instance, Some(surface), choice)?)
    }

    /// Opens the adapter `choice` asks for before there is a window to show,
    /// so that loading the drivers, which takes most of start-up, runs while
    /// the window is made. Whether it can show the window is checked later
    /// with [`Gpu::can_show`].
    pub fn open_without_surface(
        instance: &wgpu::Instance,
        choice: AdapterChoice,
    ) -> Result<Self, String> {
        Self::with_adapter(request_adapter(instance, None, choice)?)
    }

    pub fn can_show(&self, surface: &wgpu::Surface<'_>) -> bool {
        self.adapter.is_surface_supported(surface)
    }

    fn with_adapter(adapter: wgpu::Adapter) -> Result<Self, String> {
        let summary = adapter_summary(&adapter.get_info());
        tracing::info!(adapter = %summary, "selected GPU adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|error| format!("cannot open the DX12 device: {error}"))?;
        tracing::debug!("GPU device opened");

        let lost = Arc::new(AtomicBool::new(false));
        let flag = lost.clone();
        device.set_device_lost_callback(move |reason, message| {
            tracing::warn!(?reason, message, "GPU device lost");
            flag.store(true, Ordering::Release);
        });
        // The default handler panics. Errors on a removed device are expected
        // until the caller replaces it; any other error is a bug, still worth
        // logging rather than taking the document down.
        let flag = lost.clone();
        device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
            if flag.load(Ordering::Acquire) {
                tracing::debug!(%error, "GPU error on the lost device");
            } else {
                tracing::error!(%error, "GPU error");
            }
        }));
        Ok(Self {
            adapter,
            device,
            queue,
            summary,
            lost,
        })
    }

    /// Whether this is WARP, which draws on the CPU and shows input several
    /// frames late (m0-evidence section 8).
    pub fn is_software(&self) -> bool {
        self.adapter.get_info().device_type == wgpu::DeviceType::Cpu
    }

    /// Whether the device was lost and everything made with it must be
    /// replaced.
    pub fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }

    /// Asks D3D12 whether the device was removed. wgpu reports a removal
    /// only through the device-lost callback, which it calls when a wait on
    /// the device fails; this catches it on the next frame instead.
    pub fn check_removed(&self) -> bool {
        if self.is_lost() {
            return true;
        }
        // SAFETY: the hal device is only queried here.
        let removed = unsafe { self.device.as_hal::<wgpu::hal::api::Dx12>() }
            .is_some_and(|hal| unsafe { hal.raw_device().GetDeviceRemovedReason() }.is_err());
        if removed {
            tracing::warn!("GPU device removed");
            self.lost.store(true, Ordering::Release);
        }
        removed
    }

    /// Puts the device into the removed state, as a driver reset would, for
    /// testing recovery. Affects only this device.
    pub fn remove_for_test(&self) -> Result<(), String> {
        // SAFETY: removal is what this is for; the caller then replaces the
        // device.
        let hal = unsafe { self.device.as_hal::<wgpu::hal::api::Dx12>() }
            .ok_or("the device is not DX12")?;
        let device5: ID3D12Device5 = hal
            .raw_device()
            .cast()
            .map_err(|error| format!("ID3D12Device5 is not available: {error}"))?;
        // SAFETY: plain call on a live device.
        unsafe { device5.RemoveDevice() };
        Ok(())
    }
}

/// The high-performance adapter `choice` asks for, falling back to WARP
/// when no hardware adapter can show `surface`.
fn request_adapter(
    instance: &wgpu::Instance,
    surface: Option<&wgpu::Surface<'_>>,
    choice: AdapterChoice,
) -> Result<wgpu::Adapter, String> {
    let request = |force_fallback_adapter| {
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter,
            compatible_surface: surface,
            apply_limit_buckets: false,
        }))
    };
    match choice {
        AdapterChoice::Warp => request(true),
        AdapterChoice::Hardware => request(false).or_else(|error| {
            tracing::warn!(%error, "no hardware DX12 adapter can show the window; trying WARP");
            request(true)
        }),
    }
    .map_err(|error| format!("no DX12 adapter can show the window: {error}"))
}

fn adapter_summary(info: &wgpu::AdapterInfo) -> String {
    let mut summary = format!("{} ({:?}, {:?}", info.name, info.device_type, info.backend);
    if !info.driver_info.is_empty() {
        summary += &format!(", driver {}", info.driver_info);
    }
    summary + ")"
}
