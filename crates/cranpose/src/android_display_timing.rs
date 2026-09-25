#![expect(unsafe_code)]

//! When each frame reached the screen, from the swapchain's
//! `VK_GOOGLE_display_timing` records.
//!
//! Android keeps a record for every present that carries a present time,
//! saying when the frame was shown. Matched against when each present
//! returned, it tells how many frames were already queued behind a frame as
//! it reached the screen, which is the queue's depth then.
//!
//! Asking for those records turns on frame timestamps for the swapchain, and
//! Android 10's Vulkan loader then crashes destroying that swapchain once a
//! newer one has replaced it: it turns the timestamps off through a window it
//! has already let go. So the swapchain is destroyed while it is still the
//! window's own, before the new configuration is made.

use std::sync::{Arc, mpsc::Sender};

use ash::vk;
use wgpu::hal::{Surface as _, api::Vulkan};

use crate::frame_pacer::PresentLog;

/// When one frame reached the screen, in nanoseconds on the monotonic
/// clock, and how many later frames were queued behind it then.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DisplayedFrame {
    pub(crate) shown_ns: i64,
    pub(crate) queued_behind: u32,
}

/// Tags every present with a present time and hands on the records the
/// swapchain returns for earlier ones.
pub(crate) struct DisplayTimingObserver {
    device: Arc<wgpu::Device>,
    timing: ash::google::display_timing::Device,
    next_present_id: u32,
    presents: PresentLog,
    frames: Sender<DisplayedFrame>,
}

impl DisplayTimingObserver {
    /// An observer for presents made with `device`, sending the frames it
    /// learns about to `frames`, or `None` when the device was created
    /// without `VK_GOOGLE_display_timing`.
    pub(crate) fn new(device: Arc<wgpu::Device>, frames: Sender<DisplayedFrame>) -> Option<Self> {
        if !device
            .features()
            .contains(wgpu::Features::VULKAN_GOOGLE_DISPLAY_TIMING)
        {
            return None;
        }
        // SAFETY: the raw handles only load the extension's function
        // pointers, and the observer keeps `device` alive as long as it
        // calls them.
        let timing = {
            let hal = unsafe { device.as_hal::<Vulkan>() }?;
            ash::google::display_timing::Device::new(
                hal.shared_instance().raw_instance(),
                hal.raw_device(),
            )
        };
        Some(Self {
            device,
            timing,
            next_present_id: 0,
            presents: PresentLog::default(),
            frames,
        })
    }
}

impl cranpose_render_wgpu::PresentObserver for DisplayTimingObserver {
    fn before_present(&mut self, surface: &wgpu::Surface<'static>) {
        // SAFETY: the Vulkan surface is used only within this call, on the
        // thread that presents to it.
        let Some(surface) = (unsafe { surface.as_hal::<Vulkan>() }) else {
            return;
        };
        surface.set_next_present_time(vk::PresentTimeGOOGLE {
            present_id: self.next_present_id,
            desired_present_time: 0,
        });
    }

    fn before_reconfigure(&mut self, surface: &wgpu::Surface<'static>) {
        // SAFETY: the present runtime reconfigures only between frames, so
        // no image of the swapchain is acquired, and only this thread uses
        // the surface; the configuration that follows makes a new swapchain.
        unsafe {
            if let (Some(surface), Some(device)) =
                (surface.as_hal::<Vulkan>(), self.device.as_hal::<Vulkan>())
            {
                surface.unconfigure(&device);
            }
        }
    }

    fn after_present(&mut self, surface: &wgpu::Surface<'static>) {
        self.presents.record(
            self.next_present_id,
            crate::android_frame_telemetry::monotonic_nanos(),
        );
        self.next_present_id = self.next_present_id.wrapping_add(1);
        // SAFETY: as in `before_present`.
        let swapchain = unsafe { surface.as_hal::<Vulkan>() }
            .and_then(|surface| surface.raw_native_swapchain());
        let Some(swapchain) = swapchain else {
            return;
        };
        // SAFETY: the swapchain belongs to the device the extension was
        // loaded from, and only this thread presents to it, so nothing
        // touches it during the call.
        let records = match unsafe { self.timing.get_past_presentation_timing(swapchain) } {
            Ok(records) => records,
            Err(error) => {
                log::debug!("[display-timing] past presentation timing unavailable: {error}");
                return;
            }
        };
        for record in records {
            let shown_ns = record.actual_present_time as i64;
            let queued_behind = self.presents.queued_behind(record.present_id, shown_ns);
            if let (true, Some(queued_behind)) = (shown_ns > 0, queued_behind) {
                let _ = self.frames.send(DisplayedFrame {
                    shown_ns,
                    queued_behind,
                });
            }
        }
    }
}
