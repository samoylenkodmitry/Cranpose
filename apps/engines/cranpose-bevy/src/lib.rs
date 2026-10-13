pub mod hud;
mod view;

use bevy::render::{
    renderer::{RenderAdapter, RenderAdapterInfo, RenderDevice, RenderInstance, RenderQueue},
    settings::RenderCreation,
};
use cranpose::{GpuContext, GpuRequirements};
pub use view::BevyView;

pub fn gpu_requirements() -> GpuRequirements {
    GpuRequirements {
        features: wgpu::Features::all() - wgpu::Features::MAPPABLE_PRIMARY_BUFFERS,
        adapter_limits: true,
    }
}

pub fn render_creation(gpu: &GpuContext) -> RenderCreation {
    RenderCreation::manual(
        RenderDevice::new((*gpu.device).clone()),
        RenderQueue::new((*gpu.queue).clone()),
        RenderAdapterInfo::new(gpu.adapter.get_info()),
        RenderAdapter::new(gpu.adapter.clone()),
        RenderInstance::new(gpu.instance.clone()),
    )
}
