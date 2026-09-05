use std::cell::Cell;

use cranpose_render_common::geometry::blur_scratch_block;
use cranpose_ui_graphics::{BlendMode, MAX_SUBSTRATES, RenderEffect, RuntimeShader, TileMode};

use crate::{
    frame_graph::{
        BufferUpload, FrameCommandRecorder, FrameCommandStats, FrameTextureDescriptor,
        UniformUpload, UploadAllocatorId, UploadAllocatorSpec,
    },
    gpu_stats::FrameStats,
    lazy_resource::LazyGpuResource,
    offscreen::{OffscreenPool, OffscreenTarget},
    shader_cache::{
        RuntimeShaderPipelineMode, ShaderDrawVariant, ShaderPipelineCache,
        shader_specialization_enabled,
    },
    shaders,
};

pub(crate) fn blur_scratch_size(
    radius_x: f32,
    radius_y: f32,
    width: u32,
    height: u32,
) -> (u32, u32) {
    scratch_size_at(blur_scratch_block(radius_x.max(radius_y)), width, height)
}

/// The scratch a substrate blur of `radius_px` runs at: a low-frequency copy
/// read once per fragment tolerates a coarser grid than a blur that is the
/// picture, so the scale doubles at half the radius the blur's does.
pub(crate) fn substrate_scratch_size(radius_px: f32, width: u32, height: u32) -> (u32, u32) {
    let scale = if radius_px < 3.0 {
        1
    } else if radius_px < 8.0 {
        2
    } else {
        4
    };
    scratch_size_at(scale, width, height)
}

fn scratch_size_at(mut scale: u32, width: u32, height: u32) -> (u32, u32) {
    while scale > 1 && (width / scale < 16 || height / scale < 16) {
        scale /= 2;
    }
    if scale <= 1 {
        return (width, height);
    }
    (width.div_ceil(scale).max(1), height.div_ceil(scale).max(1))
}

fn scaled_scissor(
    scissor: Option<(u32, u32, u32, u32)>,
    scale_x: f32,
    scale_y: f32,
    width: u32,
    height: u32,
) -> Option<(u32, u32, u32, u32)> {
    let (x, y, w, h) = scissor?;
    if scale_x <= 1.0 && scale_y <= 1.0 {
        return scissor;
    }
    let left = ((x as f32 / scale_x).floor() as u32).min(width);
    let top = ((y as f32 / scale_y).floor() as u32).min(height);
    let right = (((x + w) as f32 / scale_x).ceil() as u32).min(width);
    let bottom = (((y + h) as f32 / scale_y).ceil() as u32).min(height);
    Some((
        left,
        top,
        right.saturating_sub(left),
        bottom.saturating_sub(top),
    ))
}

pub(crate) struct EffectRenderer {
    offscreen_pool: OffscreenPool,
    pub shader_cache: ShaderPipelineCache,
    pipeline_cache: Option<wgpu::PipelineCache>,

    blur_shader: wgpu::ShaderModule,
    blur_pipeline_layout: wgpu::PipelineLayout,
    blur_pipeline: LazyGpuResource<wgpu::RenderPipeline>,
    blur_downsample_pipelines:
        [LazyGpuResource<wgpu::RenderPipeline>; BLUR_DOWNSAMPLE_BLOCKS.len()],
    blur_uniform_bind_group_layout: wgpu::BindGroupLayout,

    offset_shader: wgpu::ShaderModule,
    offset_pipeline_layout: wgpu::PipelineLayout,
    offset_pipeline: LazyGpuResource<wgpu::RenderPipeline>,
    offset_uniform_bind_group_layout: wgpu::BindGroupLayout,

    blit_shader: wgpu::ShaderModule,
    blit_pipeline_layout: wgpu::PipelineLayout,
    blit_pipeline: LazyGpuResource<wgpu::RenderPipeline>,
    blit_pipeline_src: LazyGpuResource<wgpu::RenderPipeline>,
    blit_pipeline_dst_out: LazyGpuResource<wgpu::RenderPipeline>,
    blit_uniform_bind_group_layout: wgpu::BindGroupLayout,
    projective_blit_shader: wgpu::ShaderModule,
    projective_blit_pipeline_layout: wgpu::PipelineLayout,
    projective_blit_pipeline: LazyGpuResource<wgpu::RenderPipeline>,
    projective_blit_pipeline_src: LazyGpuResource<wgpu::RenderPipeline>,
    projective_blit_pipeline_dst_out: LazyGpuResource<wgpu::RenderPipeline>,

    pub effect_texture_bind_group_layout: wgpu::BindGroupLayout,
    pub effect_uniform_bind_group_layout: wgpu::BindGroupLayout,

    pub effect_linear_sampler: wgpu::Sampler,

    surface_format: wgpu::TextureFormat,
    adapter_backend: wgpu::Backend,

    pub(crate) debug_command_stats: Cell<FrameCommandStats>,
    pub(crate) debug_blurs: Cell<u32>,
    pub(crate) debug_substrates: Cell<u32>,
    pub(crate) debug_composites: Cell<u32>,
    pub(crate) debug_effects: Cell<u32>,
    pub(crate) debug_shader_pixels: Cell<u64>,
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
                let (scratch_width, scratch_height) =
                    blur_scratch_size(*radius_x, *radius_y, width, height);
                let descriptor = FrameTextureDescriptor::render_attachment(
                    "Render Effect Blur Scratch",
                    scratch_width,
                    scratch_height,
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

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct BlurUniforms {
    direction_and_radius: [f32; 4],
    texture_size_and_tile_mode: [f32; 4],
    source_region: [f32; 4],
    dest_region: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct OffsetUniforms {
    offset: [f32; 2],
    _padding: [f32; 2],
}

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

/// The blocks of source texels a blur's downsample averages per scratch
/// pixel: the downscales `blur_scratch_size` chooses.
const BLUR_DOWNSAMPLE_BLOCKS: [u32; 2] = [2, 4];

/// The block of source texels one pixel of a blur's scratch stands for: one
/// when `scratch` is `source`'s size.
fn blur_block(source: (u32, u32, u32, u32), scratch: (u32, u32, u32, u32)) -> u32 {
    (source.2 as f32 / scratch.2.max(1) as f32).round().max(1.0) as u32
}

fn blur_uniform_spec(pass: UploadAllocatorId) -> UploadAllocatorSpec {
    let (buffer, bind_group) = match pass {
        UploadAllocatorId::BlurDownsample => (
            "Blur Downsample Uniform Buffer",
            "Blur Downsample Uniform Bind Group",
        ),
        UploadAllocatorId::BlurHorizontal => (
            "Blur Horizontal Uniform Buffer",
            "Blur Horizontal Uniform Bind Group",
        ),
        _ => (
            "Blur Vertical Uniform Buffer",
            "Blur Vertical Uniform Bind Group",
        ),
    };
    UploadAllocatorSpec::uniform(
        buffer,
        bind_group,
        std::mem::size_of::<BlurUniforms>() as u64,
    )
}

/// One draw of a blur pass: the texture it samples, its uniforms, the
/// downsample block it averages (the kernel when `None`) and the
/// destination pixels it covers.
struct BlurDraw<'a> {
    source: &'a OffscreenTarget,
    uniforms: BlurUniforms,
    downsample: Option<u32>,
    scissor: Option<(u32, u32, u32, u32)>,
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

#[derive(Copy, Clone, Debug)]
pub(crate) struct RoundedCompositeMask {
    pub rect: [f32; 4],
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
    source_logical_size: Option<(f32, f32)>,
}

/// A runtime shader drawn into a pass over `dest_viewport`, reading
/// `source_region` of `source` (the whole texture when `None`), which
/// stands for `source_logical_size` pixels when it is a downscaled result,
/// with `layer_pixel_rect` relative to that region, masked by
/// `rounded_mask` (target pixels) and scaled by `alpha`. Region, logical
/// size, mask and alpha reach the shader through its reserved uniform slots
/// and require a shader that declares `batched_source`.
#[derive(Clone, Copy)]
pub(crate) struct ShaderCompositeBatchItem<'a> {
    pub(crate) source: &'a OffscreenTarget,
    pub(crate) shader: &'a RuntimeShader,
    pub(crate) layer_pixel_rect: [f32; 4],
    pub(crate) source_region: Option<(f32, f32, f32, f32)>,
    pub(crate) source_logical_size: Option<(f32, f32)>,
    pub(crate) substrate_regions: SubstrateRegions,
    pub(crate) rounded_mask: Option<RoundedCompositeMask>,
    pub(crate) alpha: f32,
    pub(crate) scissor: Option<(u32, u32, u32, u32)>,
    pub(crate) dest_viewport: (f32, f32, f32, f32),
}

/// Fills the renderer-reserved uniform slots of a runtime shader; see
/// `RuntimeShader`'s slot table.
/// The regions of a shader's substrates in its input texture, in slot
/// order; none where the shader declared fewer or the stage packed none.
pub(crate) type SubstrateRegions = [Option<(f32, f32, f32, f32)>; MAX_SUBSTRATES];

struct ReservedShaderUniforms {
    layer_pixel_rect: [f32; 4],
    source_region: Option<(f32, f32, f32, f32)>,
    substrate_regions: SubstrateRegions,
    mask: Option<RoundedCompositeMask>,
    logical_size: Option<(f32, f32)>,
    alpha: f32,
}

fn region_slot(region: Option<(f32, f32, f32, f32)>) -> [f32; 4] {
    let region = region.unwrap_or((0.0, 0.0, 0.0, 0.0));
    [region.0, region.1, region.2, region.3]
}

impl ReservedShaderUniforms {
    fn write(&self, padded: &mut [f32; RuntimeShader::MAX_UNIFORMS]) {
        let (mask_rect, mask_radii) = self
            .mask
            .map_or(([0.0; 4], [0.0; 4]), |mask| (mask.rect, mask.radii));
        let logical = self.logical_size.unwrap_or((0.0, 0.0));
        for (slot, region) in RuntimeShader::SUBSTRATE_REGION_UNIFORMS
            .iter()
            .zip(self.substrate_regions)
        {
            padded[*slot..*slot + 4].copy_from_slice(&region_slot(region));
        }
        let slots = [
            (
                RuntimeShader::SOURCE_REGION_UNIFORM,
                region_slot(self.source_region),
            ),
            (RuntimeShader::MASK_RECT_UNIFORM, mask_rect),
            (RuntimeShader::MASK_RADII_UNIFORM, mask_radii),
            (RuntimeShader::EFFECT_RECT_UNIFORM, self.layer_pixel_rect),
            (
                RuntimeShader::LOGICAL_SIZE_UNIFORM,
                [logical.0, logical.1, self.alpha, 0.0],
            ),
        ];
        for (start, values) in slots {
            padded[start..start + 4].copy_from_slice(&values);
        }
    }
}

/// Pixels a shader draw shades: its viewport clipped by its scissor.
fn shaded_pixels(viewport: (f32, f32, f32, f32), scissor: (u32, u32, u32, u32)) -> u64 {
    let left = viewport.0.max(scissor.0 as f32);
    let top = viewport.1.max(scissor.1 as f32);
    let right = (viewport.0 + viewport.2).min((scissor.0 + scissor.2) as f32);
    let bottom = (viewport.1 + viewport.3).min((scissor.1 + scissor.3) as f32);
    ((right - left).max(0.0).round() as u64) * ((bottom - top).max(0.0).round() as u64)
}

fn region_uniform(region: (u32, u32, u32, u32)) -> [f32; 4] {
    [
        region.0 as f32,
        region.1 as f32,
        region.2 as f32,
        region.3 as f32,
    ]
}

/// One region of a blur atlas pass: the capture texels to blur, the
/// downscaled slot the horizontal pass writes in the scratch texture and
/// the vertical pass writes in the result texture, and the blur to run.
#[derive(Clone, Copy)]
pub(crate) struct BlurRegion {
    pub(crate) source: (u32, u32, u32, u32),
    pub(crate) scratch: (u32, u32, u32, u32),
    pub(crate) radius_x: f32,
    pub(crate) radius_y: f32,
    pub(crate) tile_mode: TileMode,
}

/// One averaged substrate of a capture atlas: the capture texels to
/// average in blocks of `block`, and the slot the downsample writes in the
/// result texture.
#[derive(Clone, Copy)]
pub(crate) struct SubstrateRegion {
    pub(crate) source: (u32, u32, u32, u32),
    pub(crate) scratch: (u32, u32, u32, u32),
    pub(crate) block: u32,
}

/// What one capture atlas renders beside itself: the regions to blur and
/// the regions to average.
pub(crate) struct AtlasSideWork<'a> {
    pub(crate) blurs: &'a [BlurRegion],
    pub(crate) averages: &'a [SubstrateRegion],
}

impl BlurRegion {
    fn scratch_radius(&self) -> (f32, f32) {
        (
            self.radius_x * self.scratch.2 as f32 / self.source.2.max(1) as f32,
            self.radius_y * self.scratch.3 as f32 / self.source.3.max(1) as f32,
        )
    }
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
    uniform: UniformUpload,
    scissor: Option<(u32, u32, u32, u32)>,
    blend_mode: BlendMode,
}

pub(crate) struct PreparedShaderDraw<'a> {
    shader: &'a RuntimeShader,
    texture_bind_group: &'a wgpu::BindGroup,
    uniform: UniformUpload,
    scissor: Option<(u32, u32, u32, u32)>,
    dest_viewport: (f32, f32, f32, f32),
    variants: &'static [ShaderDrawVariant],
}

const WHOLE_DRAW: &[ShaderDrawVariant] = &[ShaderDrawVariant::Whole];
const SPLIT_DRAWS: &[ShaderDrawVariant] = &[ShaderDrawVariant::Interior, ShaderDrawVariant::Rim];

/// The draws a shader makes in a pass: its interior and its rim when it
/// declared a split and the pipelines are specialized, else the one draw.
fn shader_draw_variants(shader: &RuntimeShader) -> &'static [ShaderDrawVariant] {
    if shader.draw_split().is_some() && shader_specialization_enabled() {
        SPLIT_DRAWS
    } else {
        WHOLE_DRAW
    }
}

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
    uniform: UniformUpload,
    vertices: BufferUpload,
    blend_mode: BlendMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompositeSampleMode {
    Linear,
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
    cache: Option<&wgpu::PipelineCache>,
    label: &'static str,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    fragment_entry: &'static str,
    constants: &[(&str, f64)],
    surface_format: wgpu::TextureFormat,
    blend: wgpu::BlendState,
) -> wgpu::RenderPipeline {
    crate::render::create_fullscreen_strip_pipeline(
        device,
        cache,
        &format!("effect {label} entry={fragment_entry}"),
        label,
        layout,
        shader,
        fragment_entry,
        constants,
        wgpu::ColorTargetState {
            format: surface_format,
            blend: Some(blend),
            write_mask: wgpu::ColorWrites::ALL,
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn create_projective_pipeline(
    device: &wgpu::Device,
    cache: Option<&wgpu::PipelineCache>,
    label: &'static str,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    surface_format: wgpu::TextureFormat,
    blend: wgpu::BlendState,
) -> wgpu::RenderPipeline {
    crate::render::create_render_pipeline_logged(
        device,
        cache,
        &format!("effect {label}"),
        wgpu::RenderPipelineDescriptor {
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
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        },
    )
}

impl EffectRenderer {
    pub fn new(
        device: &wgpu::Device,
        pipeline_cache: Option<wgpu::PipelineCache>,
        surface_format: wgpu::TextureFormat,
        adapter_backend: wgpu::Backend,
    ) -> Self {
        let effect_texture_bind_group_layout = OffscreenPool::texture_bind_group_layout(device);
        let effect_uniform_bind_group_layout = OffscreenPool::uniform_bind_group_layout(device);

        let blur_uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Blur Uniform Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let offset_uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Offset Uniform Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let blit_uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Blit Uniform Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

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
        let blur_downsample_pipelines = [
            LazyGpuResource::new("effect/blur-downsample/2"),
            LazyGpuResource::new("effect/blur-downsample/4"),
        ];

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

        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Blit Shader"),
            source: wgpu::ShaderSource::Wgsl(shaders::blit_shader().into()),
        });

        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Blit Pipeline Layout"),
            bind_group_layouts: &[
                Some(&effect_texture_bind_group_layout),
                Some(&blit_uniform_bind_group_layout),
            ],
            immediate_size: 0,
        });

        let blit_pipeline = LazyGpuResource::new("effect/blit-src-over");
        let blit_pipeline_src = LazyGpuResource::new("effect/blit-src");
        let blit_pipeline_dst_out = LazyGpuResource::new("effect/blit-dst-out");

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
        let projective_blit_pipeline = LazyGpuResource::new("effect/projective-src-over");
        let projective_blit_pipeline_src = LazyGpuResource::new("effect/projective-src");
        let projective_blit_pipeline_dst_out = LazyGpuResource::new("effect/projective-dst-out");

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
            shader_cache: ShaderPipelineCache::new(adapter_backend, pipeline_cache.clone()),
            pipeline_cache,
            blur_shader,
            blur_pipeline_layout,
            blur_pipeline,
            blur_downsample_pipelines,
            blur_uniform_bind_group_layout,
            offset_shader,
            offset_pipeline_layout,
            offset_pipeline,
            offset_uniform_bind_group_layout,
            blit_shader,
            blit_pipeline_layout,
            blit_pipeline,
            blit_pipeline_src,
            blit_pipeline_dst_out,
            blit_uniform_bind_group_layout,
            projective_blit_shader,
            projective_blit_pipeline_layout,
            projective_blit_pipeline,
            projective_blit_pipeline_src,
            projective_blit_pipeline_dst_out,
            effect_texture_bind_group_layout,
            effect_uniform_bind_group_layout,
            effect_linear_sampler,
            surface_format,
            adapter_backend,
            debug_command_stats: Cell::new(FrameCommandStats::default()),
            debug_blurs: Cell::new(0),
            debug_substrates: Cell::new(0),
            debug_composites: Cell::new(0),
            debug_effects: Cell::new(0),
            debug_shader_pixels: Cell::new(0),
        }
    }

    fn blur_pipeline(&self, device: &wgpu::Device) -> &wgpu::RenderPipeline {
        self.blur_pipeline.get_or_init(self.adapter_backend, || {
            create_fullscreen_pipeline(
                device,
                self.pipeline_cache.as_ref(),
                "Blur Pipeline",
                &self.blur_pipeline_layout,
                &self.blur_shader,
                "blur_fs",
                &[],
                self.surface_format,
                wgpu::BlendState::REPLACE,
            )
        })
    }

    /// The downsample pipeline averaging `block` source texels per axis into
    /// one pixel of a blur's scratch.
    fn blur_downsample_pipeline(&self, device: &wgpu::Device, block: u32) -> &wgpu::RenderPipeline {
        let index = BLUR_DOWNSAMPLE_BLOCKS
            .iter()
            .position(|candidate| *candidate == block)
            .unwrap_or_else(|| {
                panic!("a blur downsample block of {block}; the scratch is 2 or 4 to 1")
            });
        self.blur_downsample_pipelines[index].get_or_init(self.adapter_backend, || {
            create_fullscreen_pipeline(
                device,
                self.pipeline_cache.as_ref(),
                "Blur Downsample Pipeline",
                &self.blur_pipeline_layout,
                &self.blur_shader,
                "blur_downsample_fs",
                &[("BLUR_BLOCK", f64::from(block))],
                self.surface_format,
                wgpu::BlendState::REPLACE,
            )
        })
    }

    fn offset_pipeline(&self, device: &wgpu::Device) -> &wgpu::RenderPipeline {
        self.offset_pipeline.get_or_init(self.adapter_backend, || {
            create_fullscreen_pipeline(
                device,
                self.pipeline_cache.as_ref(),
                "Offset Pipeline",
                &self.offset_pipeline_layout,
                &self.offset_shader,
                "offset_fs",
                &[],
                self.surface_format,
                wgpu::BlendState::REPLACE,
            )
        })
    }

    fn blit_pipeline(&self, device: &wgpu::Device, blend_mode: BlendMode) -> &wgpu::RenderPipeline {
        let (resource, label, blend) = match blend_mode {
            BlendMode::Src => (
                &self.blit_pipeline_src,
                "Blit Pipeline Src",
                wgpu::BlendState::REPLACE,
            ),
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
        resource.get_or_init(self.adapter_backend, || {
            create_fullscreen_pipeline(
                device,
                self.pipeline_cache.as_ref(),
                label,
                &self.blit_pipeline_layout,
                &self.blit_shader,
                "blit_fs",
                &[],
                self.surface_format,
                blend,
            )
        })
    }

    fn initialized_blit_pipeline(&self, blend_mode: BlendMode) -> &wgpu::RenderPipeline {
        let resource = match blend_mode {
            BlendMode::Src => &self.blit_pipeline_src,
            BlendMode::DstOut => &self.blit_pipeline_dst_out,
            _ => &self.blit_pipeline,
        };
        resource
            .get()
            .expect("prepared composite must initialize its blit pipeline")
    }

    fn projective_blit_pipeline(
        &self,
        device: &wgpu::Device,
        blend_mode: BlendMode,
    ) -> &wgpu::RenderPipeline {
        let (resource, label, blend) = match blend_mode {
            BlendMode::Src => (
                &self.projective_blit_pipeline_src,
                "Projective Blit Pipeline Src",
                wgpu::BlendState::REPLACE,
            ),
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
        resource.get_or_init(self.adapter_backend, || {
            create_projective_pipeline(
                device,
                self.pipeline_cache.as_ref(),
                label,
                &self.projective_blit_pipeline_layout,
                &self.projective_blit_shader,
                self.surface_format,
                blend,
            )
        })
    }

    fn initialized_projective_blit_pipeline(&self, blend_mode: BlendMode) -> &wgpu::RenderPipeline {
        let resource = match blend_mode {
            BlendMode::Src => &self.projective_blit_pipeline_src,
            BlendMode::DstOut => &self.projective_blit_pipeline_dst_out,
            _ => &self.projective_blit_pipeline,
        };
        resource
            .get()
            .expect("prepared projective composite must initialize its pipeline")
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
            .substrates
            .set(stats.substrates.get() + self.debug_substrates.get());
        stats
            .composite_passes
            .set(stats.composite_passes.get() + self.debug_composites.get());
        stats
            .effect_applies
            .set(stats.effect_applies.get() + self.debug_effects.get());
        stats
            .shader_pixels
            .set(stats.shader_pixels.get() + self.debug_shader_pixels.get());
        self.debug_command_stats.set(FrameCommandStats::default());
        self.debug_blurs.set(0);
        self.debug_substrates.set(0);
        self.debug_composites.set(0);
        self.debug_effects.set(0);
        self.debug_shader_pixels.set(0);
    }

    pub(crate) fn record_blur_pass(&self) {
        self.debug_blurs.set(self.debug_blurs.get() + 1);
    }

    pub(crate) fn record_substrates(&self, count: u32) {
        self.debug_substrates
            .set(self.debug_substrates.get() + count);
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

    #[allow(clippy::too_many_arguments)]
    /// Encodes one blur pass over `dest_view`: the draws in order, each
    /// with its own source, uniforms and pipeline.
    fn encode_blur_pass<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        label: &'static str,
        pass_id: UploadAllocatorId,
        dest_view: &wgpu::TextureView,
        draws: &[BlurDraw<'_>],
    ) {
        let uniforms: Vec<UniformUpload> = draws
            .iter()
            .map(|draw| {
                recorder.upload_uniform(
                    pass_id,
                    blur_uniform_spec(pass_id),
                    device,
                    &self.blur_uniform_bind_group_layout,
                    bytemuck::bytes_of(&draw.uniforms),
                )
            })
            .collect();
        let mut pass = recorder.begin_color_pass(
            label,
            dest_view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        );
        let mut bound = None;
        for (draw, uniform) in draws.iter().zip(&uniforms) {
            if bound != Some(draw.downsample) {
                pass.set_pipeline(match draw.downsample {
                    Some(block) => self.blur_downsample_pipeline(device, block),
                    None => self.blur_pipeline(device),
                });
                bound = Some(draw.downsample);
            }
            let source_bind_group = draw.source.get_or_create_bind_group(
                device,
                &self.effect_texture_bind_group_layout,
                &self.effect_linear_sampler,
            );
            pass.set_bind_group(0, source_bind_group, &[]);
            pass.set_bind_group(1, &uniform.bind_group, &[uniform.offset]);
            if let Some((x, y, width, height)) = draw.scissor {
                pass.set_scissor_rect(x, y, width, height);
            }
            pass.draw(0..4, 0..1);
        }
    }

    /// The uniforms of one blur pass sampling `sampled`, reading its
    /// `source` region and writing the `dest` region of the target; the
    /// radius counts kernel steps, the coarser of a source texel and a
    /// destination pixel.
    fn blur_uniforms(
        horizontal: bool,
        sampled: (u32, u32),
        source: (u32, u32, u32, u32),
        dest: (u32, u32, u32, u32),
        radius: (f32, f32),
        tile_mode: TileMode,
    ) -> BlurUniforms {
        let direction = if horizontal { [1.0, 0.0] } else { [0.0, 1.0] };
        BlurUniforms {
            direction_and_radius: [direction[0], direction[1], radius.0, radius.1],
            texture_size_and_tile_mode: [
                sampled.0 as f32,
                sampled.1 as f32,
                tile_mode_uniform_value(tile_mode),
                0.0,
            ],
            source_region: region_uniform(source),
            dest_region: region_uniform(dest),
        }
    }

    /// Blurs `source` into `dest` through `scratch`: a wide blur first
    /// averages each block of source texels into a scratch-size downsample
    /// and runs its horizontal pass over that, so no source texel is skipped;
    /// the vertical pass reads the scratch and writes `dest`, at the
    /// source's size or the scratch's. Returns the passes encoded.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn encode_blur_scissored_ping_pong_passes<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        source: &OffscreenTarget,
        scratch: &OffscreenTarget,
        dest: (&wgpu::TextureView, (u32, u32)),
        radius_x: f32,
        radius_y: f32,
        tile_mode: TileMode,
        scissor: Option<(u32, u32, u32, u32)>,
    ) -> u32 {
        debug_assert!(
            radius_x > 0.0 || radius_y > 0.0,
            "zero-radius blur should use the composite fast path"
        );
        let scale_x = source.width as f32 / scratch.width.max(1) as f32;
        let scale_y = source.height as f32 / scratch.height.max(1) as f32;
        let radius = (radius_x / scale_x, radius_y / scale_y);
        let whole_source = (0, 0, source.width, source.height);
        let whole_scratch = (0, 0, scratch.width, scratch.height);
        let scratch_scissor =
            scaled_scissor(scissor, scale_x, scale_y, scratch.width, scratch.height);
        let (dest_view, dest_size) = dest;
        let (dest_region, dest_scissor) = if dest_size == (scratch.width, scratch.height) {
            (whole_scratch, scratch_scissor)
        } else {
            ((0, 0, dest_size.0, dest_size.1), scissor)
        };
        let block = blur_block(whole_source, whole_scratch);
        let small = (block > 1).then(|| {
            let descriptor = FrameTextureDescriptor::render_attachment(
                "Blur Downsample",
                scratch.width,
                scratch.height,
                self.surface_format,
            );
            (
                descriptor,
                recorder.acquire_transient_offscreen(device, descriptor),
            )
        });
        if let Some((_, small)) = &small {
            self.encode_blur_pass(
                recorder,
                device,
                "Blur Downsample Pass",
                UploadAllocatorId::BlurDownsample,
                &small.view,
                &[BlurDraw {
                    source,
                    uniforms: Self::blur_uniforms(
                        true,
                        (source.width, source.height),
                        whole_source,
                        whole_scratch,
                        (0.0, 0.0),
                        tile_mode,
                    ),
                    downsample: Some(block),
                    scissor: scratch_scissor,
                }],
            );
        }
        let (horizontal_source, horizontal_region) = match &small {
            Some((_, small)) => (small, whole_scratch),
            None => (source, whole_source),
        };
        self.encode_blur_pass(
            recorder,
            device,
            "Blur Horizontal Pass",
            UploadAllocatorId::BlurHorizontal,
            &scratch.view,
            &[BlurDraw {
                source: horizontal_source,
                uniforms: Self::blur_uniforms(
                    true,
                    (horizontal_source.width, horizontal_source.height),
                    horizontal_region,
                    whole_scratch,
                    radius,
                    tile_mode,
                ),
                downsample: None,
                scissor: scratch_scissor,
            }],
        );
        self.encode_blur_pass(
            recorder,
            device,
            "Blur Vertical Pass",
            UploadAllocatorId::BlurVertical,
            dest_view,
            &[BlurDraw {
                source: scratch,
                uniforms: Self::blur_uniforms(
                    false,
                    (scratch.width, scratch.height),
                    whole_scratch,
                    dest_region,
                    radius,
                    tile_mode,
                ),
                downsample: None,
                scissor: dest_scissor,
            }],
        );
        match small {
            Some((descriptor, small)) => {
                recorder.release_transient_offscreen(descriptor, small);
                3
            }
            None => 2,
        }
    }

    /// Blurs every region of the capture atlas: a wide region's downsample
    /// averages its blocks of texels into its scratch slot of the result
    /// texture, the horizontal pass writes each region's scratch slot from
    /// that downsample or, unscaled, from the atlas, and the vertical pass
    /// reads the slot and writes the same slot of the result texture. Both
    /// targets hold only the blurred regions, so no pass loads or stores
    /// the atlas.
    pub(crate) fn encode_blur_atlas_passes<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        atlas: &OffscreenTarget,
        scratch: &OffscreenTarget,
        result: &OffscreenTarget,
        work: AtlasSideWork<'_>,
    ) {
        let AtlasSideWork {
            blurs: regions,
            averages: substrates,
        } = work;
        let blocks: Vec<u32> = regions
            .iter()
            .map(|region| blur_block(region.source, region.scratch))
            .collect();
        let downsample_draw = |source: (u32, u32, u32, u32),
                               scratch: (u32, u32, u32, u32),
                               block: u32,
                               tile_mode: TileMode| BlurDraw {
            source: atlas,
            uniforms: Self::blur_uniforms(
                true,
                (atlas.width, atlas.height),
                source,
                scratch,
                (0.0, 0.0),
                tile_mode,
            ),
            downsample: Some(block),
            scissor: Some(scratch),
        };
        let downsample: Vec<BlurDraw<'_>> = regions
            .iter()
            .zip(&blocks)
            .filter(|(_, block)| **block > 1)
            .map(|(region, block)| {
                downsample_draw(region.source, region.scratch, *block, region.tile_mode)
            })
            .chain(substrates.iter().map(|substrate| {
                downsample_draw(
                    substrate.source,
                    substrate.scratch,
                    substrate.block,
                    TileMode::Clamp,
                )
            }))
            .collect();
        if !downsample.is_empty() {
            self.encode_blur_pass(
                recorder,
                device,
                "Blur Downsample Pass",
                UploadAllocatorId::BlurDownsample,
                &result.view,
                &downsample,
            );
        }
        if regions.is_empty() {
            return;
        }
        let horizontal: Vec<BlurDraw<'_>> = regions
            .iter()
            .zip(&blocks)
            .map(|(region, block)| {
                let (source, source_region) = if *block > 1 {
                    (result, region.scratch)
                } else {
                    (atlas, region.source)
                };
                BlurDraw {
                    source,
                    uniforms: Self::blur_uniforms(
                        true,
                        (source.width, source.height),
                        source_region,
                        region.scratch,
                        region.scratch_radius(),
                        region.tile_mode,
                    ),
                    downsample: None,
                    scissor: Some(region.scratch),
                }
            })
            .collect();
        self.encode_blur_pass(
            recorder,
            device,
            "Blur Horizontal Pass",
            UploadAllocatorId::BlurHorizontal,
            &scratch.view,
            &horizontal,
        );
        let vertical: Vec<BlurDraw<'_>> = regions
            .iter()
            .map(|region| BlurDraw {
                source: scratch,
                uniforms: Self::blur_uniforms(
                    false,
                    (scratch.width, scratch.height),
                    region.scratch,
                    region.scratch,
                    region.scratch_radius(),
                    region.tile_mode,
                ),
                downsample: None,
                scissor: Some(region.scratch),
            })
            .collect();
        self.encode_blur_pass(
            recorder,
            device,
            "Blur Vertical Pass",
            UploadAllocatorId::BlurVertical,
            &result.view,
            &vertical,
        );
        self.record_blur_pass();
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
        let uniform = recorder.upload_uniform(
            UploadAllocatorId::Offset,
            offset_uniform_spec(),
            device,
            &self.offset_uniform_bind_group_layout,
            bytemuck::bytes_of(&uniforms),
        );

        let texture_bind_group = source.get_or_create_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            &self.effect_linear_sampler,
        );

        let mut pass = recorder.begin_color_pass(
            "Offset Effect Pass",
            dest_view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        );

        pass.set_pipeline(self.offset_pipeline(device));
        pass.set_bind_group(0, texture_bind_group, &[]);
        pass.set_bind_group(1, &uniform.bind_group, &[uniform.offset]);
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
                source_logical_size: None,
            },
        )
    }

    pub(crate) fn prepare_shader_batch_draws<'a, C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        items: &[ShaderCompositeBatchItem<'a>],
    ) -> Option<Vec<PreparedShaderDraw<'a>>> {
        let mut prepared = Vec::with_capacity(items.len());
        for item in items {
            let variants = shader_draw_variants(item.shader);
            for variant in variants {
                self.shader_cache.get_or_create(
                    device,
                    item.shader,
                    self.surface_format,
                    &self.effect_texture_bind_group_layout,
                    &self.effect_uniform_bind_group_layout,
                    RuntimeShaderPipelineMode::PremultipliedSrcOver,
                    *variant,
                )?;
            }
            let mut padded = item.shader.uniforms_padded();
            let (dest_x, dest_y, _, _) = item.dest_viewport;
            let mask = item.rounded_mask.map(|mask| RoundedCompositeMask {
                rect: [
                    mask.rect[0] - dest_x,
                    mask.rect[1] - dest_y,
                    mask.rect[2],
                    mask.rect[3],
                ],
                radii: mask.radii,
            });
            ReservedShaderUniforms {
                layer_pixel_rect: item.layer_pixel_rect,
                source_region: item.source_region,
                substrate_regions: item.substrate_regions,
                mask,
                logical_size: item.source_logical_size,
                alpha: item.alpha,
            }
            .write(&mut padded);
            let uniform = recorder.upload_uniform(
                UploadAllocatorId::EffectUniform,
                effect_uniform_spec(),
                device,
                &self.effect_uniform_bind_group_layout,
                bytemuck::cast_slice(&padded),
            );

            let texture_bind_group = item.source.get_or_create_bind_group(
                device,
                &self.effect_texture_bind_group_layout,
                &self.effect_linear_sampler,
            );
            prepared.push(PreparedShaderDraw {
                shader: item.shader,
                texture_bind_group,
                uniform,
                scissor: item.scissor,
                dest_viewport: item.dest_viewport,
                variants,
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
    ) {
        pass.set_bind_group(0, draw.texture_bind_group, &[]);
        pass.set_bind_group(1, &draw.uniform.bind_group, &[draw.uniform.offset]);
        let (x, y, width, height) = draw.dest_viewport;
        pass.set_viewport(x, y, width, height, 0.0, 1.0);
        let scissor = draw.scissor.unwrap_or((0, 0, viewport.0, viewport.1));
        pass.set_scissor_rect(scissor.0, scissor.1, scissor.2, scissor.3);
        self.debug_shader_pixels
            .set(self.debug_shader_pixels.get() + shaded_pixels((x, y, width, height), scissor));
        for variant in draw.variants {
            let pipeline = self
                .shader_cache
                .get_or_create(
                    device,
                    draw.shader,
                    self.surface_format,
                    &self.effect_texture_bind_group_layout,
                    &self.effect_uniform_bind_group_layout,
                    RuntimeShaderPipelineMode::PremultipliedSrcOver,
                    *variant,
                )
                .expect("shader batch pipeline was prevalidated");
            pass.set_pipeline(pipeline);
            pass.draw(0..4, 0..1);
        }
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
        ReservedShaderUniforms {
            layer_pixel_rect,
            substrate_regions: [None; MAX_SUBSTRATES],
            source_region: None,
            mask: None,
            logical_size: options.source_logical_size,
            alpha: 1.0,
        }
        .write(&mut padded);
        let uniform = recorder.upload_uniform(
            UploadAllocatorId::EffectUniform,
            effect_uniform_spec(),
            device,
            &self.effect_uniform_bind_group_layout,
            bytemuck::cast_slice(&padded),
        );

        if self
            .shader_cache
            .get_or_create(
                device,
                shader,
                self.surface_format,
                &self.effect_texture_bind_group_layout,
                &self.effect_uniform_bind_group_layout,
                options.pipeline_mode,
                ShaderDrawVariant::Whole,
            )
            .is_none()
        {
            return false;
        }
        let Some(pipeline) = self.shader_cache.get_or_create(
            device,
            shader,
            self.surface_format,
            &self.effect_texture_bind_group_layout,
            &self.effect_uniform_bind_group_layout,
            options.pipeline_mode,
            ShaderDrawVariant::Whole,
        ) else {
            return false;
        };

        let texture_bind_group = source.get_or_create_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            &self.effect_linear_sampler,
        );

        let mut pass = recorder.begin_color_pass("Shader Effect Pass", dest_view, options.load_op);

        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, texture_bind_group, &[]);
        pass.set_bind_group(1, &uniform.bind_group, &[uniform.offset]);
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
                let passes = self.encode_blur_scissored_ping_pong_passes(
                    recorder,
                    device,
                    source,
                    intermediate,
                    (dest_view, (source.width, source.height)),
                    *radius_x,
                    *radius_y,
                    *edge_treatment,
                    None,
                );
                self.record_blur_pass();
                Ok(passes)
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

    fn composite_pass_uniforms(options: CompositePassOptions) -> BlitUniforms {
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
        }
    }

    #[allow(clippy::too_many_arguments)]
    /// Writes `source` into `dest_view` whole and bilinearly, every fetch
    /// held to `source`'s texel centres: a downscaled result brought to
    /// `dest_view`'s size.
    pub(crate) fn encode_upscale_pass<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
    ) {
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
                source_viewport: Some((0.0, 0.0, source.width as f32, source.height as f32)),
                sample_mode: CompositeSampleMode::Linear,
            },
        );
        self.record_composite_pass();
    }

    fn encode_composite_to_view_pass<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
        options: CompositePassOptions,
    ) {
        let uniforms = Self::composite_pass_uniforms(options);
        let sampler = &self.effect_linear_sampler;
        let texture_bind_group = source.get_or_create_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            sampler,
        );
        let uniform = recorder.upload_uniform(
            UploadAllocatorId::Blit,
            blit_uniform_spec(),
            device,
            &self.blit_uniform_bind_group_layout,
            bytemuck::bytes_of(&uniforms),
        );

        let mut pass = recorder.begin_color_pass("Blit Composite Pass", dest_view, options.load_op);

        pass.set_pipeline(self.blit_pipeline(device, options.blend_mode));
        pass.set_bind_group(0, texture_bind_group, &[]);
        pass.set_bind_group(1, &uniform.bind_group, &[uniform.offset]);
        if let Some((x, y, w, h)) = options.scissor {
            pass.set_scissor_rect(x, y, w, h);
        }
        pass.draw(0..4, 0..1);
    }

    pub(crate) fn prepare_composite_batch_draws<'a, C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        load_op: wgpu::LoadOp<wgpu::Color>,
        items: &[CompositeBatchItem<'a>],
    ) -> Vec<PreparedCompositeDraw<'a>> {
        let mut prepared = Vec::with_capacity(items.len());
        for item in items {
            self.blit_pipeline(device, item.blend_mode);
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
            let uniforms = Self::composite_pass_uniforms(options);
            let sampler = &self.effect_linear_sampler;
            let texture_bind_group = item.source.get_or_create_bind_group(
                device,
                &self.effect_texture_bind_group_layout,
                sampler,
            );
            let uniform = recorder.upload_uniform(
                UploadAllocatorId::Blit,
                blit_uniform_spec(),
                device,
                &self.blit_uniform_bind_group_layout,
                bytemuck::bytes_of(&uniforms),
            );
            prepared.push(PreparedCompositeDraw {
                texture_bind_group,
                uniform,
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
    ) {
        pass.set_pipeline(self.initialized_blit_pipeline(draw.blend_mode));
        pass.set_bind_group(0, draw.texture_bind_group, &[]);
        pass.set_bind_group(1, &draw.uniform.bind_group, &[draw.uniform.offset]);
        if let Some((x, y, w, h)) = draw.scissor {
            pass.set_scissor_rect(x, y, w, h);
        } else {
            pass.set_scissor_rect(0, 0, viewport.0, viewport.1);
        }
        pass.draw(0..4, 0..1);
    }

    pub(crate) fn prepare_projective_composite_draw<'a, C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        item: &ProjectiveCompositeItem<'a>,
    ) -> PreparedProjectiveComposite<'a> {
        self.projective_blit_pipeline(device, item.blend_mode);
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
        let sampler = &self.effect_linear_sampler;
        let texture_bind_group = item.source.get_or_create_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            sampler,
        );
        let vertices = recorder.upload_buffer(
            projective_blit_vertex_spec(),
            device,
            bytemuck::cast_slice(&vertices),
        );
        let uniform = recorder.upload_uniform(
            UploadAllocatorId::ProjectiveBlitUniform,
            projective_blit_uniform_spec(),
            device,
            &self.blit_uniform_bind_group_layout,
            bytemuck::bytes_of(&uniforms),
        );
        PreparedProjectiveComposite {
            texture_bind_group,
            uniform,
            vertices,
            blend_mode: item.blend_mode,
        }
    }

    pub(crate) fn draw_prepared_projective_composite(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        viewport: (u32, u32),
        draw: &PreparedProjectiveComposite<'_>,
    ) {
        pass.set_pipeline(self.initialized_projective_blit_pipeline(draw.blend_mode));
        pass.set_bind_group(0, draw.texture_bind_group, &[]);
        pass.set_bind_group(1, &draw.uniform.bind_group, &[draw.uniform.offset]);
        pass.set_vertex_buffer(0, draw.vertices.slice());
        pass.set_scissor_rect(0, 0, viewport.0, viewport.1);
        pass.draw(0..4, 0..1);
    }
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
        CompositeSampleMode::Nearest => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::BlurUniforms;

    #[test]
    fn blur_uniforms_use_vec4_packing_for_gl_backends() {
        assert_eq!(std::mem::size_of::<BlurUniforms>(), 64);
        assert_eq!(std::mem::offset_of!(BlurUniforms, direction_and_radius), 0);
        assert_eq!(
            std::mem::offset_of!(BlurUniforms, texture_size_and_tile_mode),
            16
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_wide_blur_gets_a_smaller_scratch() {
        use super::blur_scratch_size;
        assert_eq!(blur_scratch_size(2.0, 2.0, 1080, 400), (1080, 400));
        assert_eq!(blur_scratch_size(8.0, 8.0, 1080, 400), (540, 200));
        assert_eq!(blur_scratch_size(30.0, 30.0, 1080, 400), (270, 100));
        assert_eq!(blur_scratch_size(30.0, 30.0, 1081, 401), (271, 101));
    }

    #[test]
    fn a_small_target_keeps_its_full_scratch() {
        use super::blur_scratch_size;
        assert_eq!(blur_scratch_size(30.0, 30.0, 40, 40), (20, 20));
        assert_eq!(blur_scratch_size(30.0, 30.0, 20, 20), (20, 20));
    }

    #[test]
    fn a_scissor_shrinks_with_the_scratch() {
        use super::scaled_scissor;
        assert_eq!(
            scaled_scissor(Some((10, 20, 100, 200)), 2.0, 2.0, 540, 200),
            Some((5, 10, 50, 100))
        );
        assert_eq!(
            scaled_scissor(Some((9, 9, 10, 10)), 4.0, 4.0, 270, 100),
            Some((2, 2, 3, 3))
        );
        assert_eq!(
            scaled_scissor(Some((0, 0, 4000, 4000)), 4.0, 4.0, 270, 100),
            Some((0, 0, 270, 100))
        );
        assert_eq!(scaled_scissor(None, 4.0, 4.0, 270, 100), None);
    }
}
