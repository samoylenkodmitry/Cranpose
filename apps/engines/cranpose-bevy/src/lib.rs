//! Bevy and Cranpose on one wgpu device.
//!
//! [`BevyView`] runs a Bevy app on its own thread and hands its frames to a
//! composition. [`hud::CranposeHudPlugin`] draws a composition over a Bevy
//! camera. Both share the device without copies: Bevy needs the `wgpu`
//! package that Cranpose uses, which the engines workspace patches in.

pub mod hud;
mod view;

use bevy::render::{
    renderer::{RenderAdapter, RenderAdapterInfo, RenderDevice, RenderInstance, RenderQueue},
    settings::RenderCreation,
};
use cranpose::{GpuContext, GpuRequirements};
pub use view::BevyView;

/// What Bevy's renderer needs from Cranpose's device: every feature the
/// adapter offers but mappable primary buffers, and the adapter's limits.
pub fn gpu_requirements() -> GpuRequirements {
    GpuRequirements {
        features: wgpu::Features::all() - wgpu::Features::MAPPABLE_PRIMARY_BUFFERS,
        adapter_limits: true,
    }
}

/// Bevy's renderer on `gpu`, for Bevy's `RenderPlugin`.
pub fn render_creation(gpu: &GpuContext) -> RenderCreation {
    RenderCreation::manual(
        RenderDevice::new((*gpu.device).clone()),
        RenderQueue::new((*gpu.queue).clone()),
        RenderAdapterInfo::new(gpu.adapter.get_info()),
        RenderAdapter::new(gpu.adapter.clone()),
        RenderInstance::new(gpu.instance.clone()),
    )
}
