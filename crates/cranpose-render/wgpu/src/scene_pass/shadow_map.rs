// The shadow map of a scene: the depth of its shadow casters as its light sees them, and the bind
// group through which the surfaces read it.

use super::{DEPTH_FORMAT, SHADOW_MAP_SIZE, ScenePipelineParts};

/// The two ways a surface reads the shadow map: compared with its own depth
/// and filtered, and as the depth of the nearest caster, which the search
/// for a soft shadow's casters reads. The map is bound once for each, as
/// GLSL samples a texture with one sampler.
pub(super) struct ShadowSamplers {
    pub(super) compare: wgpu::Sampler,
    pub(super) depth: wgpu::Sampler,
}

/// The depth of a scene's shadow casters as its light sees them.
pub(crate) struct ShadowMap {
    pub(crate) view: wgpu::TextureView,
    pub(crate) group: wgpu::BindGroup,
}

/// A shadow map binding of the scene's shadow bind group, which the shader
/// reads as `sample_type`.
pub(super) fn shadow_map_entry(
    binding: u32,
    sample_type: wgpu::TextureSampleType,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

pub(super) fn depth_texture(
    device: &wgpu::Device,
    label: &'static str,
    side: u32,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

pub(super) fn shadow_group(
    device: &wgpu::Device,
    (layout, samplers): (&wgpu::BindGroupLayout, &ShadowSamplers),
    view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Scene Shadow Map Bind Group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&samplers.compare),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(&samplers.depth),
            },
        ],
    })
}

pub(super) fn create_shadow_map(device: &wgpu::Device, parts: &ScenePipelineParts) -> ShadowMap {
    let view = depth_texture(device, "Scene Shadow Map", SHADOW_MAP_SIZE);
    let group = shadow_group(
        device,
        (&parts.shadow_layout, &parts.shadow_samplers),
        &view,
    );
    ShadowMap { view, group }
}
