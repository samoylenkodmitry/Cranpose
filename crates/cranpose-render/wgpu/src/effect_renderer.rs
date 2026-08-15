//! Effect rendering infrastructure: blur, custom shaders, offscreen passes.
//!
//! This module ties together the offscreen pool, blur pipeline, and shader
//! pipeline cache to apply `RenderEffect`s to subtree-rendered textures.

use crate::frame_graph::{
    FrameCommandRecorder, FrameCommandStats, FrameTextureDescriptor, UploadAllocatorId,
    UploadAllocatorSpec,
};
use crate::offscreen::{OffscreenPool, OffscreenTarget};
use crate::shader_cache::{RuntimeShaderPipelineMode, ShaderPipelineCache};
use crate::shaders;
use cranpose_ui_graphics::{
    BlendMode, RenderEffect, RuntimeShader, TileMode, ROUNDED_ALPHA_MASK_WGSL,
};

use crate::display_clip;
use crate::gpu_stats::FrameStats;
use crate::lazy_resource::{LazyGpuResource, PassPipeline};
use std::cell::Cell;

pub(crate) struct EffectRenderer {
    offscreen_pool: OffscreenPool,
    pub shader_cache: ShaderPipelineCache,

    blur_shader: wgpu::ShaderModule,
    blur_rounded_mask_shader: wgpu::ShaderModule,
    blur_pipeline_layout: wgpu::PipelineLayout,
    blur_pipeline: LazyGpuResource<wgpu::RenderPipeline>,
    blur_rounded_mask_pipeline: LazyGpuResource<wgpu::RenderPipeline>,
    blur_uniform_bind_group_layout: wgpu::BindGroupLayout,

    offset_shader: wgpu::ShaderModule,
    offset_pipeline_layout: wgpu::PipelineLayout,
    offset_pipeline: LazyGpuResource<wgpu::RenderPipeline>,
    offset_uniform_bind_group_layout: wgpu::BindGroupLayout,

    blit_shader: wgpu::ShaderModule,
    /// Kept for the display-clip blit variants, whose module is compiled
    /// lazily from this text with the content z rewritten (see
    /// [`crate::display_clip::with_content_z`]).
    blit_shader_source: String,
    blit_pipeline_layout: wgpu::PipelineLayout,
    blit_pipeline: PassPipeline,
    blit_pipeline_dst_out: PassPipeline,
    blit_uniform_bind_group_layout: wgpu::BindGroupLayout,
    projective_blit_shader: wgpu::ShaderModule,
    projective_blit_pipeline_layout: wgpu::PipelineLayout,
    projective_blit_pipeline: PassPipeline,
    projective_blit_pipeline_dst_out: PassPipeline,

    // Shared bind group layouts for effect texture + uniform access
    pub effect_texture_bind_group_layout: wgpu::BindGroupLayout,
    pub effect_uniform_bind_group_layout: wgpu::BindGroupLayout,

    // Sampler for effect textures
    pub effect_linear_sampler: wgpu::Sampler,

    surface_format: wgpu::TextureFormat,
    adapter_backend: wgpu::Backend,

    // Debug counters (reset each frame by GpuRenderer)
    pub(crate) debug_command_stats: Cell<FrameCommandStats>,
    pub(crate) debug_blurs: Cell<u32>,
    pub(crate) debug_composites: Cell<u32>,
    pub(crate) debug_effects: Cell<u32>,
    pub(crate) debug_upload_bytes: Cell<u64>,
    /// `CRANPOSE_FILL_DIAG` target-area fill of effect passes, device px²,
    /// drained once per frame by the renderer into `FillAreaDiag`'s
    /// effect-composite / offscreen-source buckets. Always zero while the
    /// diagnostic is off.
    #[cfg(not(target_arch = "wasm32"))]
    debug_fill_composite_px2: Cell<f64>,
    #[cfg(not(target_arch = "wasm32"))]
    debug_fill_offscreen_px2: Cell<f64>,
}

/// Approximate fill of one full-target effect pass, in px²: the dest
/// viewport's area when one is set, else the fallback target size, clamped
/// by the scissor when one is set. (The fullscreen triangle shades exactly
/// scissor ∩ viewport; the min of the two areas stands in for the exact
/// intersection — a deliberate `CRANPOSE_FILL_DIAG` approximation.)
#[cfg(not(target_arch = "wasm32"))]
fn effect_pass_fill_px2(
    scissor: Option<(u32, u32, u32, u32)>,
    dest_viewport: Option<(f32, f32, f32, f32)>,
    fallback: (u32, u32),
) -> f64 {
    let scissor_px2 = scissor.map(|(_, _, w, h)| f64::from(w) * f64::from(h));
    let viewport_px2 = dest_viewport
        .filter(|(_, _, w, h)| *w > 0.0 && *h > 0.0)
        .map(|(_, _, w, h)| f64::from(w) * f64::from(h));
    let base = viewport_px2.unwrap_or(f64::from(fallback.0) * f64::from(fallback.1));
    scissor_px2.map_or(base, |scissor_px2| scissor_px2.min(base))
}

#[derive(Clone, Copy)]
pub(crate) struct ProjectiveSurfaceComposite<'a> {
    pub(crate) source: &'a OffscreenTarget,
    pub(crate) source_size: (f32, f32),
    pub(crate) inverse_matrix: [[f32; 3]; 3],
    pub(crate) dest_bounds: [[f32; 2]; 4],
    pub(crate) alpha: f32,
    pub(crate) load_op: wgpu::LoadOp<wgpu::Color>,
    pub(crate) scissor: Option<(u32, u32, u32, u32)>,
    pub(crate) blend_mode: BlendMode,
    pub(crate) sample_mode: CompositeSampleMode,
}

pub(crate) trait EffectScratchTargetProvider<'target> {
    fn next(&mut self) -> Result<&'target OffscreenTarget, String>;
    fn assert_consumed(&self) -> Result<(), String>;
}

pub(crate) struct RecordedEffectScratchTargets {
    targets: Vec<RecordedEffectScratchTarget>,
}

struct RecordedEffectScratchTarget {
    descriptor: FrameTextureDescriptor,
    target: OffscreenTarget,
}

impl RecordedEffectScratchTargets {
    fn new() -> Self {
        Self {
            targets: Vec::new(),
        }
    }

    fn push(&mut self, descriptor: FrameTextureDescriptor, target: OffscreenTarget) {
        self.targets
            .push(RecordedEffectScratchTarget { descriptor, target });
    }

    pub(crate) fn release_into<C: FrameCommandRecorder>(self, recorder: &mut C) {
        for scratch in self.targets {
            recorder.release_transient_offscreen(scratch.descriptor, scratch.target);
        }
    }

    pub(crate) fn refs(&self) -> RecordedEffectScratchTargetRefs<'_> {
        RecordedEffectScratchTargetRefs {
            targets: &self.targets,
            next: 0,
        }
    }
}

pub(crate) struct RecordedEffectScratchTargetRefs<'a> {
    targets: &'a [RecordedEffectScratchTarget],
    next: usize,
}

impl<'a> EffectScratchTargetProvider<'a> for RecordedEffectScratchTargetRefs<'a> {
    fn next(&mut self) -> Result<&'a OffscreenTarget, String> {
        let index = self.next;
        let target = self
            .targets
            .get(index)
            .map(|scratch| &scratch.target)
            .ok_or_else(|| format!("render effect scratch target {index} was not acquired"))?;
        self.next += 1;
        Ok(target)
    }

    fn assert_consumed(&self) -> Result<(), String> {
        if self.next == self.targets.len() {
            Ok(())
        } else {
            Err(format!(
                "render effect acquired {} scratch targets but consumed {}",
                self.targets.len(),
                self.next
            ))
        }
    }
}

fn acquire_recorded_effect_scratch_textures_into<C: FrameCommandRecorder>(
    recorder: &mut C,
    device: &wgpu::Device,
    effect: &RenderEffect,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    targets: &mut RecordedEffectScratchTargets,
) {
    match effect {
        RenderEffect::Blur {
            radius_x, radius_y, ..
        } => {
            if *radius_x > 0.0 || *radius_y > 0.0 {
                let descriptor = FrameTextureDescriptor::render_attachment(
                    "Render Effect Blur Scratch",
                    width,
                    height,
                    format,
                );
                let target = recorder.acquire_transient_offscreen(device, descriptor);
                targets.push(descriptor, target);
            }
        }
        RenderEffect::Offset { .. } | RenderEffect::Shader { .. } => {}
        RenderEffect::Chain { first, second } => {
            let descriptor = FrameTextureDescriptor::render_attachment(
                "Render Effect Chain Scratch",
                width,
                height,
                format,
            );
            let target = recorder.acquire_transient_offscreen(device, descriptor);
            targets.push(descriptor, target);
            acquire_recorded_effect_scratch_textures_into(
                recorder, device, first, width, height, format, targets,
            );
            acquire_recorded_effect_scratch_textures_into(
                recorder, device, second, width, height, format, targets,
            );
        }
    }
}

/// Blur uniform data matching the WGSL `BlurUniforms` struct.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct BlurUniforms {
    direction_and_radius: [f32; 4],
    texture_size_and_tile_mode: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct BlurRoundedMaskUniforms {
    direction_and_radius: [f32; 4],
    texture_size_and_tile_mode: [f32; 4],
    effect_rect: [f32; 4],
    container_and_feather: [f32; 4],
    corner_radii: [f32; 4],
}

/// Offset uniform data matching the WGSL `OffsetUniforms` struct.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct OffsetUniforms {
    offset: [f32; 2],
    _padding: [f32; 2],
}

/// Blit uniform data matching the WGSL `BlitUniforms` struct.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct BlitUniforms {
    alpha: [f32; 4],
    mask_rect: [f32; 4],
    mask_radii: [f32; 4],
    mask_enabled: [f32; 4],
    sampling: [f32; 4],
    dest_viewport: [f32; 4],
    source_viewport: [f32; 4],
    resolve_span: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct ProjectiveBlitUniforms {
    viewport: [f32; 2],
    source_size: [f32; 2],
    inverse_row0: [f32; 4],
    inverse_row1: [f32; 4],
    inverse_row2: [f32; 4],
    alpha: [f32; 4],
    sampling: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct ProjectiveBlitVertex {
    position: [f32; 2],
}

fn blur_uniform_spec(horizontal: bool) -> UploadAllocatorSpec {
    if horizontal {
        UploadAllocatorSpec::uniform(
            "Blur Horizontal Uniform Buffer",
            "Blur Horizontal Uniform Bind Group",
            std::mem::size_of::<BlurUniforms>() as u64,
        )
    } else {
        UploadAllocatorSpec::uniform(
            "Blur Vertical Uniform Buffer",
            "Blur Vertical Uniform Bind Group",
            std::mem::size_of::<BlurUniforms>() as u64,
        )
    }
}

fn blur_rounded_mask_uniform_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::uniform(
        "Blur Rounded Mask Uniform Buffer",
        "Blur Rounded Mask Uniform Bind Group",
        std::mem::size_of::<BlurRoundedMaskUniforms>() as u64,
    )
}

fn offset_uniform_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::uniform(
        "Offset Uniform Buffer",
        "Offset Uniform Bind Group",
        std::mem::size_of::<OffsetUniforms>() as u64,
    )
}

fn blit_uniform_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::uniform(
        "Blit Uniform Buffer",
        "Blit Uniform Bind Group",
        std::mem::size_of::<BlitUniforms>() as u64,
    )
}

fn projective_blit_uniform_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::uniform(
        "Projective Blit Uniform Buffer",
        "Projective Blit Uniform Bind Group",
        std::mem::size_of::<ProjectiveBlitUniforms>() as u64,
    )
}

fn projective_blit_vertex_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::vertex(
        "Projective Blit Vertex Buffer",
        (std::mem::size_of::<ProjectiveBlitVertex>() * 4) as u64,
    )
}

fn effect_uniform_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::uniform(
        "Effect Uniform Buffer",
        "Effect Uniform Bind Group",
        (RuntimeShader::MAX_UNIFORMS * std::mem::size_of::<f32>()) as u64,
    )
}

/// Optional rounded-rectangle clip mask applied during fullscreen blit.
#[derive(Copy, Clone, Debug)]
pub(crate) struct RoundedCompositeMask {
    /// Clip rect in destination pixel coordinates: x, y, width, height.
    pub rect: [f32; 4],
    /// Corner radii in pixels: top_left, top_right, bottom_left, bottom_right.
    pub radii: [f32; 4],
}

#[derive(Clone, Copy)]
struct CompositePassOptions {
    alpha: f32,
    load_op: wgpu::LoadOp<wgpu::Color>,
    scissor: Option<(u32, u32, u32, u32)>,
    rounded_mask: Option<RoundedCompositeMask>,
    blend_mode: BlendMode,
    dest_viewport: Option<(f32, f32, f32, f32)>,
    source_viewport: Option<(f32, f32, f32, f32)>,
    sample_mode: CompositeSampleMode,
}

#[derive(Clone, Copy)]
struct ShaderPassOptions {
    load_op: wgpu::LoadOp<wgpu::Color>,
    scissor: Option<(u32, u32, u32, u32)>,
    dest_viewport: Option<(f32, f32, f32, f32)>,
    pipeline_mode: RuntimeShaderPipelineMode,
}

#[derive(Clone, Copy)]
pub(crate) struct ShaderCompositeBatchItem<'a> {
    pub(crate) source: &'a OffscreenTarget,
    pub(crate) shader: &'a RuntimeShader,
    pub(crate) layer_pixel_rect: [f32; 4],
    pub(crate) scissor: Option<(u32, u32, u32, u32)>,
    pub(crate) dest_viewport: (f32, f32, f32, f32),
}

#[derive(Clone, Copy)]
pub(crate) struct CompositeBatchItem<'a> {
    pub(crate) source: &'a OffscreenTarget,
    pub(crate) alpha: f32,
    pub(crate) scissor: Option<(u32, u32, u32, u32)>,
    pub(crate) rounded_mask: Option<RoundedCompositeMask>,
    pub(crate) blend_mode: BlendMode,
    pub(crate) dest_viewport: Option<(f32, f32, f32, f32)>,
    pub(crate) source_viewport: Option<(f32, f32, f32, f32)>,
    pub(crate) sample_mode: CompositeSampleMode,
}

pub(crate) struct PreparedCompositeDraw<'a> {
    texture_bind_group: &'a wgpu::BindGroup,
    uniform_bind_group: wgpu::BindGroup,
    scissor: Option<(u32, u32, u32, u32)>,
    blend_mode: BlendMode,
}

pub(crate) struct PreparedShaderDraw<'a> {
    shader: &'a RuntimeShader,
    texture_bind_group: &'a wgpu::BindGroup,
    uniform_bind_group: wgpu::BindGroup,
    scissor: Option<(u32, u32, u32, u32)>,
    dest_viewport: (f32, f32, f32, f32),
}

/// One rotated/scaled textured-quad composite for the fused pass — the
/// segment-surface cache's draw-back. `dest_quad` is in destination pixels,
/// strip order TL, TR, BL, BR; `inverse` maps destination pixels to SOURCE
/// TEXEL coordinates (not normalized).
pub(crate) struct ProjectiveCompositeItem<'a> {
    pub source: &'a OffscreenTarget,
    pub viewport: (u32, u32),
    pub dest_quad: [[f32; 2]; 4],
    pub inverse: [[f32; 3]; 3],
    pub alpha: f32,
    pub blend_mode: BlendMode,
    pub sample_mode: CompositeSampleMode,
}

pub(crate) struct PreparedProjectiveComposite<'a> {
    texture_bind_group: &'a wgpu::BindGroup,
    uniform_bind_group: wgpu::BindGroup,
    vertex_buffer: wgpu::Buffer,
    blend_mode: BlendMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompositeSampleMode {
    Linear,
    Box4,
    Nearest,
}

fn dst_out_blend_state() -> wgpu::BlendState {
    let component = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    };
    wgpu::BlendState {
        color: component,
        alpha: component,
    }
}

#[allow(clippy::too_many_arguments)]
fn create_fullscreen_pipeline(
    device: &wgpu::Device,
    label: &'static str,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    fragment_entry: &'static str,
    surface_format: wgpu::TextureFormat,
    blend: wgpu::BlendState,
    depth: bool,
) -> wgpu::RenderPipeline {
    crate::render::create_render_pipeline_logged(
        device,
        &format!("effect {label} entry={fragment_entry} depth={depth}"),
        &wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("fullscreen_vs"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some(fragment_entry),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: display_clip::content_depth_state(depth),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        },
    )
}

fn create_projective_pipeline(
    device: &wgpu::Device,
    label: &'static str,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    surface_format: wgpu::TextureFormat,
    blend: wgpu::BlendState,
    depth: bool,
) -> wgpu::RenderPipeline {
    crate::render::create_render_pipeline_logged(
        device,
        &format!("effect {label} depth={depth}"),
        &wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("projective_blit_vs"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<ProjectiveBlitVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        offset: 0,
                        shader_location: 0,
                        format: wgpu::VertexFormat::Float32x2,
                    }],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("projective_blit_fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: display_clip::content_depth_state(depth),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        },
    )
}

impl EffectRenderer {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        adapter_backend: wgpu::Backend,
    ) -> Self {
        // Create shared bind group layouts
        let effect_texture_bind_group_layout = OffscreenPool::texture_bind_group_layout(device);
        let effect_uniform_bind_group_layout = OffscreenPool::uniform_bind_group_layout(device);

        // Blur-specific uniform layout
        let blur_uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Blur Uniform Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        // Offset-specific uniform layout
        let offset_uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Offset Uniform Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        // Blit-specific uniform layout
        let blit_uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Blit Uniform Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        // Compile blur pipeline
        let blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Blur Shader"),
            source: wgpu::ShaderSource::Wgsl(shaders::blur_shader().into()),
        });

        let blur_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Blur Pipeline Layout"),
            bind_group_layouts: &[
                Some(&effect_texture_bind_group_layout),
                Some(&blur_uniform_bind_group_layout),
            ],
            immediate_size: 0,
        });

        let blur_pipeline = LazyGpuResource::new("effect/blur");

        let blur_rounded_mask_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Blur Rounded Mask Shader"),
            source: wgpu::ShaderSource::Wgsl(shaders::blur_rounded_mask_shader().into()),
        });
        let blur_rounded_mask_pipeline = LazyGpuResource::new("effect/blur-rounded-mask");

        // Compile offset pipeline
        let offset_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Offset Shader"),
            source: wgpu::ShaderSource::Wgsl(shaders::offset_shader().into()),
        });

        let offset_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Offset Pipeline Layout"),
                bind_group_layouts: &[
                    Some(&effect_texture_bind_group_layout),
                    Some(&offset_uniform_bind_group_layout),
                ],
                immediate_size: 0,
            });

        let offset_pipeline = LazyGpuResource::new("effect/offset");

        // Compile blit pipeline
        let blit_shader_source = shaders::blit_shader();
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Blit Shader"),
            source: wgpu::ShaderSource::Wgsl(blit_shader_source.clone().into()),
        });

        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Blit Pipeline Layout"),
            bind_group_layouts: &[
                Some(&effect_texture_bind_group_layout),
                Some(&blit_uniform_bind_group_layout),
            ],
            immediate_size: 0,
        });

        let blit_pipeline = PassPipeline::new("effect/blit-src-over", "effect/blit-src-over-depth");
        let blit_pipeline_dst_out =
            PassPipeline::new("effect/blit-dst-out", "effect/blit-dst-out-depth");

        let projective_blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Projective Blit Shader"),
            source: wgpu::ShaderSource::Wgsl(shaders::projective_blit_shader().into()),
        });
        let projective_blit_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Projective Blit Pipeline Layout"),
                bind_group_layouts: &[
                    Some(&effect_texture_bind_group_layout),
                    Some(&blit_uniform_bind_group_layout),
                ],
                immediate_size: 0,
            });
        let projective_blit_pipeline = PassPipeline::new(
            "effect/projective-src-over",
            "effect/projective-src-over-depth",
        );
        let projective_blit_pipeline_dst_out = PassPipeline::new(
            "effect/projective-dst-out",
            "effect/projective-dst-out-depth",
        );

        let effect_linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Effect Linear Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            offscreen_pool: OffscreenPool::new(device, surface_format),
            shader_cache: ShaderPipelineCache::new(adapter_backend),
            blur_shader,
            blur_rounded_mask_shader,
            blur_pipeline_layout,
            blur_pipeline,
            blur_rounded_mask_pipeline,
            blur_uniform_bind_group_layout,
            offset_shader,
            offset_pipeline_layout,
            offset_pipeline,
            offset_uniform_bind_group_layout,
            blit_shader,
            blit_shader_source,
            blit_pipeline_layout,
            blit_pipeline,
            blit_pipeline_dst_out,
            blit_uniform_bind_group_layout,
            projective_blit_shader,
            projective_blit_pipeline_layout,
            projective_blit_pipeline,
            projective_blit_pipeline_dst_out,
            effect_texture_bind_group_layout,
            effect_uniform_bind_group_layout,
            effect_linear_sampler,
            surface_format,
            adapter_backend,
            debug_command_stats: Cell::new(FrameCommandStats::default()),
            debug_blurs: Cell::new(0),
            debug_composites: Cell::new(0),
            debug_effects: Cell::new(0),
            debug_upload_bytes: Cell::new(0),
            #[cfg(not(target_arch = "wasm32"))]
            debug_fill_composite_px2: Cell::new(0.0),
            #[cfg(not(target_arch = "wasm32"))]
            debug_fill_offscreen_px2: Cell::new(0.0),
        }
    }

    /// One draw into a caller-supplied view (composite/blit/src-over shader):
    /// counts its target-area fill for `CRANPOSE_FILL_DIAG`. No-op while the
    /// diagnostic is off.
    fn note_composite_fill(
        &self,
        scissor: Option<(u32, u32, u32, u32)>,
        dest_viewport: Option<(f32, f32, f32, f32)>,
        fallback: (u32, u32),
    ) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if crate::render::fill_area_diag_enabled() {
                let area = effect_pass_fill_px2(scissor, dest_viewport, fallback);
                self.debug_fill_composite_px2
                    .set(self.debug_fill_composite_px2.get() + area);
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (scissor, dest_viewport, fallback);
    }

    /// One pass into an offscreen chain texture (blur axis, offset,
    /// replace-mode shader): counts its target-area fill for
    /// `CRANPOSE_FILL_DIAG`. No-op while the diagnostic is off.
    fn note_offscreen_fill(
        &self,
        scissor: Option<(u32, u32, u32, u32)>,
        dest_viewport: Option<(f32, f32, f32, f32)>,
        fallback: (u32, u32),
    ) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if crate::render::fill_area_diag_enabled() {
                let area = effect_pass_fill_px2(scissor, dest_viewport, fallback);
                self.debug_fill_offscreen_px2
                    .set(self.debug_fill_offscreen_px2.get() + area);
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (scissor, dest_viewport, fallback);
    }

    /// Drains the frame's accumulated effect fill: (composite px²,
    /// offscreen px²). Called once per frame by the renderer while
    /// `CRANPOSE_FILL_DIAG` is on.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn take_fill_diag_fill_px2(&self) -> (f64, f64) {
        (
            self.debug_fill_composite_px2.take(),
            self.debug_fill_offscreen_px2.take(),
        )
    }

    fn blur_pipeline(&self, device: &wgpu::Device) -> &wgpu::RenderPipeline {
        self.blur_pipeline.get_or_init(self.adapter_backend, || {
            create_fullscreen_pipeline(
                device,
                "Blur Pipeline",
                &self.blur_pipeline_layout,
                &self.blur_shader,
                "blur_fs",
                self.surface_format,
                wgpu::BlendState::REPLACE,
                false,
            )
        })
    }

    fn blur_rounded_mask_pipeline(&self, device: &wgpu::Device) -> &wgpu::RenderPipeline {
        self.blur_rounded_mask_pipeline
            .get_or_init(self.adapter_backend, || {
                create_fullscreen_pipeline(
                    device,
                    "Blur Rounded Mask Pipeline",
                    &self.blur_pipeline_layout,
                    &self.blur_rounded_mask_shader,
                    "blur_rounded_mask_fs",
                    self.surface_format,
                    wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
                    false,
                )
            })
    }

    fn offset_pipeline(&self, device: &wgpu::Device) -> &wgpu::RenderPipeline {
        self.offset_pipeline.get_or_init(self.adapter_backend, || {
            create_fullscreen_pipeline(
                device,
                "Offset Pipeline",
                &self.offset_pipeline_layout,
                &self.offset_shader,
                "offset_fs",
                self.surface_format,
                wgpu::BlendState::REPLACE,
                false,
            )
        })
    }

    /// The composite blit in its two pass flavors: `depth` is true exactly
    /// when the caller is encoding the display-clip culled fused pass,
    /// whose variant compiles the blit source with the content z rewritten
    /// and depth-tests against the visible-region occluder.
    fn blit_pipeline(
        &self,
        device: &wgpu::Device,
        blend_mode: BlendMode,
        depth: bool,
    ) -> &wgpu::RenderPipeline {
        let (resource, label, blend) = match blend_mode {
            BlendMode::DstOut => (
                &self.blit_pipeline_dst_out,
                "Blit Pipeline DstOut",
                dst_out_blend_state(),
            ),
            _ => (
                &self.blit_pipeline,
                "Blit Pipeline",
                wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
            ),
        };
        resource.get_or_init(self.adapter_backend, depth, |depth| {
            let depth_shader = depth.then(|| {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Blit Shader (display clip)"),
                    source: wgpu::ShaderSource::Wgsl(display_clip::with_content_z(
                        self.blit_shader_source.clone().into(),
                        true,
                    )),
                })
            });
            create_fullscreen_pipeline(
                device,
                label,
                &self.blit_pipeline_layout,
                depth_shader.as_ref().unwrap_or(&self.blit_shader),
                "blit_fs",
                self.surface_format,
                blend,
                depth,
            )
        })
    }

    fn initialized_blit_pipeline(
        &self,
        blend_mode: BlendMode,
        depth: bool,
    ) -> &wgpu::RenderPipeline {
        let resource = match blend_mode {
            BlendMode::DstOut => &self.blit_pipeline_dst_out,
            _ => &self.blit_pipeline,
        };
        resource
            .get(depth)
            .expect("prepared composite must initialize its blit pipeline")
    }

    /// The projective blit in its two pass flavors, mirroring
    /// [`Self::blit_pipeline`]: `depth` is true exactly when the caller is
    /// encoding into the display-clip culled fused pass, whose variant
    /// compiles the shader with the content z rewritten and depth-tests
    /// against the visible-region occluder.
    fn projective_blit_pipeline(
        &self,
        device: &wgpu::Device,
        blend_mode: BlendMode,
        depth: bool,
    ) -> &wgpu::RenderPipeline {
        let (resource, label, blend) = match blend_mode {
            BlendMode::DstOut => (
                &self.projective_blit_pipeline_dst_out,
                "Projective Blit Pipeline DstOut",
                dst_out_blend_state(),
            ),
            _ => (
                &self.projective_blit_pipeline,
                "Projective Blit Pipeline",
                wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
            ),
        };
        resource.get_or_init(self.adapter_backend, depth, |depth| {
            let depth_shader = depth.then(|| {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Projective Blit Shader (display clip)"),
                    source: wgpu::ShaderSource::Wgsl(display_clip::with_content_z(
                        shaders::projective_blit_shader().into(),
                        true,
                    )),
                })
            });
            create_projective_pipeline(
                device,
                label,
                &self.projective_blit_pipeline_layout,
                depth_shader
                    .as_ref()
                    .unwrap_or(&self.projective_blit_shader),
                self.surface_format,
                blend,
                depth,
            )
        })
    }

    fn initialized_projective_blit_pipeline(
        &self,
        blend_mode: BlendMode,
        depth: bool,
    ) -> &wgpu::RenderPipeline {
        let resource = match blend_mode {
            BlendMode::DstOut => &self.projective_blit_pipeline_dst_out,
            _ => &self.projective_blit_pipeline,
        };
        resource
            .get(depth)
            .expect("prepared projective composite must initialize its pipeline")
    }

    fn sampler_for_mode(&self, _sample_mode: CompositeSampleMode) -> &wgpu::Sampler {
        &self.effect_linear_sampler
    }

    pub(crate) fn max_texture_dim(&self) -> u32 {
        self.offscreen_pool.max_texture_dim()
    }

    pub(crate) fn acquire_offscreen(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        stats: Option<&FrameStats>,
    ) -> OffscreenTarget {
        self.offscreen_pool.acquire(device, width, height, stats)
    }

    pub(crate) fn release_offscreen(&mut self, target: OffscreenTarget) {
        self.offscreen_pool.release(target);
    }

    pub(crate) fn retained_offscreen_count(&self) -> usize {
        self.offscreen_pool.pool_size()
    }

    pub(crate) fn retained_offscreen_bytes(&self) -> usize {
        self.offscreen_pool.estimated_bytes()
    }

    pub(crate) fn merge_and_reset_debug_counters(&mut self, stats: &FrameStats) {
        stats.record_command_stats(self.debug_command_stats.get());
        stats
            .blur_passes
            .set(stats.blur_passes.get() + self.debug_blurs.get());
        stats
            .composite_passes
            .set(stats.composite_passes.get() + self.debug_composites.get());
        stats
            .effect_applies
            .set(stats.effect_applies.get() + self.debug_effects.get());
        stats.record_upload_bytes(self.debug_upload_bytes.get());
        self.debug_command_stats.set(FrameCommandStats::default());
        self.debug_blurs.set(0);
        self.debug_composites.set(0);
        self.debug_effects.set(0);
        self.debug_upload_bytes.set(0);
    }

    pub(crate) fn record_blur_pass(&self) {
        self.debug_blurs.set(self.debug_blurs.get() + 1);
    }

    pub(crate) fn record_composite_pass(&self) {
        self.debug_composites.set(self.debug_composites.get() + 1);
    }

    pub(crate) fn acquire_recorded_effect_scratch_targets<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        effect: &RenderEffect,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> RecordedEffectScratchTargets {
        let mut targets = RecordedEffectScratchTargets::new();
        acquire_recorded_effect_scratch_textures_into(
            recorder,
            device,
            effect,
            width,
            height,
            format,
            &mut targets,
        );
        targets
    }

    /// Encode a two-pass separable Gaussian blur using one caller-owned scratch
    /// target. The horizontal pass writes source -> scratch, and the vertical
    /// pass writes scratch -> dest_view.
    #[allow(clippy::too_many_arguments)]
    fn encode_blur_axis_pass<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        uniforms: BlurUniforms,
        horizontal: bool,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
    ) {
        let source_bind_group = source.get_or_create_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            &self.effect_linear_sampler,
        );

        let uniform_bind_group = recorder.upload_uniform(
            if horizontal {
                UploadAllocatorId::BlurHorizontal
            } else {
                UploadAllocatorId::BlurVertical
            },
            blur_uniform_spec(horizontal),
            device,
            &self.blur_uniform_bind_group_layout,
            bytemuck::bytes_of(&uniforms),
            &self.debug_upload_bytes,
        );
        // Blur axis passes ping-pong through chain textures sized like the
        // source (asserted by the ping-pong caller).
        self.note_offscreen_fill(scissor, None, (source.width, source.height));
        let mut pass = recorder
            .encoder()
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(if horizontal {
                    "Blur Horizontal Pass"
                } else {
                    "Blur Vertical Pass"
                }),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dest_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
        pass.set_pipeline(self.blur_pipeline(device));
        pass.set_bind_group(0, source_bind_group, &[]);
        pass.set_bind_group(1, &uniform_bind_group, &[]);
        if let Some((x, y, w, h)) = scissor {
            pass.set_scissor_rect(x, y, w, h);
        }
        pass.draw(0..4, 0..1);
    }

    fn blur_uniforms(
        horizontal: bool,
        width: u32,
        height: u32,
        radius_x: f32,
        radius_y: f32,
        tile_mode: TileMode,
    ) -> BlurUniforms {
        let direction = if horizontal { [1.0, 0.0] } else { [0.0, 1.0] };
        BlurUniforms {
            direction_and_radius: [direction[0], direction[1], radius_x, radius_y],
            texture_size_and_tile_mode: [
                width as f32,
                height as f32,
                tile_mode_uniform_value(tile_mode),
                0.0,
            ],
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn encode_blur_scissored_ping_pong_passes<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        source: &OffscreenTarget,
        scratch: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        radius_x: f32,
        radius_y: f32,
        tile_mode: TileMode,
        scissor: Option<(u32, u32, u32, u32)>,
    ) {
        debug_assert!(
            radius_x > 0.0 || radius_y > 0.0,
            "zero-radius blur should use the composite fast path"
        );
        debug_assert_eq!(scratch.width, source.width);
        debug_assert_eq!(scratch.height, source.height);

        self.encode_blur_axis_pass(
            recorder,
            device,
            source,
            &scratch.view,
            Self::blur_uniforms(
                true,
                source.width,
                source.height,
                radius_x,
                radius_y,
                tile_mode,
            ),
            true,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            scissor,
        );
        self.encode_blur_axis_pass(
            recorder,
            device,
            scratch,
            dest_view,
            Self::blur_uniforms(
                false,
                source.width,
                source.height,
                radius_x,
                radius_y,
                tile_mode,
            ),
            false,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            scissor,
        );
    }

    fn rounded_mask_uniforms(
        shader: &RuntimeShader,
        layer_pixel_rect: [f32; 4],
        width: u32,
        height: u32,
        radius_x: f32,
        radius_y: f32,
        tile_mode: TileMode,
    ) -> Option<BlurRoundedMaskUniforms> {
        if shader.source() != ROUNDED_ALPHA_MASK_WGSL {
            return None;
        }
        let uniforms = shader.uniforms();
        let get = |index: usize| uniforms.get(index).copied().unwrap_or(0.0);
        Some(BlurRoundedMaskUniforms {
            direction_and_radius: [0.0, 1.0, radius_x, radius_y],
            texture_size_and_tile_mode: [
                width as f32,
                height as f32,
                tile_mode_uniform_value(tile_mode),
                0.0,
            ],
            effect_rect: layer_pixel_rect,
            container_and_feather: [get(0).max(1.0), get(1).max(1.0), get(2).max(0.0), 0.0],
            corner_radii: [
                get(3).max(0.0),
                get(4).max(0.0),
                get(5).max(0.0),
                get(6).max(0.0),
            ],
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn encode_blur_then_rounded_mask_src_over_to_view<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        source: &OffscreenTarget,
        scratch: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        radius_x: f32,
        radius_y: f32,
        tile_mode: TileMode,
        shader: &RuntimeShader,
        layer_pixel_rect: [f32; 4],
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        dest_viewport: (f32, f32, f32, f32),
    ) -> bool {
        if dest_viewport.2 <= 0.0 || dest_viewport.3 <= 0.0 {
            return false;
        }
        let Some(mask_uniforms) = Self::rounded_mask_uniforms(
            shader,
            layer_pixel_rect,
            source.width,
            source.height,
            radius_x,
            radius_y,
            tile_mode,
        ) else {
            return false;
        };
        debug_assert_eq!(scratch.width, source.width);
        debug_assert_eq!(scratch.height, source.height);

        self.encode_blur_axis_pass(
            recorder,
            device,
            source,
            &scratch.view,
            Self::blur_uniforms(
                true,
                source.width,
                source.height,
                radius_x,
                radius_y,
                tile_mode,
            ),
            true,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            None,
        );

        let scratch_bind_group = scratch.get_or_create_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            &self.effect_linear_sampler,
        );
        let uniform_bind_group = recorder.upload_uniform(
            UploadAllocatorId::BlurRoundedMask,
            blur_rounded_mask_uniform_spec(),
            device,
            &self.blur_uniform_bind_group_layout,
            bytemuck::bytes_of(&mask_uniforms),
            &self.debug_upload_bytes,
        );
        self.note_composite_fill(scissor, Some(dest_viewport), (source.width, source.height));
        let mut pass = recorder
            .encoder()
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Blur Rounded Mask Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dest_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
        pass.set_pipeline(self.blur_rounded_mask_pipeline(device));
        pass.set_bind_group(0, scratch_bind_group, &[]);
        pass.set_bind_group(1, &uniform_bind_group, &[]);
        let (x, y, width, height) = dest_viewport;
        pass.set_viewport(x, y, width, height, 0.0, 1.0);
        if let Some((x, y, w, h)) = scissor {
            pass.set_scissor_rect(x, y, w, h);
        }
        pass.draw(0..4, 0..1);
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn encode_offset<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        offset_x: f32,
        offset_y: f32,
    ) {
        let uniforms = OffsetUniforms {
            offset: [offset_x, offset_y],
            _padding: [0.0; 2],
        };
        let uniform_bind_group = recorder.upload_uniform(
            UploadAllocatorId::Offset,
            offset_uniform_spec(),
            device,
            &self.offset_uniform_bind_group_layout,
            bytemuck::bytes_of(&uniforms),
            &self.debug_upload_bytes,
        );

        let texture_bind_group = source.get_or_create_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            &self.effect_linear_sampler,
        );
        self.note_offscreen_fill(None, None, (source.width, source.height));

        let mut pass = recorder
            .encoder()
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Offset Effect Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dest_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

        pass.set_pipeline(self.offset_pipeline(device));
        pass.set_bind_group(0, texture_bind_group, &[]);
        pass.set_bind_group(1, &uniform_bind_group, &[]);
        pass.draw(0..4, 0..1);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn encode_shader<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        shader: &RuntimeShader,
        layer_pixel_rect: [f32; 4],
    ) -> bool {
        self.encode_shader_pass(
            recorder,
            device,
            source,
            dest_view,
            shader,
            layer_pixel_rect,
            ShaderPassOptions {
                load_op: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                scissor: None,
                dest_viewport: None,
                pipeline_mode: RuntimeShaderPipelineMode::Replace,
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn encode_shader_src_over_to_view<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        shader: &RuntimeShader,
        layer_pixel_rect: [f32; 4],
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        dest_viewport: (f32, f32, f32, f32),
    ) -> bool {
        if dest_viewport.2 <= 0.0 || dest_viewport.3 <= 0.0 {
            return false;
        }
        self.encode_shader_pass(
            recorder,
            device,
            source,
            dest_view,
            shader,
            layer_pixel_rect,
            ShaderPassOptions {
                load_op,
                scissor,
                dest_viewport: Some(dest_viewport),
                pipeline_mode: RuntimeShaderPipelineMode::PremultipliedSrcOver,
            },
        )
    }

    pub(crate) fn encode_shader_batch_src_over_to_view<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        dest_view: &wgpu::TextureView,
        viewport: (u32, u32),
        load_op: wgpu::LoadOp<wgpu::Color>,
        items: &[ShaderCompositeBatchItem<'_>],
    ) -> bool {
        if items.is_empty() {
            return true;
        }
        let Some(prepared) = self.prepare_shader_batch_draws(recorder, device, items, false) else {
            return false;
        };

        let mut pass = recorder
            .encoder()
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Batched Shader Effect Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dest_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

        for draw in &prepared {
            self.draw_prepared_shader_src_over(device, &mut pass, viewport, draw, false);
        }

        true
    }

    /// `pass_depth` names the pass the prepared draws will execute in: true
    /// exactly for the display-clip culled fused pass, whose shader
    /// pipelines carry the depth state (see [`ShaderPipelineCache`]).
    pub(crate) fn prepare_shader_batch_draws<'a, C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        items: &'a [ShaderCompositeBatchItem<'a>],
        pass_depth: bool,
    ) -> Option<Vec<PreparedShaderDraw<'a>>> {
        let mut prepared = Vec::with_capacity(items.len());
        for item in items {
            self.shader_cache.get_or_create(
                device,
                item.shader,
                self.surface_format,
                &self.effect_texture_bind_group_layout,
                &self.effect_uniform_bind_group_layout,
                RuntimeShaderPipelineMode::PremultipliedSrcOver,
                pass_depth,
            )?;
            let mut padded = item.shader.uniforms_padded();
            let slot = RuntimeShader::RESERVED_UNIFORM_START;
            padded[slot] = item.layer_pixel_rect[0];
            padded[slot + 1] = item.layer_pixel_rect[1];
            padded[slot + 2] = item.layer_pixel_rect[2];
            padded[slot + 3] = item.layer_pixel_rect[3];
            let uniform_bind_group = recorder.upload_uniform(
                UploadAllocatorId::EffectUniform,
                effect_uniform_spec(),
                device,
                &self.effect_uniform_bind_group_layout,
                bytemuck::cast_slice(&padded),
                &self.debug_upload_bytes,
            );

            let texture_bind_group = item.source.get_or_create_bind_group(
                device,
                &self.effect_texture_bind_group_layout,
                &self.effect_linear_sampler,
            );
            // Prepared draws execute exactly once each (fused batch arms);
            // counting here covers every batched shader composite.
            self.note_composite_fill(
                item.scissor,
                Some(item.dest_viewport),
                (item.source.width, item.source.height),
            );
            prepared.push(PreparedShaderDraw {
                shader: item.shader,
                texture_bind_group,
                uniform_bind_group,
                scissor: item.scissor,
                dest_viewport: item.dest_viewport,
            });
        }
        Some(prepared)
    }

    pub(crate) fn draw_prepared_shader_src_over(
        &mut self,
        device: &wgpu::Device,
        pass: &mut wgpu::RenderPass<'_>,
        viewport: (u32, u32),
        draw: &PreparedShaderDraw<'_>,
        pass_depth: bool,
    ) {
        let pipeline = self
            .shader_cache
            .get_or_create(
                device,
                draw.shader,
                self.surface_format,
                &self.effect_texture_bind_group_layout,
                &self.effect_uniform_bind_group_layout,
                RuntimeShaderPipelineMode::PremultipliedSrcOver,
                pass_depth,
            )
            .expect("shader batch pipeline was prevalidated");
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, draw.texture_bind_group, &[]);
        pass.set_bind_group(1, &draw.uniform_bind_group, &[]);
        let (x, y, width, height) = draw.dest_viewport;
        pass.set_viewport(x, y, width, height, 0.0, 1.0);
        if let Some((x, y, width, height)) = draw.scissor {
            pass.set_scissor_rect(x, y, width, height);
        } else {
            pass.set_scissor_rect(0, 0, viewport.0, viewport.1);
        }
        pass.draw(0..4, 0..1);
        // This pass is caller-owned and shared with subsequent fused batches
        // (text, shapes, more composites): restore the full-target viewport
        // and scissor, or every later draw inherits this composite's
        // sub-viewport transform and lands squeezed inside it.
        pass.set_viewport(0.0, 0.0, viewport.0 as f32, viewport.1 as f32, 0.0, 1.0);
        pass.set_scissor_rect(0, 0, viewport.0, viewport.1);
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_shader_pass<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        shader: &RuntimeShader,
        layer_pixel_rect: [f32; 4],
        options: ShaderPassOptions,
    ) -> bool {
        let mut padded = shader.uniforms_padded();
        let slot = RuntimeShader::RESERVED_UNIFORM_START;
        padded[slot] = layer_pixel_rect[0];
        padded[slot + 1] = layer_pixel_rect[1];
        padded[slot + 2] = layer_pixel_rect[2];
        padded[slot + 3] = layer_pixel_rect[3];
        let uniform_bind_group = recorder.upload_uniform(
            UploadAllocatorId::EffectUniform,
            effect_uniform_spec(),
            device,
            &self.effect_uniform_bind_group_layout,
            bytemuck::cast_slice(&padded),
            &self.debug_upload_bytes,
        );

        // Validate the pipeline first, then account, then fetch it for the
        // draw — the fetch below is a cache hit, and a shader that fails to
        // compile must not count fill (the caller composites instead, which
        // counts its own pass).
        if self
            .shader_cache
            .get_or_create(
                device,
                shader,
                self.surface_format,
                &self.effect_texture_bind_group_layout,
                &self.effect_uniform_bind_group_layout,
                options.pipeline_mode,
                false,
            )
            .is_none()
        {
            return false;
        }
        // Replace-mode shader passes write chain textures; src-over ones
        // composite into the caller's view.
        match options.pipeline_mode {
            RuntimeShaderPipelineMode::Replace => self.note_offscreen_fill(
                options.scissor,
                options.dest_viewport,
                (source.width, source.height),
            ),
            RuntimeShaderPipelineMode::PremultipliedSrcOver => self.note_composite_fill(
                options.scissor,
                options.dest_viewport,
                (source.width, source.height),
            ),
        }
        let Some(pipeline) = self.shader_cache.get_or_create(
            device,
            shader,
            self.surface_format,
            &self.effect_texture_bind_group_layout,
            &self.effect_uniform_bind_group_layout,
            options.pipeline_mode,
            false,
        ) else {
            return false;
        };

        let texture_bind_group = source.get_or_create_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            &self.effect_linear_sampler,
        );

        let mut pass = recorder
            .encoder()
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Shader Effect Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dest_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: options.load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, texture_bind_group, &[]);
        pass.set_bind_group(1, &uniform_bind_group, &[]);
        if let Some((x, y, width, height)) = options.dest_viewport {
            pass.set_viewport(x, y, width, height, 0.0, 1.0);
        }
        if let Some((x, y, width, height)) = options.scissor {
            pass.set_scissor_rect(x, y, width, height);
        }
        pass.draw(0..4, 0..1);
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn encode_effect<'scratch, C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        effect: &RenderEffect,
        layer_pixel_rect: [f32; 4],
        scratch_targets: &mut impl EffectScratchTargetProvider<'scratch>,
    ) -> Result<u32, String> {
        match effect {
            RenderEffect::Blur {
                radius_x,
                radius_y,
                edge_treatment,
            } => {
                if *radius_x <= 0.0 && *radius_y <= 0.0 {
                    self.encode_composite_to_view_pass(
                        recorder,
                        device,
                        source,
                        dest_view,
                        CompositePassOptions {
                            alpha: 1.0,
                            load_op: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            scissor: None,
                            rounded_mask: None,
                            blend_mode: BlendMode::SrcOver,
                            dest_viewport: None,
                            source_viewport: None,
                            sample_mode: CompositeSampleMode::Linear,
                        },
                    );
                    self.record_composite_pass();
                    return Ok(1);
                }

                let intermediate = scratch_targets.next()?;
                self.encode_blur_scissored_ping_pong_passes(
                    recorder,
                    device,
                    source,
                    intermediate,
                    dest_view,
                    *radius_x,
                    *radius_y,
                    *edge_treatment,
                    None,
                );
                self.record_blur_pass();
                Ok(2)
            }
            RenderEffect::Offset { offset_x, offset_y } => {
                self.encode_offset(recorder, device, source, dest_view, *offset_x, *offset_y);
                self.debug_effects.set(self.debug_effects.get() + 1);
                Ok(1)
            }
            RenderEffect::Shader { shader } => {
                if self.encode_shader(
                    recorder,
                    device,
                    source,
                    dest_view,
                    shader,
                    layer_pixel_rect,
                ) {
                    self.debug_effects.set(self.debug_effects.get() + 1);
                    Ok(1)
                } else {
                    self.encode_composite_to_view_pass(
                        recorder,
                        device,
                        source,
                        dest_view,
                        CompositePassOptions {
                            alpha: 1.0,
                            load_op: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            scissor: None,
                            rounded_mask: None,
                            blend_mode: BlendMode::SrcOver,
                            dest_viewport: None,
                            source_viewport: None,
                            sample_mode: CompositeSampleMode::Linear,
                        },
                    );
                    self.record_composite_pass();
                    Ok(1)
                }
            }
            RenderEffect::Chain { first, second } => {
                let intermediate = scratch_targets.next()?;
                let first_passes = self.encode_effect(
                    recorder,
                    device,
                    source,
                    &intermediate.view,
                    first,
                    layer_pixel_rect,
                    scratch_targets,
                )?;
                let second_passes = self.encode_effect(
                    recorder,
                    device,
                    intermediate,
                    dest_view,
                    second,
                    layer_pixel_rect,
                    scratch_targets,
                )?;
                Ok(first_passes.saturating_add(second_passes))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn composite_pass_uniforms(
        source: &OffscreenTarget,
        options: CompositePassOptions,
    ) -> BlitUniforms {
        let (mask_rect, mask_radii, mask_enabled) = if let Some(mask) = options.rounded_mask {
            (mask.rect, mask.radii, [1.0, 0.0, 0.0, 0.0])
        } else {
            (
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0],
            )
        };
        let dest_viewport_uniform = options.dest_viewport.unwrap_or((0.0, 0.0, 0.0, 0.0));
        let source_viewport_uniform = options.source_viewport.unwrap_or((0.0, 0.0, 0.0, 0.0));
        let source_span = options
            .source_viewport
            .filter(|(_, _, width, height)| *width > 0.0 && *height > 0.0)
            .map(|(_, _, width, height)| (width, height))
            .unwrap_or((source.width as f32, source.height as f32));
        let resolve_span = options
            .dest_viewport
            .filter(|(_, _, width, height)| *width > 0.0 && *height > 0.0)
            .map(|(_, _, width, height)| (source_span.0 / width, source_span.1 / height))
            .unwrap_or((0.0, 0.0));
        BlitUniforms {
            alpha: [options.alpha.clamp(0.0, 1.0), 0.0, 0.0, 0.0],
            mask_rect,
            mask_radii,
            mask_enabled,
            sampling: [
                composite_sampling_mode_value(options.sample_mode),
                0.0,
                0.0,
                0.0,
            ],
            dest_viewport: [
                dest_viewport_uniform.0,
                dest_viewport_uniform.1,
                dest_viewport_uniform.2,
                dest_viewport_uniform.3,
            ],
            source_viewport: [
                source_viewport_uniform.0,
                source_viewport_uniform.1,
                source_viewport_uniform.2,
                source_viewport_uniform.3,
            ],
            resolve_span: [resolve_span.0, resolve_span.1, 0.0, 0.0],
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_composite_to_view_pass<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        options: CompositePassOptions,
    ) {
        let uniforms = Self::composite_pass_uniforms(source, options);
        let sampler = self.sampler_for_mode(options.sample_mode);
        let texture_bind_group = source.get_or_create_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            sampler,
        );
        let uniform_bind_group = recorder.upload_uniform(
            UploadAllocatorId::Blit,
            blit_uniform_spec(),
            device,
            &self.blit_uniform_bind_group_layout,
            bytemuck::bytes_of(&uniforms),
            &self.debug_upload_bytes,
        );
        self.note_composite_fill(
            options.scissor,
            options.dest_viewport,
            (source.width, source.height),
        );

        let mut pass = recorder
            .encoder()
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Blit Composite Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dest_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: options.load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

        pass.set_pipeline(self.blit_pipeline(device, options.blend_mode, false));
        pass.set_bind_group(0, texture_bind_group, &[]);
        pass.set_bind_group(1, &uniform_bind_group, &[]);
        if let Some((x, y, w, h)) = options.scissor {
            pass.set_scissor_rect(x, y, w, h);
        }
        pass.draw(0..4, 0..1);
    }

    pub(crate) fn encode_composite_batch_to_view_pass<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        dest_view: &wgpu::TextureView,
        viewport: (u32, u32),
        load_op: wgpu::LoadOp<wgpu::Color>,
        items: &[CompositeBatchItem<'_>],
    ) {
        if items.is_empty() {
            return;
        }

        let prepared = self.prepare_composite_batch_draws(recorder, device, load_op, items, false);

        let mut pass = recorder
            .encoder()
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Batched Blit Composite Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dest_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

        for draw in &prepared {
            self.draw_prepared_composite(&mut pass, viewport, draw, false);
        }
    }

    /// `pass_depth` names the pass the prepared draws will execute in: true
    /// exactly for the display-clip culled fused pass, so the right blit
    /// variant is initialized here (the draw arm fetches it without a
    /// device).
    pub(crate) fn prepare_composite_batch_draws<'a, C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        load_op: wgpu::LoadOp<wgpu::Color>,
        items: &[CompositeBatchItem<'a>],
        pass_depth: bool,
    ) -> Vec<PreparedCompositeDraw<'a>> {
        let mut prepared = Vec::with_capacity(items.len());
        for item in items {
            self.blit_pipeline(device, item.blend_mode, pass_depth);
            let options = CompositePassOptions {
                alpha: item.alpha,
                load_op,
                scissor: item.scissor,
                rounded_mask: item.rounded_mask,
                blend_mode: item.blend_mode,
                dest_viewport: item.dest_viewport,
                source_viewport: item.source_viewport,
                sample_mode: item.sample_mode,
            };
            let uniforms = Self::composite_pass_uniforms(item.source, options);
            let sampler = self.sampler_for_mode(item.sample_mode);
            let texture_bind_group = item.source.get_or_create_bind_group(
                device,
                &self.effect_texture_bind_group_layout,
                sampler,
            );
            let uniform_bind_group = recorder.upload_uniform(
                UploadAllocatorId::Blit,
                blit_uniform_spec(),
                device,
                &self.blit_uniform_bind_group_layout,
                bytemuck::bytes_of(&uniforms),
                &self.debug_upload_bytes,
            );
            // Prepared draws execute exactly once each (fused batch arms);
            // counting here covers every batched composite.
            self.note_composite_fill(
                item.scissor,
                item.dest_viewport,
                (item.source.width, item.source.height),
            );
            prepared.push(PreparedCompositeDraw {
                texture_bind_group,
                uniform_bind_group,
                scissor: item.scissor,
                blend_mode: item.blend_mode,
            });
        }
        prepared
    }

    pub(crate) fn draw_prepared_composite(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        viewport: (u32, u32),
        draw: &PreparedCompositeDraw<'_>,
        pass_depth: bool,
    ) {
        pass.set_pipeline(self.initialized_blit_pipeline(draw.blend_mode, pass_depth));
        pass.set_bind_group(0, draw.texture_bind_group, &[]);
        pass.set_bind_group(1, &draw.uniform_bind_group, &[]);
        if let Some((x, y, w, h)) = draw.scissor {
            pass.set_scissor_rect(x, y, w, h);
        } else {
            pass.set_scissor_rect(0, 0, viewport.0, viewport.1);
        }
        pass.draw(0..4, 0..1);
    }

    /// Prepares one projective composite for later drawing INSIDE an
    /// existing render pass (the fused segment pass), mirroring
    /// [`Self::prepare_composite_batch_draws`] for the rotated-quad case.
    /// Unlike [`Self::encode_composite_to_view_projective`], the four
    /// uploaded vertices are the item's dest quad corners themselves (strip
    /// order TL, TR, BL, BR), not their AABB — the rasterized area is then
    /// exactly the transformed quad, which is what the segment-surface
    /// economics gate prices. The fragment shader's out-of-source discard
    /// stays as the safety net at the quad edges.
    pub(crate) fn prepare_projective_composite_draw<'a, C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        item: &ProjectiveCompositeItem<'a>,
        pass_depth: bool,
    ) -> PreparedProjectiveComposite<'a> {
        self.projective_blit_pipeline(device, item.blend_mode, pass_depth);
        let vertices = [
            ProjectiveBlitVertex {
                position: item.dest_quad[0],
            },
            ProjectiveBlitVertex {
                position: item.dest_quad[1],
            },
            ProjectiveBlitVertex {
                position: item.dest_quad[2],
            },
            ProjectiveBlitVertex {
                position: item.dest_quad[3],
            },
        ];
        let uniforms = ProjectiveBlitUniforms {
            viewport: [item.viewport.0 as f32, item.viewport.1 as f32],
            source_size: [item.source.width as f32, item.source.height as f32],
            inverse_row0: [
                item.inverse[0][0],
                item.inverse[0][1],
                item.inverse[0][2],
                0.0,
            ],
            inverse_row1: [
                item.inverse[1][0],
                item.inverse[1][1],
                item.inverse[1][2],
                0.0,
            ],
            inverse_row2: [
                item.inverse[2][0],
                item.inverse[2][1],
                item.inverse[2][2],
                0.0,
            ],
            alpha: [item.alpha.clamp(0.0, 1.0), 0.0, 0.0, 0.0],
            sampling: [
                composite_sampling_mode_value(item.sample_mode),
                0.0,
                0.0,
                0.0,
            ],
        };
        let sampler = self.sampler_for_mode(item.sample_mode);
        let texture_bind_group = item.source.get_or_create_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            sampler,
        );
        let vertex_buffer = recorder.upload_vertex(
            UploadAllocatorId::ProjectiveBlitVertex,
            projective_blit_vertex_spec(),
            device,
            bytemuck::cast_slice(&vertices),
            &self.debug_upload_bytes,
        );
        let uniform_bind_group = recorder.upload_uniform(
            UploadAllocatorId::ProjectiveBlitUniform,
            projective_blit_uniform_spec(),
            device,
            &self.blit_uniform_bind_group_layout,
            bytemuck::bytes_of(&uniforms),
            &self.debug_upload_bytes,
        );
        let quad = item.dest_quad;
        let min_x = quad.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
        let min_y = quad.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
        let max_x = quad.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max);
        let max_y = quad.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max);
        if min_x.is_finite() && min_y.is_finite() && max_x > min_x && max_y > min_y {
            self.note_composite_fill(
                None,
                Some((min_x, min_y, max_x - min_x, max_y - min_y)),
                item.viewport,
            );
        }
        PreparedProjectiveComposite {
            texture_bind_group,
            uniform_bind_group,
            vertex_buffer,
            blend_mode: item.blend_mode,
        }
    }

    /// Draws a prepared projective composite inside an existing pass. The
    /// scissor is reset to the full viewport, matching what the retained
    /// draws around it do.
    pub(crate) fn draw_prepared_projective_composite(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        viewport: (u32, u32),
        draw: &PreparedProjectiveComposite<'_>,
        pass_depth: bool,
    ) {
        pass.set_pipeline(self.initialized_projective_blit_pipeline(draw.blend_mode, pass_depth));
        pass.set_bind_group(0, draw.texture_bind_group, &[]);
        pass.set_bind_group(1, &draw.uniform_bind_group, &[]);
        pass.set_vertex_buffer(0, draw.vertex_buffer.slice(..));
        pass.set_scissor_rect(0, 0, viewport.0, viewport.1);
        pass.draw(0..4, 0..1);
    }

    /// Encode an offscreen composite pass into an existing command buffer.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn encode_composite_to_view_scissored_with_alpha_and_mask_and_blend_mode<
        C: FrameCommandRecorder,
    >(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        rounded_mask: Option<RoundedCompositeMask>,
        blend_mode: BlendMode,
        dest_viewport: Option<(f32, f32, f32, f32)>,
        sample_mode: CompositeSampleMode,
    ) {
        self.encode_composite_to_view_pass(
            recorder,
            device,
            source,
            dest_view,
            CompositePassOptions {
                alpha,
                load_op,
                scissor,
                rounded_mask,
                blend_mode,
                dest_viewport,
                source_viewport: None,
                sample_mode,
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn encode_composite_to_view_projective<C: FrameCommandRecorder>(
        &self,
        recorder: &mut C,
        device: &wgpu::Device,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        viewport: (u32, u32),
        source_size: (f32, f32),
        inverse_matrix: [[f32; 3]; 3],
        dest_bounds: [[f32; 2]; 4],
        alpha: f32,
        load_op: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<(u32, u32, u32, u32)>,
        blend_mode: BlendMode,
        sample_mode: CompositeSampleMode,
    ) -> bool {
        let Some((min_x, min_y, max_x, max_y)) = projective_dest_bounds_rect(dest_bounds) else {
            return false;
        };
        // The projective blit rasterizes the dest bounds' AABB quad.
        self.note_composite_fill(
            scissor,
            Some((min_x, min_y, max_x - min_x, max_y - min_y)),
            viewport,
        );

        let vertices = [
            ProjectiveBlitVertex {
                position: [min_x, min_y],
            },
            ProjectiveBlitVertex {
                position: [max_x, min_y],
            },
            ProjectiveBlitVertex {
                position: [min_x, max_y],
            },
            ProjectiveBlitVertex {
                position: [max_x, max_y],
            },
        ];
        let uniforms = ProjectiveBlitUniforms {
            viewport: [viewport.0 as f32, viewport.1 as f32],
            source_size: [source_size.0.max(0.0), source_size.1.max(0.0)],
            inverse_row0: [
                inverse_matrix[0][0],
                inverse_matrix[0][1],
                inverse_matrix[0][2],
                0.0,
            ],
            inverse_row1: [
                inverse_matrix[1][0],
                inverse_matrix[1][1],
                inverse_matrix[1][2],
                0.0,
            ],
            inverse_row2: [
                inverse_matrix[2][0],
                inverse_matrix[2][1],
                inverse_matrix[2][2],
                0.0,
            ],
            alpha: [alpha.clamp(0.0, 1.0), 0.0, 0.0, 0.0],
            sampling: [composite_sampling_mode_value(sample_mode), 0.0, 0.0, 0.0],
        };

        let sampler = self.sampler_for_mode(sample_mode);
        let texture_bind_group = source.get_or_create_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            sampler,
        );
        let vertex_buffer = recorder.upload_vertex(
            UploadAllocatorId::ProjectiveBlitVertex,
            projective_blit_vertex_spec(),
            device,
            bytemuck::cast_slice(&vertices),
            &self.debug_upload_bytes,
        );
        let uniform_bind_group = recorder.upload_uniform(
            UploadAllocatorId::ProjectiveBlitUniform,
            projective_blit_uniform_spec(),
            device,
            &self.blit_uniform_bind_group_layout,
            bytemuck::bytes_of(&uniforms),
            &self.debug_upload_bytes,
        );
        let mut pass = recorder
            .encoder()
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Projective Blit Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dest_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
        pass.set_pipeline(self.projective_blit_pipeline(device, blend_mode, false));
        pass.set_bind_group(0, texture_bind_group, &[]);
        pass.set_bind_group(1, &uniform_bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        if let Some((x, y, w, h)) = scissor {
            pass.set_scissor_rect(x, y, w, h);
        }
        pass.draw(0..4, 0..1);
        true
    }
}

pub(crate) fn projective_dest_bounds_rect(
    dest_bounds: [[f32; 2]; 4],
) -> Option<(f32, f32, f32, f32)> {
    let min_x = dest_bounds
        .iter()
        .map(|point| point[0])
        .fold(f32::INFINITY, f32::min);
    let min_y = dest_bounds
        .iter()
        .map(|point| point[1])
        .fold(f32::INFINITY, f32::min);
    let max_x = dest_bounds
        .iter()
        .map(|point| point[0])
        .fold(f32::NEG_INFINITY, f32::max);
    let max_y = dest_bounds
        .iter()
        .map(|point| point[1])
        .fold(f32::NEG_INFINITY, f32::max);

    (min_x.is_finite()
        && min_y.is_finite()
        && max_x.is_finite()
        && max_y.is_finite()
        && max_x > min_x
        && max_y > min_y)
        .then_some((min_x, min_y, max_x, max_y))
}

fn tile_mode_uniform_value(tile_mode: TileMode) -> f32 {
    match tile_mode {
        TileMode::Clamp => 0.0,
        TileMode::Repeated => 1.0,
        TileMode::Mirror => 2.0,
        TileMode::Decal => 3.0,
    }
}

fn composite_sampling_mode_value(sample_mode: CompositeSampleMode) -> f32 {
    match sample_mode {
        CompositeSampleMode::Linear => 0.0,
        CompositeSampleMode::Box4 => 1.0,
        CompositeSampleMode::Nearest => 2.0,
    }
}

#[cfg(test)]
mod tests {
    use super::{projective_dest_bounds_rect, BlurUniforms};

    #[test]
    fn blur_uniforms_use_vec4_packing_for_gl_backends() {
        assert_eq!(std::mem::size_of::<BlurUniforms>(), 32);
        assert_eq!(std::mem::offset_of!(BlurUniforms, direction_and_radius), 0);
        assert_eq!(
            std::mem::offset_of!(BlurUniforms, texture_size_and_tile_mode),
            16
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn effect_pass_fill_prefers_the_viewport_and_clamps_by_the_scissor() {
        use super::effect_pass_fill_px2;
        // No scissor, no viewport: the whole fallback target.
        assert_eq!(effect_pass_fill_px2(None, None, (100, 50)), 5000.0);
        // A dest viewport bounds the rasterized region.
        assert_eq!(
            effect_pass_fill_px2(None, Some((5.0, 5.0, 40.0, 10.0)), (100, 50)),
            400.0
        );
        // A scissor clamps the fallback.
        assert_eq!(
            effect_pass_fill_px2(Some((0, 0, 30, 30)), None, (100, 50)),
            900.0
        );
        // A scissor larger than the viewport never inflates the pass.
        assert_eq!(
            effect_pass_fill_px2(
                Some((0, 0, 100, 100)),
                Some((0.0, 0.0, 20.0, 20.0)),
                (100, 50)
            ),
            400.0
        );
        // A degenerate viewport falls back to the target size.
        assert_eq!(
            effect_pass_fill_px2(None, Some((0.0, 0.0, 0.0, 0.0)), (10, 10)),
            100.0
        );
    }

    #[test]
    fn projective_dest_bounds_reject_degenerate_quads() {
        assert_eq!(
            projective_dest_bounds_rect([[2.0, 3.0], [2.0, 3.0], [2.0, 3.0], [2.0, 3.0]]),
            None
        );
        assert_eq!(
            projective_dest_bounds_rect([[1.0, 2.0], [5.0, 2.0], [1.0, 7.0], [5.0, 7.0]]),
            Some((1.0, 2.0, 5.0, 7.0))
        );
    }
}
