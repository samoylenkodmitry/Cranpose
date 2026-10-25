use std::cell::{Cell, RefCell};

use cranpose_core::collections::bounded_lru::BoundedLruCache;
use cranpose_render_common::geometry::{BLUR_TAP_PAIRS, BlurKernel, blur_scratch_block};
use cranpose_ui_graphics::{
    BlendMode, DrawSpecialization, MAX_SUBSTRATES, RenderEffect, RuntimeShader, SubstrateSpec,
    TileMode,
};
use smallvec::SmallVec;

use crate::{
    fixed_pipeline::{FixedPipeline, FixedPipelineJob},
    frame_graph::{
        BufferUpload, FrameCommandRecorder, FrameCommandStats, FrameTextureDescriptor,
        TextureRegionCopy, UniformUpload, UploadAllocatorId, UploadAllocatorSpec, copy_compatible,
    },
    glass_split::split_scissors,
    gpu_stats::FrameStats,
    lazy_resource::Pending,
    offscreen::{OffscreenPool, OffscreenTarget},
    pipeline_compiler::{CompileLane, PipelineCompiler},
    pipeline_recorder::PipelineRecorder,
    scissor::{read_scissor, scaled_scissor, widened_scissor},
    shader_cache::{
        RuntimeShaderPipelineMode, ShaderDrawVariant, ShaderPipelineCache, ShaderPipelineFit,
        shader_specialization_enabled,
    },
    shaders,
    shared_shader::SharedShader,
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

const MAX_BLUR_KERNEL_CACHE_ITEMS: usize = 32;
const BLUR_TILE_MODES: [TileMode; 4] = [
    TileMode::Clamp,
    TileMode::Repeated,
    TileMode::Mirror,
    TileMode::Decal,
];
const BLUR_LABELS: [&str; BLUR_TILE_MODES.len()] = [
    "effect/blur/clamp",
    "effect/blur/repeated",
    "effect/blur/mirror",
    "effect/blur/decal",
];
const BLUR_DOWNSAMPLE_LABELS: [[&str; BLUR_DOWNSAMPLE_BLOCKS.len()]; BLUR_TILE_MODES.len()] = [
    [
        "effect/blur-downsample/clamp/2",
        "effect/blur-downsample/clamp/4",
    ],
    [
        "effect/blur-downsample/repeated/2",
        "effect/blur-downsample/repeated/4",
    ],
    [
        "effect/blur-downsample/mirror/2",
        "effect/blur-downsample/mirror/4",
    ],
    [
        "effect/blur-downsample/decal/2",
        "effect/blur-downsample/decal/4",
    ],
];

// Where a blur pass draws: its label and uniform pass, the view and size it
// writes, and how the pass starts that view.
#[derive(Clone, Copy)]
struct BlurPassTarget<'a> {
    label: &'static str,
    pass_id: UploadAllocatorId,
    view: &'a wgpu::TextureView,
    size: (u32, u32),
    load_op: wgpu::LoadOp<wgpu::Color>,
}

impl<'a> BlurPassTarget<'a> {
    fn cleared(
        (label, pass_id): (&'static str, UploadAllocatorId),
        view: &'a wgpu::TextureView,
        size: (u32, u32),
    ) -> Self {
        Self {
            label,
            pass_id,
            view,
            size,
            load_op: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        }
    }
}

// Where a blur writes: the view, its size, and the part of it the blur may
// change, or all of it.
#[derive(Clone, Copy)]
pub(crate) struct BlurDest<'a> {
    pub(crate) view: &'a wgpu::TextureView,
    pub(crate) size: (u32, u32),
    pub(crate) scissor: Option<(u32, u32, u32, u32)>,
}

// A blur's radii in pixels and what it samples past the source's edges.
#[derive(Clone, Copy)]
pub(crate) struct BlurSpec {
    pub(crate) radius_x: f32,
    pub(crate) radius_y: f32,
    pub(crate) tile_mode: TileMode,
}

impl BlurSpec {
    pub(crate) fn uniform(radius: f32, tile_mode: TileMode) -> Self {
        Self {
            radius_x: radius,
            radius_y: radius,
            tile_mode,
        }
    }
}

// What an effect reads and writes: its source surface, the view it draws
// into and the layer's rect in pixels.
#[derive(Clone, Copy)]
pub(crate) struct EffectTarget<'a> {
    pub(crate) source: &'a OffscreenTarget,
    pub(crate) dest_view: &'a wgpu::TextureView,
    pub(crate) layer_pixel_rect: [f32; 4],
}

// Where the substrate means land, and whether that surface keeps what it
// holds or starts clear.
#[derive(Clone, Copy)]
enum MeanOutput<'a> {
    Keep(&'a OffscreenTarget),
    Clear(&'a OffscreenTarget),
}

pub(crate) struct EffectRenderer {
    offscreen_pool: OffscreenPool,
    pub shader_cache: ShaderPipelineCache,
    pipeline_cache: Option<wgpu::PipelineCache>,

    blur_shader: SharedShader,
    blur_pipelines: [FixedPipeline; BLUR_TILE_MODES.len()],
    blur_downsample_pipelines:
        [[FixedPipeline; BLUR_DOWNSAMPLE_BLOCKS.len()]; BLUR_TILE_MODES.len()],
    blur_mean_pipeline: FixedPipeline,
    recorder: PipelineRecorder,
    /// One bit per tile mode whose blur family is queued for warm-up.
    blur_families_queued: Cell<u8>,
    /// Whether a composite was left out this frame while its pipeline
    /// compiles (see [`Self::take_skipped_draw`]).
    skipped_draw: Cell<bool>,
    compiler: PipelineCompiler,
    blur_uniform_bind_group_layout: wgpu::BindGroupLayout,
    blur_prepared_draws: Vec<PreparedBlurDraw>,
    blur_kernels: RefCell<BoundedLruCache<u32, BlurKernel>>,

    offset_shader: SharedShader,
    offset_pipeline: FixedPipeline,
    offset_uniform_bind_group_layout: wgpu::BindGroupLayout,

    blit_shader: SharedShader,
    blit_pipeline: [FixedPipeline; CompositeVariant::COUNT],
    blit_pipeline_src: [FixedPipeline; CompositeVariant::COUNT],
    blit_pipeline_dst_out: [FixedPipeline; CompositeVariant::COUNT],
    blit_uniform_bind_group_layout: wgpu::BindGroupLayout,
    projective_blit_shader: SharedShader,
    projective_blit_pipeline: [FixedPipeline; CompositeVariant::COUNT],
    projective_blit_pipeline_src: [FixedPipeline; CompositeVariant::COUNT],
    projective_blit_pipeline_dst_out: [FixedPipeline; CompositeVariant::COUNT],

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
    pub(crate) debug_glass_rasterized_pixels: Cell<u64>,
    pub(crate) debug_blur_pixels: Cell<u64>,
    debug_shader_fallback_draws: Cell<u32>,
    debug_shader_specialized_draws: Cell<u32>,
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
        RenderEffect::Offset { .. } => {}
        RenderEffect::Shader { shader } => {
            if !shader.substrates().is_empty() {
                let layout = ChainedSubstrates::new(shader, width, height);
                for _ in 0..2 {
                    let descriptor = FrameTextureDescriptor::render_attachment(
                        "Chained Shader Substrates",
                        layout.width,
                        layout.height,
                        format,
                    );
                    let target = recorder.acquire_transient_offscreen(device, descriptor);
                    targets.push(descriptor, target);
                }
            }
        }
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
    pairs: [[f32; 4]; BLUR_TAP_PAIRS],
    kernel: [f32; 4],
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
    dest_clip: [f32; 4],
    source_viewport: [f32; 4],
    /// `x`: the quad's depth, its place in a pass that lays opaque
    /// interiors down first.
    depth: [f32; 4],
}

/// Where a blit lands: `rect` (x, y, width, height) in the pixels of a
/// target `target` pixels in size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BlitDest {
    pub(crate) rect: (f32, f32, f32, f32),
    pub(crate) target: (u32, u32),
}

impl BlitDest {
    /// The rect's left, top, right and bottom in clip space, where the
    /// blit's vertex stage places its quad.
    fn clip(self) -> [f32; 4] {
        let (x, y, width, height) = self.rect;
        let (target_width, target_height) = (self.target.0 as f32, self.target.1 as f32);
        [
            x / target_width * 2.0 - 1.0,
            1.0 - y / target_height * 2.0,
            (x + width) / target_width * 2.0 - 1.0,
            1.0 - (y + height) / target_height * 2.0,
        ]
    }
}

/// The whole target in clip space: a blit with no destination rect.
const WHOLE_TARGET_CLIP: [f32; 4] = [-1.0, 1.0, 1.0, -1.0];

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
    source_region: [f32; 4],
    /// `x`: the quad's depth, its place in a pass that lays opaque
    /// interiors down first.
    depth: [f32; 4],
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
    UploadAllocatorSpec::uniform(buffer, bind_group, size_of::<BlurUniforms>() as u64)
}

#[derive(Clone, Copy)]
struct BlurDraw {
    source: BlurSource,
    uniforms: BlurUniformSpec,
    filter: BlurFilter,
    scissor: Option<(u32, u32, u32, u32)>,
}

#[derive(Clone, Copy)]
enum BlurSource {
    Primary,
    Secondary,
}

struct BlurSourceSet<'a> {
    primary: &'a OffscreenTarget,
    secondary: &'a OffscreenTarget,
}

impl<'a> BlurSourceSet<'a> {
    fn single(target: &'a OffscreenTarget) -> Self {
        BlurSourceSet {
            primary: target,
            secondary: target,
        }
    }

    fn resolve(&self, source: BlurSource) -> &OffscreenTarget {
        match source {
            BlurSource::Primary => self.primary,
            BlurSource::Secondary => self.secondary,
        }
    }
}

struct PreparedBlurDraw {
    source: BlurSource,
    uniform: UniformUpload,
    pipeline: BlurPipeline,
    scissor: Option<(u32, u32, u32, u32)>,
}

#[derive(Clone, Copy)]
struct BlurUniformSpec {
    horizontal: bool,
    sampled: (u32, u32),
    source: [f32; 4],
    dest: [f32; 4],
    radius: (f32, f32),
    tile_mode: TileMode,
}

#[derive(Clone, Copy)]
enum BlurFilter {
    Kernel,
    Downsample(u32),
    Mean,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlurPipeline {
    Mean,
    Downsample { block: u32, tile_mode: usize },
    Kernel { tile_mode: usize },
}

impl BlurPipeline {
    /// The tile mode whose blur family this pipeline belongs to: its
    /// kernel and its downsample of each block. The mean has none.
    fn family(self) -> Option<usize> {
        match self {
            Self::Mean => None,
            Self::Downsample { tile_mode, .. } | Self::Kernel { tile_mode } => Some(tile_mode),
        }
    }
}

/// The pipelines of the blur family of `tile_mode`.
fn blur_family(tile_mode: usize) -> impl Iterator<Item = BlurPipeline> {
    std::iter::once(BlurPipeline::Kernel { tile_mode }).chain(
        BLUR_DOWNSAMPLE_BLOCKS
            .into_iter()
            .map(move |block| BlurPipeline::Downsample { block, tile_mode }),
    )
}

impl BlurDraw {
    fn uniforms(&self, renderer: &EffectRenderer) -> BlurUniforms {
        let spec = self.uniforms;
        renderer.blur_uniforms(
            spec.horizontal,
            spec.sampled,
            spec.source,
            spec.dest,
            spec.radius,
            spec.tile_mode,
        )
    }

    fn pipeline(&self) -> BlurPipeline {
        let tile_mode = tile_mode_uniform_value(self.uniforms.tile_mode) as usize;
        match self.filter {
            BlurFilter::Downsample(block) => BlurPipeline::Downsample { block, tile_mode },
            BlurFilter::Kernel => BlurPipeline::Kernel { tile_mode },
            BlurFilter::Mean => BlurPipeline::Mean,
        }
    }
}

fn offset_uniform_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::uniform(
        "Offset Uniform Buffer",
        "Offset Uniform Bind Group",
        size_of::<OffsetUniforms>() as u64,
    )
}

fn blit_uniform_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::uniform(
        "Blit Uniform Buffer",
        "Blit Uniform Bind Group",
        size_of::<BlitUniforms>() as u64,
    )
}

fn projective_blit_uniform_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::uniform(
        "Projective Blit Uniform Buffer",
        "Projective Blit Uniform Bind Group",
        size_of::<ProjectiveBlitUniforms>() as u64,
    )
}

fn projective_blit_vertex_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::vertex(
        "Projective Blit Vertex Buffer",
        (size_of::<ProjectiveBlitVertex>() * 5) as u64,
    )
}

fn effect_uniform_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::uniform(
        "Effect Uniform Buffer",
        "Effect Uniform Bind Group",
        (RuntimeShader::MAX_UNIFORMS * size_of::<f32>()) as u64,
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
    dest_viewport: Option<BlitDest>,
    source_viewport: Option<(f32, f32, f32, f32)>,
    sample_mode: CompositeSampleMode,
}

impl CompositePassOptions {
    fn unmasked_nearest(self) -> bool {
        unmasked_nearest(self.sample_mode, self.rounded_mask.is_some())
    }
}

/// Whether a blit samples nearest texels through no mask, which its own
/// specialized pipeline draws.
fn unmasked_nearest(sample_mode: CompositeSampleMode, masked: bool) -> bool {
    matches!(sample_mode, CompositeSampleMode::Nearest)
        && !masked
        && shader_specialization_enabled()
}

/// What sets a composite pipeline apart beside its blend: its specialized
/// sampling (a blit's nearest texels through no mask, a projective blit's
/// bilinear over the surface's own texels), and whether the pass it draws
/// into has a depth buffer its opaque interiors fill first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CompositeVariant {
    sampling: bool,
    depth: bool,
}

impl CompositeVariant {
    const COUNT: usize = 4;

    const ALL: [Self; Self::COUNT] = [
        Self::new(false, false),
        Self::new(true, false),
        Self::new(false, true),
        Self::new(true, true),
    ];

    const fn new(sampling: bool, depth: bool) -> Self {
        Self { sampling, depth }
    }

    const fn index(self) -> usize {
        self.sampling as usize | (self.depth as usize) << 1
    }
}

#[derive(Clone, Copy)]
struct ShaderPassOptions {
    load_op: wgpu::LoadOp<wgpu::Color>,
    scissor: Option<(u32, u32, u32, u32)>,
    pipeline_mode: RuntimeShaderPipelineMode,
    source_logical_size: Option<(f32, f32)>,
    source_region: Option<(f32, f32, f32, f32)>,
    substrate_regions: SubstrateRegions,
}

/// A runtime shader drawn into a pass over `dest_viewport`, placed in the
/// pass's target by `placement`, reading
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
    pub(crate) placement: QuadPlacement,
}

/// Where a runtime shader's effect rect lands in a pass: the viewport, the
/// rect held to the pass's target, and the placement the prelude's vertex
/// stage reads to span the whole rect from that viewport. Safari's WebGPU
/// rejects a viewport that reaches past its attachment and drops the frame's
/// command buffer with it, so a rect at the target's edge cannot be the
/// viewport itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct QuadPlacement {
    viewport: (f32, f32, f32, f32),
    uniform: [f32; 4],
}

impl QuadPlacement {
    /// The placement of `rect` in a target of `target` pixels; `None` when
    /// no part of it lies on the target.
    pub(crate) fn in_target(rect: (f32, f32, f32, f32), target: (u32, u32)) -> Option<Self> {
        let (x, y, width, height) = rect;
        let left = x.max(0.0);
        let top = y.max(0.0);
        let right = (x + width).min(target.0 as f32);
        let bottom = (y + height).min(target.1 as f32);
        if right <= left || bottom <= top {
            return None;
        }
        let viewport = (left, top, right - left, bottom - top);
        if viewport == rect {
            return Some(Self {
                viewport,
                uniform: [0.0; 4],
            });
        }
        let (view_width, view_height) = (viewport.2, viewport.3);
        Some(Self {
            viewport,
            uniform: [
                width / view_width - 1.0,
                height / view_height - 1.0,
                (2.0 * (x - left) + width) / view_width - 1.0,
                1.0 - (2.0 * (y - top) + height) / view_height,
            ],
        })
    }
}

/// The regions of a shader's substrates in its input texture, in slot
/// order; none where the shader declared fewer or the stage packed none.
pub(crate) type SubstrateRegions = [Option<(f32, f32, f32, f32)>; MAX_SUBSTRATES];

/// Fills the renderer-reserved uniform slots of a runtime shader; see
/// `RuntimeShader`'s slot table.
struct ReservedShaderUniforms {
    placement: [f32; 4],
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
            (RuntimeShader::QUAD_PLACEMENT_UNIFORM, self.placement),
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

/// The device pixels of the layer an effect draws, however much of it the
/// target clips: it picks the draw's specialization, which must not flip
/// while the layer scrolls across the target's edge.
pub(crate) fn layer_pixels(layer_pixel_rect: [f32; 4]) -> u64 {
    let [_, _, width, height] = layer_pixel_rect;
    (width.max(0.0).round() as u64).saturating_mul(height.max(0.0).round() as u64)
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

#[derive(Clone, Copy)]
pub(crate) struct BlurRegion {
    pub(crate) source: (u32, u32, u32, u32),
    pub(crate) scratch: (u32, u32, u32, u32),
    pub(crate) dest: (u32, u32, u32, u32),
    pub(crate) blur: BlurSpec,
    pub(crate) read: Option<(u32, u32, u32, u32)>,
}

#[derive(Clone, Copy)]
pub(crate) struct SubstrateRegion {
    pub(crate) source: (u32, u32, u32, u32),
    pub(crate) scratch: (u32, u32, u32, u32),
    pub(crate) dest: (u32, u32, u32, u32),
    pub(crate) average: SubstrateAverage,
    pub(crate) read: Option<(u32, u32, u32, u32)>,
}

#[derive(Clone, Copy)]
pub(crate) enum SubstrateAverage {
    Block(u32),
    Mean,
}

struct ChainedSubstrates {
    width: u32,
    height: u32,
    regions: SubstrateRegions,
    blurs: Vec<BlurRegion>,
    averages: Vec<SubstrateRegion>,
}

impl ChainedSubstrates {
    fn new(shader: &RuntimeShader, width: u32, height: u32) -> Self {
        let mut layout = Self {
            width,
            height,
            regions: [None; MAX_SUBSTRATES],
            blurs: Vec::new(),
            averages: Vec::new(),
        };
        for (index, spec) in shader.substrates().iter().enumerate() {
            let (sw, sh) = match *spec {
                SubstrateSpec::Mean => (1, height),
                SubstrateSpec::Average { block } => (width.div_ceil(block), height.div_ceil(block)),
                SubstrateSpec::Blur { radius_px } => {
                    substrate_scratch_size(radius_px, width, height)
                }
            };
            let region = (0, layout.height, sw, sh);
            let source = (0, 0, width, height);
            layout.regions[index] = Some(if matches!(spec, SubstrateSpec::Mean) {
                (0.0, layout.height as f32, 1.0, 1.0)
            } else {
                (0.0, layout.height as f32, sw as f32, sh as f32)
            });
            layout.height += sh;
            match *spec {
                SubstrateSpec::Blur { radius_px } => layout.blurs.push(BlurRegion {
                    source,
                    scratch: region,
                    dest: region,
                    blur: BlurSpec::uniform(radius_px, TileMode::Clamp),
                    read: None,
                }),
                SubstrateSpec::Mean | SubstrateSpec::Average { .. } => {
                    layout.averages.push(SubstrateRegion {
                        source,
                        scratch: region,
                        dest: if matches!(spec, SubstrateSpec::Mean) {
                            (region.0, region.1, 1, 1)
                        } else {
                            region
                        },
                        read: None,
                        average: match *spec {
                            SubstrateSpec::Average { block } => SubstrateAverage::Block(block),
                            _ => SubstrateAverage::Mean,
                        },
                    });
                }
            }
        }
        layout
    }

    fn passes(&self) -> u32 {
        let blurs = !self.blurs.is_empty();
        let means = self
            .averages
            .iter()
            .any(|s| matches!(s.average, SubstrateAverage::Mean));
        2 * u32::from(blurs)
            + 2 * u32::from(means && !blurs)
            + u32::from(
                self.averages
                    .iter()
                    .any(|s| matches!(s.average, SubstrateAverage::Block(_)))
                    || self
                        .blurs
                        .iter()
                        .any(|b| blur_block(b.source, b.scratch) > 1),
            )
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct EffectReads {
    pub(crate) output: Option<(u32, u32, u32, u32)>,
    pub(crate) shader_input: Option<(u32, u32, u32, u32)>,
}

fn kernel_margin((radius_x, radius_y): (f32, f32)) -> (u32, u32) {
    (radius_x.ceil() as u32 + 1, radius_y.ceil() as u32 + 1)
}

pub(crate) struct AtlasSideWork<'a> {
    pub(crate) blurs: &'a [BlurRegion],
    pub(crate) averages: &'a [SubstrateRegion],
    pub(crate) blur_output: Option<&'a OffscreenTarget>,
}

impl BlurRegion {
    fn scratch_radius(&self) -> (f32, f32) {
        (
            self.blur.radius_x * self.scratch.2 as f32 / self.source.2.max(1) as f32,
            self.blur.radius_y * self.scratch.3 as f32 / self.source.3.max(1) as f32,
        )
    }

    fn pass_scissor(&self, steps: (u32, u32)) -> (u32, u32, u32, u32) {
        let (radius_x, radius_y) = kernel_margin(self.scratch_radius());
        read_scissor(
            self.source,
            self.scratch,
            self.read,
            (radius_x * steps.0, radius_y * steps.1),
            self.blur.tile_mode,
        )
    }
}

impl SubstrateRegion {
    fn pass_scissor(&self) -> (u32, u32, u32, u32) {
        read_scissor(
            self.source,
            self.scratch,
            self.read,
            (1, 1),
            TileMode::Clamp,
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
    pub(crate) dest_viewport: Option<BlitDest>,
    pub(crate) source_viewport: Option<(f32, f32, f32, f32)>,
    pub(crate) sample_mode: CompositeSampleMode,
    /// Its depth in a pass that lays opaque interiors down first, from its
    /// place in the pass's order: later interiors hide it.
    pub(crate) depth: Option<f32>,
}

pub(crate) struct PreparedCompositeDraw<'a> {
    texture_bind_group: &'a wgpu::BindGroup,
    uniform: UniformUpload,
    scissor: Option<(u32, u32, u32, u32)>,
    blend_mode: BlendMode,
    variant: CompositeVariant,
}

pub(crate) struct PreparedShaderDraw<'a> {
    shader: &'a RuntimeShader,
    specialization: DrawSpecialization<'a>,
    texture_bind_group: &'a wgpu::BindGroup,
    uniform: UniformUpload,
    scissor: Option<(u32, u32, u32, u32)>,
    dest_viewport: (f32, f32, f32, f32),
    viewport: (f32, f32, f32, f32),
    layer_pixel_rect: [f32; 4],
    pipelines: SmallVec<[(ShaderDrawVariant, wgpu::RenderPipeline); 2]>,
}

const WHOLE_DRAW: &[ShaderDrawVariant] = &[ShaderDrawVariant::Whole];
const SPLIT_DRAWS: &[ShaderDrawVariant] = &[ShaderDrawVariant::Interior, ShaderDrawVariant::Rim];

/// The draws a shader makes in a pass: its interior and its rim when its
/// specialization declared a split and the pipelines are specialized, else
/// the one draw.
fn shader_draw_variants(specialization: DrawSpecialization<'_>) -> &'static [ShaderDrawVariant] {
    if specialization.draw_split().is_some() && shader_specialization_enabled() {
        SPLIT_DRAWS
    } else {
        WHOLE_DRAW
    }
}

pub(crate) struct ProjectiveCompositeItem<'a> {
    pub source: &'a OffscreenTarget,
    pub source_region: Option<(f32, f32, f32, f32)>,
    pub viewport: (u32, u32),
    pub dest_strip: [[f32; 2]; 5],
    pub dest_corners: u32,
    pub inverse: [[f32; 3]; 3],
    pub alpha: f32,
    pub blend_mode: BlendMode,
    pub sample_mode: CompositeSampleMode,
    /// The clip of the layers above the child, in target pixels. A tilted
    /// child's quad reaches wherever its transform sends it; this is what
    /// keeps it inside the layer that clips it.
    pub scissor: Option<(u32, u32, u32, u32)>,
    /// Its depth in a pass that lays opaque interiors down first, from its
    /// place in the pass's order: later interiors hide it.
    pub depth: Option<f32>,
}

pub(crate) struct PreparedProjectiveComposite<'a> {
    texture_bind_group: &'a wgpu::BindGroup,
    uniform: UniformUpload,
    vertices: BufferUpload,
    vertex_count: u32,
    blend_mode: BlendMode,
    variant: CompositeVariant,
    scissor: Option<(u32, u32, u32, u32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompositeSampleMode {
    Linear,
    Nearest,
    Texels,
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

/// The framework's own runtime shader sources, which recorded pipelines are
/// rebuilt from.
#[cfg(not(target_arch = "wasm32"))]
fn framework_shader_sources() -> impl Iterator<Item = &'static str> {
    cranpose_ui_graphics::BUILTIN_RUNTIME_SHADER_SOURCES
        .into_iter()
        .chain([crate::pipeline::GPU_TEXT_BRUSH_EFFECT_SHADER])
}

// The shader a fixed pipeline runs: its module, its vertex and fragment
// entry points and its pipeline constants.
#[derive(Clone, Copy)]
struct ShaderEntry<'a> {
    shader: &'a SharedShader,
    entries: (&'static str, &'static str),
    constants: &'a [(&'static str, f64)],
}

fn fullscreen_pipeline_job(
    device: &wgpu::Device,
    cache: Option<&wgpu::PipelineCache>,
    label: &'static str,
    entry: ShaderEntry<'_>,
    surface_format: wgpu::TextureFormat,
    (blend, depth_stencil): (wgpu::BlendState, Option<wgpu::DepthStencilState>),
) -> FixedPipelineJob {
    let device = device.clone();
    let cache = cache.cloned();
    let shader = entry.shader.clone();
    let (vertex_entry, fragment_entry) = entry.entries;
    let constants = entry.constants.to_vec();
    Box::new(move || {
        let stages = shader.stages((vertex_entry, fragment_entry), &constants);
        crate::shared_shader::create_fullscreen_strip_pipeline(
            &device,
            cache.as_ref(),
            &format!("effect {label} entry={fragment_entry}"),
            label,
            shader.layout(),
            (&stages.vertex, &stages.fragment),
            (vertex_entry, fragment_entry),
            stages.constants,
            (
                wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                },
                depth_stencil,
            ),
        )
    })
}

fn projective_pipeline_job(
    device: &wgpu::Device,
    cache: Option<&wgpu::PipelineCache>,
    label: &'static str,
    shader: &SharedShader,
    surface_format: wgpu::TextureFormat,
    blend: wgpu::BlendState,
    variant: CompositeVariant,
) -> FixedPipelineJob {
    let device = device.clone();
    let cache = cache.cloned();
    let shader = shader.clone();
    let constants = [(
        "PROJECTIVE_TEXELS",
        if variant.sampling { 1.0 } else { 0.0 },
    )];
    Box::new(move || {
        let stages = shader.stages(("projective_blit_vs", "projective_blit_fs"), &constants);
        crate::render::create_render_pipeline_logged(
            &device,
            cache.as_ref(),
            &format!("effect {label}"),
            wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(shader.layout()),
                vertex: wgpu::VertexState {
                    module: &stages.vertex,
                    entry_point: Some("projective_blit_vs"),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: size_of::<ProjectiveBlitVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x2,
                        }],
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: stages.constants,
                        ..wgpu::PipelineCompilationOptions::default()
                    },
                },
                fragment: Some(wgpu::FragmentState {
                    module: &stages.fragment,
                    entry_point: Some("projective_blit_fs"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: stages.constants,
                        ..wgpu::PipelineCompilationOptions::default()
                    },
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: crate::render::overlay_depth_state(variant.depth),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            },
        )
    })
}

impl EffectRenderer {
    pub(crate) fn new(
        device: &wgpu::Device,
        compiler: PipelineCompiler,
        pipeline_cache: Option<wgpu::PipelineCache>,
        surface_format: wgpu::TextureFormat,
        adapter_backend: wgpu::Backend,
        recorder: PipelineRecorder,
    ) -> Self {
        let effect_texture_bind_group_layout = OffscreenPool::texture_bind_group_layout(device);
        let effect_uniform_bind_group_layout = OffscreenPool::uniform_bind_group_layout(device);

        let blur_uniform_bind_group_layout = crate::offscreen::dynamic_uniform_layout(
            device,
            "Blur Uniform Bind Group Layout",
            wgpu::ShaderStages::FRAGMENT,
        );

        let offset_uniform_bind_group_layout = crate::offscreen::dynamic_uniform_layout(
            device,
            "Offset Uniform Bind Group Layout",
            wgpu::ShaderStages::FRAGMENT,
        );

        let blit_uniform_bind_group_layout = crate::offscreen::dynamic_uniform_layout(
            device,
            "Blit Uniform Bind Group Layout",
            wgpu::ShaderStages::VERTEX_FRAGMENT,
        );

        let blur_shader = SharedShader::new(
            device,
            adapter_backend,
            "Blur Shader",
            shaders::BLUR,
            &[
                Some(&effect_texture_bind_group_layout),
                Some(&blur_uniform_bind_group_layout),
            ],
        );

        let blur_pipelines = BLUR_LABELS.map(FixedPipeline::new);
        let blur_downsample_pipelines =
            BLUR_DOWNSAMPLE_LABELS.map(|labels| labels.map(FixedPipeline::new));

        let offset_shader = SharedShader::new(
            device,
            adapter_backend,
            "Offset Shader",
            shaders::OFFSET,
            &[
                Some(&effect_texture_bind_group_layout),
                Some(&offset_uniform_bind_group_layout),
            ],
        );

        let offset_pipeline = FixedPipeline::new("effect/offset");

        let blit_layouts = [
            Some(&effect_texture_bind_group_layout),
            Some(&blit_uniform_bind_group_layout),
        ];
        let blit_shader = SharedShader::new(
            device,
            adapter_backend,
            "Blit Shader",
            shaders::BLIT,
            &blit_layouts,
        );

        let blit_pipeline = [
            FixedPipeline::new("effect/blit-src-over"),
            FixedPipeline::new("effect/blit-src-over-nearest-unmasked"),
            FixedPipeline::new("effect/blit-src-over-depth"),
            FixedPipeline::new("effect/blit-src-over-nearest-unmasked-depth"),
        ];
        let blit_pipeline_src = [
            FixedPipeline::new("effect/blit-src"),
            FixedPipeline::new("effect/blit-src-nearest-unmasked"),
            FixedPipeline::new("effect/blit-src-depth"),
            FixedPipeline::new("effect/blit-src-nearest-unmasked-depth"),
        ];
        let blit_pipeline_dst_out = [
            FixedPipeline::new("effect/blit-dst-out"),
            FixedPipeline::new("effect/blit-dst-out-nearest-unmasked"),
            FixedPipeline::new("effect/blit-dst-out-depth"),
            FixedPipeline::new("effect/blit-dst-out-nearest-unmasked-depth"),
        ];

        let projective_blit_shader = SharedShader::new(
            device,
            adapter_backend,
            "Projective Blit Shader",
            shaders::PROJECTIVE_BLIT,
            &blit_layouts,
        );
        let projective_blit_pipeline = [
            FixedPipeline::new("effect/projective-src-over"),
            FixedPipeline::new("effect/projective-src-over-texels"),
            FixedPipeline::new("effect/projective-src-over-depth"),
            FixedPipeline::new("effect/projective-src-over-texels-depth"),
        ];
        let projective_blit_pipeline_src = [
            FixedPipeline::new("effect/projective-src"),
            FixedPipeline::new("effect/projective-src-texels"),
            FixedPipeline::new("effect/projective-src-depth"),
            FixedPipeline::new("effect/projective-src-texels-depth"),
        ];
        let projective_blit_pipeline_dst_out = [
            FixedPipeline::new("effect/projective-dst-out"),
            FixedPipeline::new("effect/projective-dst-out-texels"),
            FixedPipeline::new("effect/projective-dst-out-depth"),
            FixedPipeline::new("effect/projective-dst-out-texels-depth"),
        ];

        let effect_linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Effect Linear Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let effect_layouts = [
            &effect_texture_bind_group_layout,
            &effect_uniform_bind_group_layout,
        ];
        Self {
            offscreen_pool: OffscreenPool::new(device, surface_format),
            shader_cache: ShaderPipelineCache::new(
                device,
                compiler.clone(),
                pipeline_cache.clone(),
                adapter_backend,
                surface_format,
                effect_layouts,
                recorder.clone(),
            ),
            recorder,
            pipeline_cache,
            blur_shader,
            blur_pipelines,
            blur_downsample_pipelines,
            blur_mean_pipeline: FixedPipeline::new("effect/mean"),
            blur_families_queued: Cell::new(0),
            compiler,
            blur_uniform_bind_group_layout,
            blur_prepared_draws: Vec::new(),
            blur_kernels: RefCell::new(BoundedLruCache::with_capacity_at_least_one(
                MAX_BLUR_KERNEL_CACHE_ITEMS,
            )),
            offset_shader,
            offset_pipeline,
            offset_uniform_bind_group_layout,
            blit_shader,
            blit_pipeline,
            blit_pipeline_src,
            blit_pipeline_dst_out,
            blit_uniform_bind_group_layout,
            projective_blit_shader,
            projective_blit_pipeline,
            projective_blit_pipeline_src,
            projective_blit_pipeline_dst_out,
            effect_texture_bind_group_layout,
            effect_uniform_bind_group_layout,
            effect_linear_sampler,
            surface_format,
            adapter_backend,
            debug_command_stats: Cell::new(FrameCommandStats::default()),
            skipped_draw: Cell::new(false),
            debug_blurs: Cell::new(0),
            debug_substrates: Cell::new(0),
            debug_composites: Cell::new(0),
            debug_effects: Cell::new(0),
            debug_shader_pixels: Cell::new(0),
            debug_glass_rasterized_pixels: Cell::new(0),
            debug_blur_pixels: Cell::new(0),
            debug_shader_fallback_draws: Cell::new(0),
            debug_shader_specialized_draws: Cell::new(0),
        }
    }

    /// Queues the recorded runtime shader pipelines of the framework's own
    /// shaders.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn warm_recorded_shaders<'a>(
        &mut self,
        records: impl IntoIterator<Item = &'a crate::pipeline_records::ShaderPipelineRecord>,
    ) {
        self.shader_cache
            .warm_recorded(records, framework_shader_sources());
    }

    /// Queues the runtime shader pipelines the last launch drew its first
    /// screen with, behind the stand-ins they share.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn warm_first_screen_shaders<'a>(
        &mut self,
        records: impl IntoIterator<Item = &'a crate::pipeline_records::ShaderPipelineRecord>,
    ) {
        self.shader_cache
            .warm_first_screen(records, framework_shader_sources());
    }

    /// Whether `effect` over a `source_size` source draws now without
    /// waiting for a pipeline to compile: its shaders' pipelines (its last
    /// one `composited` over what is beneath, else drawn in a pass of its
    /// own), and the blur pipelines its blurs and substrates draw with. What
    /// is missing is queued, so a later frame finds it.
    pub(crate) fn effect_ready(
        &mut self,
        device: &wgpu::Device,
        effect: &RenderEffect,
        layer_pixels: u64,
        source_size: (u32, u32),
        composited: bool,
    ) -> bool {
        match effect {
            RenderEffect::Shader { shader } => {
                let specialization = shader.draw_specialization(layer_pixels);
                let (mode, variants) = if composited {
                    (
                        RuntimeShaderPipelineMode::PremultipliedSrcOver,
                        shader_draw_variants(specialization),
                    )
                } else {
                    (RuntimeShaderPipelineMode::Replace, WHOLE_DRAW)
                };
                let mut ready = true;
                for &variant in variants {
                    ready &= self
                        .shader_cache
                        .ready_to_draw(shader, specialization, mode, variant);
                }
                for &substrate in shader.substrates() {
                    ready &= self.substrate_ready(device, substrate, source_size);
                }
                ready
            }
            RenderEffect::Blur {
                radius_x,
                radius_y,
                edge_treatment,
            } => self.blur_draws_now(device, *edge_treatment, source_size, *radius_x, *radius_y),
            RenderEffect::Offset { .. } => {
                self.offset_pipeline
                    .ready_or_queue(&self.compiler, self.adapter_backend, || {
                        self.offset_pipeline_job(device)
                    })
            }
            RenderEffect::Chain { first, second } => {
                let first = self.effect_ready(device, first, layer_pixels, source_size, false);
                let second =
                    self.effect_ready(device, second, layer_pixels, source_size, composited);
                first & second
            }
        }
    }

    /// Whether a blur by `radius_x` and `radius_y` of a `source_size` source
    /// draws now; what it misses is queued.
    pub(crate) fn blur_draws_now(
        &self,
        device: &wgpu::Device,
        edge_treatment: TileMode,
        source_size: (u32, u32),
        radius_x: f32,
        radius_y: f32,
    ) -> bool {
        if radius_x <= 0.0 && radius_y <= 0.0 {
            return true;
        }
        let tile_mode = BLUR_TILE_MODES
            .iter()
            .position(|mode| *mode == edge_treatment)
            .unwrap_or(0);
        let (width, height) = source_size;
        let scratch = blur_scratch_size(radius_x, radius_y, width, height);
        self.blur_pipelines_ready(device, tile_mode, source_size, scratch)
    }

    /// Whether the substrate `spec` of a `source_size` source draws now.
    fn substrate_ready(
        &self,
        device: &wgpu::Device,
        spec: SubstrateSpec,
        source_size: (u32, u32),
    ) -> bool {
        let clamp = 0;
        match spec {
            SubstrateSpec::Mean => self.blur_ready(device, BlurPipeline::Mean),
            SubstrateSpec::Average { block } => {
                block <= 1
                    || self.blur_ready(
                        device,
                        BlurPipeline::Downsample {
                            block,
                            tile_mode: clamp,
                        },
                    )
            }
            SubstrateSpec::Blur { radius_px } => {
                let (width, height) = source_size;
                let scratch = substrate_scratch_size(radius_px, width, height);
                self.blur_pipelines_ready(device, clamp, source_size, scratch)
            }
        }
    }

    /// Whether a blur from a `source_size` source into `scratch` draws now:
    /// its kernel, and the downsample of its block when the scratch is
    /// coarser than the source.
    fn blur_pipelines_ready(
        &self,
        device: &wgpu::Device,
        tile_mode: usize,
        (width, height): (u32, u32),
        (scratch_width, scratch_height): (u32, u32),
    ) -> bool {
        let block = blur_block((0, 0, width, height), (0, 0, scratch_width, scratch_height));
        let kernel = self.blur_ready(device, BlurPipeline::Kernel { tile_mode });
        let downsample =
            block <= 1 || self.blur_ready(device, BlurPipeline::Downsample { block, tile_mode });
        kernel & downsample
    }

    fn blur_ready(&self, device: &wgpu::Device, pipeline: BlurPipeline) -> bool {
        self.blur_resource(pipeline)
            .ready_or_queue(&self.compiler, self.adapter_backend, || {
                self.blur_job(device, pipeline)
            })
    }

    /// Queues the recorded pipelines a stand-in drew the first frame for.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn warm_after_first_frame(&mut self) {
        self.shader_cache.queue_after_first_frame();
    }

    /// Trusts the loaded driver cache with the pipelines `records` names,
    /// which this build's launches drew: a frame needing one builds it as a
    /// cache hit instead of drawing a placeholder.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn trust_cached(&mut self, records: &crate::pipeline_records::PipelineRecords) {
        self.shader_cache
            .trust_cached(records.shaders.iter().map(|shader| &shader.entry));
        let blurs = (0..BLUR_TILE_MODES.len())
            .flat_map(blur_family)
            .chain([BlurPipeline::Mean])
            .map(|blur| self.blur_resource(blur));
        for pipeline in blurs.chain([&self.offset_pipeline]) {
            if records
                .fixed
                .iter()
                .any(|fixed| fixed.entry == pipeline.label())
            {
                pipeline.trust_cached();
            }
        }
    }

    /// Hands `visit` each fixed effect pipeline with the job that builds it.
    pub(crate) fn for_each_fixed(
        &self,
        device: &wgpu::Device,
        visit: &mut dyn FnMut(&FixedPipeline, &dyn Fn() -> FixedPipelineJob),
    ) {
        let blurs = (0..BLUR_TILE_MODES.len())
            .flat_map(blur_family)
            .chain([BlurPipeline::Mean]);
        for blur in blurs {
            visit(self.blur_resource(blur), &|| self.blur_job(device, blur));
        }
        visit(&self.offset_pipeline, &|| self.offset_pipeline_job(device));
        for blend_mode in [BlendMode::SrcOver, BlendMode::Src, BlendMode::DstOut] {
            for variant in CompositeVariant::ALL {
                visit(self.blit_pipeline_target(blend_mode, variant).0, &|| {
                    self.blit_pipeline_job(device, blend_mode, variant)
                });
                visit(
                    self.projective_pipeline_target(blend_mode, variant).0,
                    &|| self.projective_pipeline_job(device, blend_mode, variant),
                );
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn compiler(&self) -> &PipelineCompiler {
        &self.compiler
    }

    pub(crate) fn warm_shaders(&mut self, warm_ups: &[cranpose_ui_graphics::ShaderWarmUp]) {
        for warm_up in warm_ups {
            self.shader_cache.warm(
                &warm_up.shader,
                RuntimeShaderPipelineMode::for_target(warm_up.target),
            );
        }
    }

    fn blur_pipeline_job(&self, device: &wgpu::Device, tile_mode: usize) -> FixedPipelineJob {
        fullscreen_pipeline_job(
            device,
            self.pipeline_cache.as_ref(),
            "Blur Pipeline",
            ShaderEntry {
                shader: &self.blur_shader,
                entries: (crate::render::FULLSCREEN_VS, "blur_fs"),
                constants: &[("BLUR_TILE_MODE", tile_mode as f64)],
            },
            self.surface_format,
            (wgpu::BlendState::REPLACE, None),
        )
    }

    fn blur_resource(&self, pipeline: BlurPipeline) -> &FixedPipeline {
        match pipeline {
            BlurPipeline::Mean => &self.blur_mean_pipeline,
            BlurPipeline::Kernel { tile_mode } => &self.blur_pipelines[tile_mode],
            BlurPipeline::Downsample { block, tile_mode } => {
                let index = BLUR_DOWNSAMPLE_BLOCKS
                    .iter()
                    .position(|candidate| *candidate == block)
                    .unwrap_or_else(|| {
                        panic!("a blur downsample block of {block}; the scratch is 2 or 4 to 1")
                    });
                &self.blur_downsample_pipelines[tile_mode][index]
            }
        }
    }

    fn blur_job(&self, device: &wgpu::Device, pipeline: BlurPipeline) -> FixedPipelineJob {
        match pipeline {
            BlurPipeline::Mean => self.mean_pipeline_job(device),
            BlurPipeline::Kernel { tile_mode } => self.blur_pipeline_job(device, tile_mode),
            BlurPipeline::Downsample { block, tile_mode } => {
                self.blur_downsample_pipeline_job(device, block, tile_mode)
            }
        }
    }

    /// The pipeline of a blur draw, built here if nothing has built it yet.
    /// The first draw of a tile mode's blur family queues the rest of the
    /// family for warm-up: the downsample block follows the radius, so an
    /// effect that blurs asks for the other block once its radius crosses
    /// a threshold, often mid-scroll.
    fn blur_draw_pipeline(
        &self,
        device: &wgpu::Device,
        pipeline: BlurPipeline,
    ) -> &wgpu::RenderPipeline {
        if let Some(tile_mode) = pipeline.family() {
            let bit = 1u8 << tile_mode;
            let queued = self.blur_families_queued.get();
            if queued & bit == 0 {
                self.blur_families_queued.set(queued | bit);
                for sibling in blur_family(tile_mode).filter(|sibling| *sibling != pipeline) {
                    self.blur_resource(sibling).queue(
                        &self.compiler,
                        CompileLane::WarmUp,
                        self.adapter_backend,
                        self.blur_job(device, sibling),
                    );
                }
            }
        }
        self.blur_resource(pipeline)
            .for_draw(&self.recorder, self.adapter_backend, || {
                self.blur_job(device, pipeline)()
            })
    }

    fn mean_pipeline_job(&self, device: &wgpu::Device) -> FixedPipelineJob {
        fullscreen_pipeline_job(
            device,
            self.pipeline_cache.as_ref(),
            "Mean Pipeline",
            ShaderEntry {
                shader: &self.blur_shader,
                entries: (crate::render::FULLSCREEN_VS, "blur_mean_fs"),
                constants: &[],
            },
            self.surface_format,
            (wgpu::BlendState::REPLACE, None),
        )
    }

    fn blur_downsample_pipeline_job(
        &self,
        device: &wgpu::Device,
        block: u32,
        tile_mode: usize,
    ) -> FixedPipelineJob {
        fullscreen_pipeline_job(
            device,
            self.pipeline_cache.as_ref(),
            "Blur Downsample Pipeline",
            ShaderEntry {
                shader: &self.blur_shader,
                entries: (crate::render::FULLSCREEN_VS, "blur_downsample_fs"),
                constants: &[
                    ("BLUR_BLOCK", f64::from(block)),
                    ("BLUR_TILE_MODE", tile_mode as f64),
                ],
            },
            self.surface_format,
            (wgpu::BlendState::REPLACE, None),
        )
    }

    fn offset_pipeline_job(&self, device: &wgpu::Device) -> FixedPipelineJob {
        fullscreen_pipeline_job(
            device,
            self.pipeline_cache.as_ref(),
            "Offset Pipeline",
            ShaderEntry {
                shader: &self.offset_shader,
                entries: (crate::render::FULLSCREEN_VS, "offset_fs"),
                constants: &[],
            },
            self.surface_format,
            (wgpu::BlendState::REPLACE, None),
        )
    }

    fn offset_pipeline(&self, device: &wgpu::Device) -> &wgpu::RenderPipeline {
        self.offset_pipeline
            .for_draw(&self.recorder, self.adapter_backend, || {
                self.offset_pipeline_job(device)()
            })
    }

    fn blit_pipeline_target(
        &self,
        blend_mode: BlendMode,
        variant: CompositeVariant,
    ) -> (&FixedPipeline, &'static str, wgpu::BlendState) {
        match blend_mode {
            BlendMode::Src => (
                &self.blit_pipeline_src[variant.index()],
                "Blit Pipeline Src",
                wgpu::BlendState::REPLACE,
            ),
            BlendMode::DstOut => (
                &self.blit_pipeline_dst_out[variant.index()],
                "Blit Pipeline DstOut",
                dst_out_blend_state(),
            ),
            _ => (
                &self.blit_pipeline[variant.index()],
                "Blit Pipeline",
                wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
            ),
        }
    }

    fn blit_pipeline_job(
        &self,
        device: &wgpu::Device,
        blend_mode: BlendMode,
        variant: CompositeVariant,
    ) -> FixedPipelineJob {
        let (_, label, blend) = self.blit_pipeline_target(blend_mode, variant);
        fullscreen_pipeline_job(
            device,
            self.pipeline_cache.as_ref(),
            label,
            ShaderEntry {
                shader: &self.blit_shader,
                entries: ("blit_vs", "blit_fs"),
                constants: &[(
                    "BLIT_UNMASKED_NEAREST",
                    if variant.sampling { 1.0 } else { 0.0 },
                )],
            },
            self.surface_format,
            (blend, crate::render::overlay_depth_state(variant.depth)),
        )
    }

    /// Queues on the warm-up lane the composites most screens with layers
    /// draw with: blits of either sampling and the projective blit of whole
    /// texels, without a depth buffer.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn warm_composites(&self, device: &wgpu::Device) {
        for sampling in [false, true] {
            let variant = CompositeVariant::new(sampling, false);
            self.blit_pipeline_target(BlendMode::SrcOver, variant)
                .0
                .queue(
                    &self.compiler,
                    CompileLane::WarmUp,
                    self.adapter_backend,
                    self.blit_pipeline_job(device, BlendMode::SrcOver, variant),
                );
        }
        let texels = CompositeVariant::new(true, false);
        self.projective_pipeline_target(BlendMode::SrcOver, texels)
            .0
            .queue(
                &self.compiler,
                CompileLane::WarmUp,
                self.adapter_backend,
                self.projective_pipeline_job(device, BlendMode::SrcOver, texels),
            );
    }

    /// The blit pipeline for a composite, or `None` while it compiles.
    fn blit_pipeline(
        &self,
        device: &wgpu::Device,
        blend_mode: BlendMode,
        variant: CompositeVariant,
    ) -> Option<&wgpu::RenderPipeline> {
        let (resource, _, _) = self.blit_pipeline_target(blend_mode, variant);
        let pipeline = resource
            .when_ready(&self.recorder, &self.compiler, self.adapter_backend, || {
                self.blit_pipeline_job(device, blend_mode, variant)
            })
            // A nearest-sampled blit of whole texels draws the same through
            // the general blit while its own compiles.
            .or_else(|| {
                variant.sampling.then_some(())?;
                let general = CompositeVariant::new(false, variant.depth);
                self.blit_pipeline_target(blend_mode, general).0.built()
            });
        self.note_skipped(pipeline.is_none());
        pipeline
    }

    fn note_skipped(&self, skipped: bool) {
        self.skipped_draw.set(self.skipped_draw.get() | skipped);
    }

    /// Whether a composite was left out since the last call because its
    /// pipeline compiles: the renderer then asks for the frame after the
    /// pipeline lands.
    pub(crate) fn take_skipped_draw(&self) -> bool {
        self.skipped_draw.replace(false)
    }

    fn projective_pipeline_target(
        &self,
        blend_mode: BlendMode,
        variant: CompositeVariant,
    ) -> (&FixedPipeline, &'static str, wgpu::BlendState) {
        match blend_mode {
            BlendMode::Src => (
                &self.projective_blit_pipeline_src[variant.index()],
                "Projective Blit Pipeline Src",
                wgpu::BlendState::REPLACE,
            ),
            BlendMode::DstOut => (
                &self.projective_blit_pipeline_dst_out[variant.index()],
                "Projective Blit Pipeline DstOut",
                dst_out_blend_state(),
            ),
            _ => (
                &self.projective_blit_pipeline[variant.index()],
                "Projective Blit Pipeline",
                wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
            ),
        }
    }

    fn projective_pipeline_job(
        &self,
        device: &wgpu::Device,
        blend_mode: BlendMode,
        variant: CompositeVariant,
    ) -> FixedPipelineJob {
        let (_, label, blend) = self.projective_pipeline_target(blend_mode, variant);
        projective_pipeline_job(
            device,
            self.pipeline_cache.as_ref(),
            label,
            &self.projective_blit_shader,
            self.surface_format,
            blend,
            variant,
        )
    }

    /// The projective blit pipeline for a composite, or `None` while it
    /// compiles.
    fn projective_blit_pipeline(
        &self,
        device: &wgpu::Device,
        blend_mode: BlendMode,
        variant: CompositeVariant,
    ) -> Option<&wgpu::RenderPipeline> {
        let (resource, _, _) = self.projective_pipeline_target(blend_mode, variant);
        let pipeline =
            resource.when_ready(&self.recorder, &self.compiler, self.adapter_backend, || {
                self.projective_pipeline_job(device, blend_mode, variant)
            });
        self.note_skipped(pipeline.is_none());
        pipeline
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

    pub(crate) fn end_offscreen_frame(&mut self) {
        self.offscreen_pool.end_frame();
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
        stats
            .glass_rasterized_pixels
            .set(stats.glass_rasterized_pixels.get() + self.debug_glass_rasterized_pixels.get());
        stats
            .blur_pixels
            .set(stats.blur_pixels.get() + self.debug_blur_pixels.get());
        stats.shader_pipeline_fallback_draws.set(
            stats.shader_pipeline_fallback_draws.get() + self.debug_shader_fallback_draws.get(),
        );
        stats
            .shader_specialized_draws
            .set(stats.shader_specialized_draws.get() + self.debug_shader_specialized_draws.get());
        self.debug_command_stats.set(FrameCommandStats::default());
        self.debug_blurs.set(0);
        self.debug_substrates.set(0);
        self.debug_composites.set(0);
        self.debug_effects.set(0);
        self.debug_shader_pixels.set(0);
        self.debug_glass_rasterized_pixels.set(0);
        self.debug_blur_pixels.set(0);
        self.debug_shader_fallback_draws.set(0);
        self.debug_shader_specialized_draws.set(0);
    }

    fn record_shader_pipeline_fit(&self, fit: ShaderPipelineFit) {
        let counter = match fit {
            ShaderPipelineFit::Fallback => &self.debug_shader_fallback_draws,
            ShaderPipelineFit::Specialized => &self.debug_shader_specialized_draws,
            ShaderPipelineFit::General => return,
        };
        counter.set(counter.get() + 1);
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

    fn encode_blur_pass<C: FrameCommandRecorder, I: Iterator<Item = BlurDraw>>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        target: BlurPassTarget<'_>,
        sources: BlurSourceSet<'_>,
        draws: I,
    ) {
        let (pass_id, dest_size) = (target.pass_id, target.size);
        let mut prepared = std::mem::take(&mut self.blur_prepared_draws);
        prepared.clear();
        let mut written = 0u64;
        for draw in draws {
            let (_, _, width, height) = draw.scissor.unwrap_or((0, 0, dest_size.0, dest_size.1));
            written += u64::from(width) * u64::from(height);
            let values = draw.uniforms(self);
            let uniform = recorder.upload_uniform(
                pass_id,
                blur_uniform_spec(pass_id),
                device,
                &self.blur_uniform_bind_group_layout,
                bytemuck::bytes_of(&values),
            );
            prepared.push(PreparedBlurDraw {
                source: draw.source,
                uniform,
                pipeline: draw.pipeline(),
                scissor: draw.scissor,
            });
        }
        self.debug_blur_pixels
            .set(self.debug_blur_pixels.get() + written);
        self.encode_prepared_blur_pass(recorder, device, target, &sources, prepared);
    }

    fn encode_prepared_blur_pass<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        target: BlurPassTarget<'_>,
        sources: &BlurSourceSet<'_>,
        mut prepared: Vec<PreparedBlurDraw>,
    ) {
        let mut pass = recorder.begin_color_pass(target.label, target.view, target.load_op);
        let mut bound = None;
        for draw in prepared.drain(..) {
            if bound != Some(draw.pipeline) {
                pass.set_pipeline(self.blur_draw_pipeline(device, draw.pipeline));
                bound = Some(draw.pipeline);
            }
            let source = sources.resolve(draw.source);
            let source_bind_group = source.get_or_create_bind_group(
                device,
                &self.effect_texture_bind_group_layout,
                &self.effect_linear_sampler,
            );
            pass.set_bind_group(0, source_bind_group, &[]);
            pass.set_bind_group(1, &draw.uniform.bind_group, &[draw.uniform.offset]);
            if let Some((x, y, width, height)) = draw.scissor {
                pass.set_scissor_rect(x, y, width, height);
            }
            pass.draw(0..4, 0..1);
        }
        drop(pass);
        self.blur_prepared_draws = prepared;
    }

    /// The uniforms of one blur pass sampling `sampled`, reading its
    /// `source` region and writing the `dest` region of the target; the
    /// radius counts kernel steps, the coarser of a source texel and a
    /// destination pixel.
    fn blur_uniforms(
        &self,
        horizontal: bool,
        sampled: (u32, u32),
        source: [f32; 4],
        dest: [f32; 4],
        radius: (f32, f32),
        tile_mode: TileMode,
    ) -> BlurUniforms {
        let direction = if horizontal { [1.0, 0.0] } else { [0.0, 1.0] };
        let kernel_radius = if horizontal { radius.0 } else { radius.1 };
        let key = kernel_radius.to_bits();
        let kernel = {
            let mut kernels = self.blur_kernels.borrow_mut();
            if let Some(kernel) = kernels.get(&key) {
                *kernel
            } else {
                let kernel = BlurKernel::of_radius(kernel_radius);
                kernels.put(key, kernel);
                kernel
            }
        };
        BlurUniforms {
            direction_and_radius: [direction[0], direction[1], radius.0, radius.1],
            texture_size_and_tile_mode: [
                sampled.0 as f32,
                sampled.1 as f32,
                tile_mode_uniform_value(tile_mode),
                0.0,
            ],
            source_region: source,
            dest_region: dest,
            pairs: kernel
                .pairs
                .map(|pair| [pair.inner, pair.outer, pair.offset, pair.weight]),
            kernel: [kernel.pair_count as f32, kernel.total_weight, 0.0, 0.0],
        }
    }

    /// Blurs `source` into `dest` through `scratch`: a wide blur first
    /// averages each block of source texels into a scratch-size downsample
    /// and runs its horizontal pass over that, so no source texel is skipped;
    /// the vertical pass reads the scratch and writes `dest`, at the
    /// source's size or the scratch's. Returns the passes encoded.
    pub(crate) fn encode_blur_scissored_ping_pong_passes<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        source: &OffscreenTarget,
        scratch: &OffscreenTarget,
        dest: BlurDest<'_>,
        blur: BlurSpec,
    ) -> u32 {
        let (tile_mode, scissor) = (blur.tile_mode, dest.scissor);
        debug_assert!(
            blur.radius_x > 0.0 || blur.radius_y > 0.0,
            "zero-radius blur should use the composite fast path"
        );
        let scale_x = source.width as f32 / scratch.width.max(1) as f32;
        let scale_y = source.height as f32 / scratch.height.max(1) as f32;
        let radius = (blur.radius_x / scale_x, blur.radius_y / scale_y);
        let whole_source = (0, 0, source.width, source.height);
        let whole_scratch = (0, 0, scratch.width, scratch.height);
        let scratch_scissor =
            scaled_scissor(scissor, scale_x, scale_y, scratch.width, scratch.height);
        let (dest_region, dest_scissor) = if dest.size == (scratch.width, scratch.height) {
            (whole_scratch, scratch_scissor)
        } else {
            ((0, 0, dest.size.0, dest.size.1), scissor)
        };
        let block = blur_block(whole_source, whole_scratch);
        let margin = kernel_margin(radius);
        let scratch_size = (scratch.width, scratch.height);
        let horizontal_scissor =
            widened_scissor(scratch_scissor, (0, margin.1), scratch_size, tile_mode);
        let downsample_scissor = widened_scissor(scratch_scissor, margin, scratch_size, tile_mode);
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
                BlurPassTarget::cleared(
                    ("Blur Downsample Pass", UploadAllocatorId::BlurDownsample),
                    &small.view,
                    scratch_size,
                ),
                BlurSourceSet::single(source),
                std::iter::once(BlurDraw {
                    source: BlurSource::Primary,
                    uniforms: BlurUniformSpec {
                        horizontal: true,
                        sampled: (source.width, source.height),
                        source: region_uniform(whole_source),
                        dest: region_uniform(whole_scratch),
                        radius: (0.0, 0.0),
                        tile_mode,
                    },
                    filter: BlurFilter::Downsample(block),
                    scissor: downsample_scissor,
                }),
            );
        }
        let (horizontal_source, horizontal_region) = match &small {
            Some((_, small)) => (small, whole_scratch),
            None => (source, whole_source),
        };
        self.encode_blur_pass(
            recorder,
            device,
            BlurPassTarget::cleared(
                ("Blur Horizontal Pass", UploadAllocatorId::BlurHorizontal),
                &scratch.view,
                scratch_size,
            ),
            BlurSourceSet::single(horizontal_source),
            std::iter::once(BlurDraw {
                source: BlurSource::Primary,
                uniforms: BlurUniformSpec {
                    horizontal: true,
                    sampled: (horizontal_source.width, horizontal_source.height),
                    source: region_uniform(horizontal_region),
                    dest: region_uniform(whole_scratch),
                    radius,
                    tile_mode,
                },
                filter: BlurFilter::Kernel,
                scissor: horizontal_scissor,
            }),
        );
        self.encode_blur_pass(
            recorder,
            device,
            BlurPassTarget::cleared(
                ("Blur Vertical Pass", UploadAllocatorId::BlurVertical),
                dest.view,
                dest.size,
            ),
            BlurSourceSet::single(scratch),
            std::iter::once(BlurDraw {
                source: BlurSource::Primary,
                uniforms: BlurUniformSpec {
                    horizontal: false,
                    sampled: (scratch.width, scratch.height),
                    source: region_uniform(whole_scratch),
                    dest: region_uniform(dest_region),
                    radius,
                    tile_mode,
                },
                filter: BlurFilter::Kernel,
                scissor: dest_scissor,
            }),
        );
        match small {
            Some((descriptor, small)) => {
                recorder.release_transient_offscreen(descriptor, small);
                3
            }
            None => 2,
        }
    }

    /// One draw of the mean pipeline over `source_region` of `source`: the
    /// row means into a column, or the column into one texel, at `dest`.
    fn mean_draw(
        source: BlurSource,
        sampled: (u32, u32),
        source_region: (u32, u32, u32, u32),
        dest: (u32, u32, u32, u32),
        horizontal: bool,
    ) -> BlurDraw {
        BlurDraw {
            source,
            uniforms: BlurUniformSpec {
                horizontal,
                sampled,
                source: region_uniform(source_region),
                dest: region_uniform(dest),
                radius: (0.0, 0.0),
                tile_mode: TileMode::Clamp,
            },
            filter: BlurFilter::Mean,
            scissor: Some(dest),
        }
    }

    /// The means of a side atlas with no blur passes to ride in: the row
    /// means into `scratch`, then each column into its texel at `dest` in
    /// `output`, cleared first unless the output is the atlas.
    fn encode_mean_substrates<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        atlas: &OffscreenTarget,
        scratch: &OffscreenTarget,
        output: MeanOutput<'_>,
        substrates: &[SubstrateRegion],
    ) {
        let (output, load_op) = match output {
            MeanOutput::Keep(output) => (output, wgpu::LoadOp::Load),
            MeanOutput::Clear(output) => (output, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)),
        };
        self.encode_blur_pass(
            recorder,
            device,
            BlurPassTarget::cleared(
                ("Substrate Mean Pass", UploadAllocatorId::BlurDownsample),
                &scratch.view,
                (scratch.width, scratch.height),
            ),
            BlurSourceSet::single(atlas),
            substrates
                .iter()
                .filter(|substrate| matches!(substrate.average, SubstrateAverage::Mean))
                .map(|mean| {
                    Self::mean_draw(
                        BlurSource::Primary,
                        (atlas.width, atlas.height),
                        mean.source,
                        mean.scratch,
                        true,
                    )
                }),
        );
        self.encode_blur_pass(
            recorder,
            device,
            BlurPassTarget {
                label: "Substrate Mean Pass",
                pass_id: UploadAllocatorId::BlurDownsample,
                view: &output.view,
                size: (output.width, output.height),
                load_op,
            },
            BlurSourceSet::single(scratch),
            substrates
                .iter()
                .filter(|substrate| matches!(substrate.average, SubstrateAverage::Mean))
                .map(|mean| {
                    Self::mean_draw(
                        BlurSource::Primary,
                        (scratch.width, scratch.height),
                        mean.scratch,
                        mean.dest,
                        false,
                    )
                }),
        );
    }

    /// The downsample pass's draws: every blur whose scratch is coarser
    /// than its source, the block averages, and each riding mean's row
    /// reduction into its column.
    fn downsample_draws<'a>(
        atlas: &'a OffscreenTarget,
        regions: &'a [BlurRegion],
        substrates: &'a [SubstrateRegion],
        riding_means: bool,
    ) -> impl Iterator<Item = BlurDraw> + 'a {
        let sampled = (atlas.width, atlas.height);
        let downsample_draw = move |source: (u32, u32, u32, u32),
                                    scratch: (u32, u32, u32, u32),
                                    block: u32,
                                    tile_mode: TileMode,
                                    scissor| BlurDraw {
            source: BlurSource::Primary,
            uniforms: BlurUniformSpec {
                horizontal: true,
                sampled,
                source: region_uniform(source),
                dest: region_uniform(scratch),
                radius: (0.0, 0.0),
                tile_mode,
            },
            filter: BlurFilter::Downsample(block),
            scissor,
        };
        let blur_draws = regions.iter().filter_map(move |region| {
            let block = blur_block(region.source, region.scratch);
            (block > 1).then(|| {
                downsample_draw(
                    region.source,
                    region.scratch,
                    block,
                    region.blur.tile_mode,
                    Some(region.pass_scissor((1, 1))),
                )
            })
        });
        let block_draws = substrates.iter().filter_map(move |substrate| {
            let SubstrateAverage::Block(block) = substrate.average else {
                return None;
            };
            Some(downsample_draw(
                substrate.source,
                substrate.scratch,
                block,
                TileMode::Clamp,
                Some(substrate.pass_scissor()),
            ))
        });
        let mean_draws = substrates
            .iter()
            .filter(move |substrate| {
                riding_means && matches!(substrate.average, SubstrateAverage::Mean)
            })
            .map(move |mean| {
                Self::mean_draw(
                    BlurSource::Primary,
                    sampled,
                    mean.source,
                    mean.scratch,
                    true,
                )
            });
        blur_draws.chain(block_draws).chain(mean_draws)
    }

    /// The horizontal pass's draws: each blur's kernel over its downsample
    /// in `result` or its source in the atlas, and each riding mean's
    /// column reduction into one texel of the scratch.
    fn horizontal_draws<'a>(
        atlas: &'a OffscreenTarget,
        result: &'a OffscreenTarget,
        regions: &'a [BlurRegion],
        substrates: &'a [SubstrateRegion],
        riding_means: bool,
    ) -> impl Iterator<Item = BlurDraw> + 'a {
        let atlas_size = (atlas.width, atlas.height);
        let result_size = (result.width, result.height);
        let blur_draws = regions.iter().map(move |region| {
            let block = blur_block(region.source, region.scratch);
            let (source, sampled, source_region) = if block > 1 {
                (BlurSource::Secondary, result_size, region.scratch)
            } else {
                (BlurSource::Primary, atlas_size, region.source)
            };
            BlurDraw {
                source,
                uniforms: BlurUniformSpec {
                    horizontal: true,
                    sampled,
                    source: region_uniform(source_region),
                    dest: region_uniform(region.scratch),
                    radius: region.scratch_radius(),
                    tile_mode: region.blur.tile_mode,
                },
                filter: BlurFilter::Kernel,
                scissor: Some(region.pass_scissor((0, 1))),
            }
        });
        let mean_draws = substrates
            .iter()
            .filter(move |substrate| {
                riding_means && matches!(substrate.average, SubstrateAverage::Mean)
            })
            .map(move |mean| {
                Self::mean_draw(
                    BlurSource::Secondary,
                    result_size,
                    mean.scratch,
                    (mean.scratch.0, mean.scratch.1, 1, 1),
                    false,
                )
            });
        blur_draws.chain(mean_draws)
    }

    /// The vertical pass's draws: each blur's kernel over the scratch into
    /// its destination, and each riding mean's texel carried from the
    /// scratch to its destination.
    fn vertical_draws<'a>(
        scratch: &'a OffscreenTarget,
        regions: &'a [BlurRegion],
        substrates: &'a [SubstrateRegion],
        riding_means: bool,
    ) -> impl Iterator<Item = BlurDraw> + 'a {
        let blur_draws = regions.iter().map(move |region| BlurDraw {
            source: BlurSource::Primary,
            uniforms: BlurUniformSpec {
                horizontal: false,
                sampled: (scratch.width, scratch.height),
                source: region_uniform(region.scratch),
                dest: region_uniform(region.dest),
                radius: region.scratch_radius(),
                tile_mode: region.blur.tile_mode,
            },
            filter: BlurFilter::Kernel,
            scissor: Some({
                let (x, y, width, height) = region.pass_scissor((0, 0));
                (
                    region.dest.0 + (x - region.scratch.0),
                    region.dest.1 + (y - region.scratch.1),
                    width,
                    height,
                )
            }),
        });
        let mean_draws = substrates
            .iter()
            .filter(move |substrate| {
                riding_means && matches!(substrate.average, SubstrateAverage::Mean)
            })
            .map(move |mean| {
                Self::mean_draw(
                    BlurSource::Primary,
                    (scratch.width, scratch.height),
                    (mean.scratch.0, mean.scratch.1, 1, 1),
                    mean.dest,
                    false,
                )
            });
        blur_draws.chain(mean_draws)
    }

    /// The side passes of a stage's atlas: every blur's downsample,
    /// horizontal and vertical pass with the block averages in the
    /// downsample pass, and the means. A mean's row reduction rides in the
    /// downsample pass and its column reduction in the horizontal pass when a
    /// blur runs them, and its texel is carried into the vertical pass;
    /// without a blur the means take their own two passes. Blurs and means
    /// land in `result` at their scratch slots, or in `blur_output` at each
    /// region's `dest`.
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
            blur_output,
        } = work;
        let output = blur_output.unwrap_or(result);
        let has_mean_substrates = substrates
            .iter()
            .any(|substrate| matches!(substrate.average, SubstrateAverage::Mean));
        let folded = !regions.is_empty() && has_mean_substrates;
        let has_means = !folded && has_mean_substrates;
        if has_means {
            self.encode_mean_substrates(
                recorder,
                device,
                atlas,
                scratch,
                if blur_output.is_some() {
                    MeanOutput::Keep(output)
                } else {
                    MeanOutput::Clear(output)
                },
                substrates,
            );
        }
        let has_downsample = regions
            .iter()
            .any(|region| blur_block(region.source, region.scratch) > 1)
            || substrates
                .iter()
                .any(|substrate| matches!(substrate.average, SubstrateAverage::Block(_)))
            || folded;
        if has_downsample {
            self.encode_blur_pass(
                recorder,
                device,
                BlurPassTarget {
                    label: "Blur Downsample Pass",
                    pass_id: UploadAllocatorId::BlurDownsample,
                    view: &result.view,
                    size: (result.width, result.height),
                    load_op: if has_means && blur_output.is_none() {
                        wgpu::LoadOp::Load
                    } else {
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                    },
                },
                BlurSourceSet::single(atlas),
                Self::downsample_draws(atlas, regions, substrates, folded),
            );
        }
        if regions.is_empty() {
            return;
        }
        self.encode_blur_pass(
            recorder,
            device,
            BlurPassTarget::cleared(
                ("Blur Horizontal Pass", UploadAllocatorId::BlurHorizontal),
                &scratch.view,
                (scratch.width, scratch.height),
            ),
            BlurSourceSet {
                primary: atlas,
                secondary: result,
            },
            Self::horizontal_draws(atlas, result, regions, substrates, folded),
        );
        self.encode_blur_pass(
            recorder,
            device,
            BlurPassTarget {
                label: "Blur Vertical Pass",
                pass_id: UploadAllocatorId::BlurVertical,
                view: &output.view,
                size: (output.width, output.height),
                load_op: if blur_output.is_none() && substrates.is_empty() {
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                } else {
                    wgpu::LoadOp::Load
                },
            },
            BlurSourceSet::single(scratch),
            Self::vertical_draws(scratch, regions, substrates, folded),
        );
        self.record_blur_pass();
    }

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

    pub(crate) fn encode_shader<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        target: EffectTarget<'_>,
        shader: &RuntimeShader,
        scissor: Option<(u32, u32, u32, u32)>,
    ) -> bool {
        self.encode_shader_pass(
            recorder,
            device,
            target,
            shader,
            ShaderPassOptions {
                load_op: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                scissor,
                pipeline_mode: RuntimeShaderPipelineMode::Replace,
                source_logical_size: None,
                source_region: None,
                substrate_regions: [None; MAX_SUBSTRATES],
            },
        )
    }

    pub(crate) fn prepare_shader_draw<'a, C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        item: &ShaderCompositeBatchItem<'a>,
    ) -> Option<PreparedShaderDraw<'a>> {
        let mut pipelines = SmallVec::new();
        let mut fit = ShaderPipelineFit::General;
        let mut general = None;
        let specialization = item
            .shader
            .draw_specialization(layer_pixels(item.layer_pixel_rect));
        for &variant in shader_draw_variants(specialization) {
            let (pipeline, variant_fit) = self.shader_cache.get_or_create(
                item.shader,
                specialization,
                RuntimeShaderPipelineMode::PremultipliedSrcOver,
                variant,
            )?;
            match variant_fit {
                ShaderPipelineFit::Fallback => general = Some(pipeline.clone()),
                ShaderPipelineFit::Specialized => {
                    fit = ShaderPipelineFit::Specialized;
                    pipelines.push((variant, pipeline.clone()));
                }
                ShaderPipelineFit::General => pipelines.push((variant, pipeline.clone())),
            }
        }
        if let Some(general) = general {
            pipelines.clear();
            pipelines.push((ShaderDrawVariant::Whole, general));
            fit = ShaderPipelineFit::Fallback;
        }
        self.record_shader_pipeline_fit(fit);
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
            placement: item.placement.uniform,
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
        Some(PreparedShaderDraw {
            shader: item.shader,
            specialization,
            texture_bind_group,
            uniform,
            scissor: item.scissor,
            dest_viewport: item.dest_viewport,
            viewport: item.placement.viewport,
            layer_pixel_rect: item.layer_pixel_rect,
            pipelines,
        })
    }

    pub(crate) fn draw_prepared_shader_src_over(
        &mut self,
        pass: &mut wgpu::RenderPass<'_>,
        viewport: (u32, u32),
        draw: &PreparedShaderDraw<'_>,
    ) {
        pass.set_bind_group(0, draw.texture_bind_group, &[]);
        pass.set_bind_group(1, &draw.uniform.bind_group, &[draw.uniform.offset]);
        let (x, y, width, height) = draw.dest_viewport;
        let (left, top, view_width, view_height) = draw.viewport;
        debug_assert!(
            left >= 0.0
                && top >= 0.0
                && left + view_width <= viewport.0 as f32
                && top + view_height <= viewport.1 as f32,
            "a viewport must lie inside its target: Safari's WebGPU drops the frame otherwise"
        );
        pass.set_viewport(left, top, view_width, view_height, 0.0, 1.0);
        let scissor = draw.scissor.unwrap_or((0, 0, viewport.0, viewport.1));
        self.debug_shader_pixels
            .set(self.debug_shader_pixels.get() + shaded_pixels((x, y, width, height), scissor));
        let split = (draw.pipelines.len() > 1)
            .then(|| {
                let quad = (
                    x.floor().max(0.0) as u32,
                    y.floor().max(0.0) as u32,
                    (x + width).ceil().max(0.0) as u32,
                    (y + height).ceil().max(0.0) as u32,
                );
                let x1 = quad.2.min(scissor.0 + scissor.2);
                let y1 = quad.3.min(scissor.1 + scissor.3);
                let x0 = quad.0.max(scissor.0);
                let y0 = quad.1.max(scissor.1);
                (x1 > x0 && y1 > y0).then(|| {
                    split_scissors(
                        draw.shader,
                        draw.specialization,
                        (x, y),
                        draw.layer_pixel_rect,
                        (x0, y0, x1 - x0, y1 - y0),
                    )
                })
            })
            .flatten()
            .flatten();
        for (variant, pipeline) in &draw.pipelines {
            pass.set_pipeline(pipeline);
            let regions: SmallVec<[(u32, u32, u32, u32); 4]> = match (variant, &split) {
                (ShaderDrawVariant::Interior, Some(split)) => split.interior.into_iter().collect(),
                (ShaderDrawVariant::Rim, Some(split)) => {
                    split.rim.iter().flatten().copied().collect()
                }
                _ => std::iter::once(scissor).collect(),
            };
            for region in regions {
                pass.set_scissor_rect(region.0, region.1, region.2, region.3);
                self.debug_glass_rasterized_pixels.set(
                    self.debug_glass_rasterized_pixels.get()
                        + shaded_pixels((x, y, width, height), region),
                );
                pass.draw(0..4, 0..1);
            }
        }
        pass.set_viewport(0.0, 0.0, viewport.0 as f32, viewport.1 as f32, 0.0, 1.0);
        pass.set_scissor_rect(0, 0, viewport.0, viewport.1);
    }

    fn encode_shader_pass<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        target: EffectTarget<'_>,
        shader: &RuntimeShader,
        options: ShaderPassOptions,
    ) -> bool {
        let mut padded = shader.uniforms_padded();
        ReservedShaderUniforms {
            placement: [0.0; 4],
            layer_pixel_rect: target.layer_pixel_rect,
            substrate_regions: options.substrate_regions,
            source_region: options.source_region,
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

        let specialization = shader.draw_specialization(layer_pixels(target.layer_pixel_rect));
        let Some((pipeline, fit)) = self.shader_cache.get_or_create(
            shader,
            specialization,
            options.pipeline_mode,
            ShaderDrawVariant::Whole,
        ) else {
            return false;
        };
        let pipeline = pipeline.clone();
        self.record_shader_pipeline_fit(fit);

        let texture_bind_group = target.source.get_or_create_bind_group(
            device,
            &self.effect_texture_bind_group_layout,
            &self.effect_linear_sampler,
        );

        let mut pass =
            recorder.begin_color_pass("Shader Effect Pass", target.dest_view, options.load_op);

        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, texture_bind_group, &[]);
        pass.set_bind_group(1, &uniform.bind_group, &[uniform.offset]);
        if let Some((x, y, width, height)) = options.scissor {
            pass.set_scissor_rect(x, y, width, height);
        }
        pass.draw(0..4, 0..1);
        true
    }

    fn encode_chained_substrates<'scratch, C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        target: EffectTarget<'_>,
        shader: &RuntimeShader,
        reads: EffectReads,
        scratch_targets: &mut impl EffectScratchTargetProvider<'scratch>,
    ) -> Result<u32, String> {
        let source = target.source;
        let layout = ChainedSubstrates::new(shader, source.width, source.height);
        let scratch = scratch_targets.next()?;
        let packed = scratch_targets.next()?;
        self.encode_blur_atlas_passes(
            recorder,
            device,
            source,
            scratch,
            packed,
            AtlasSideWork {
                blurs: &layout.blurs,
                averages: &layout.averages,
                blur_output: None,
            },
        );
        let copied = copy_compatible(source, packed);
        if copied {
            recorder.copy_texture_region(TextureRegionCopy {
                source,
                source_origin: [0, 0],
                dest: packed,
                dest_origin: [0, 0],
                size: [source.width, source.height],
            });
        } else {
            self.encode_composite_to_view_pass(
                recorder,
                device,
                source,
                &packed.view,
                CompositePassOptions {
                    alpha: 1.0,
                    load_op: wgpu::LoadOp::Load,
                    scissor: None,
                    rounded_mask: None,
                    blend_mode: BlendMode::Src,
                    dest_viewport: Some(BlitDest {
                        rect: (0.0, 0.0, source.width as f32, source.height as f32),
                        target: (packed.width, packed.height),
                    }),
                    source_viewport: None,
                    sample_mode: CompositeSampleMode::Linear,
                },
            );
            self.record_composite_pass();
        }
        if !self.encode_shader_pass(
            recorder,
            device,
            EffectTarget {
                source: packed,
                ..target
            },
            shader,
            ShaderPassOptions {
                load_op: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                scissor: reads.output,
                pipeline_mode: RuntimeShaderPipelineMode::Replace,
                source_logical_size: Some((source.width as f32, source.height as f32)),
                source_region: Some((0.0, 0.0, source.width as f32, source.height as f32)),
                substrate_regions: layout.regions,
            },
        ) {
            return Err("chained substrate shader failed to compile".into());
        }
        self.record_substrates(shader.substrates().len() as u32);
        self.debug_effects.set(self.debug_effects.get() + 1);
        Ok(layout.passes() + 1 + u32::from(!copied))
    }

    pub(crate) fn encode_effect<'scratch, C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        target: EffectTarget<'_>,
        effect: &RenderEffect,
        reads: EffectReads,
        scratch_targets: &mut impl EffectScratchTargetProvider<'scratch>,
    ) -> Result<u32, String> {
        let (source, dest_view) = (target.source, target.dest_view);
        match effect {
            RenderEffect::Blur {
                radius_x,
                radius_y,
                edge_treatment,
            } => {
                if *radius_x <= 0.0 && *radius_y <= 0.0 {
                    return Ok(self.encode_plain_copy(recorder, device, source, dest_view));
                }

                let intermediate = scratch_targets.next()?;
                let passes = self.encode_blur_scissored_ping_pong_passes(
                    recorder,
                    device,
                    source,
                    intermediate,
                    BlurDest {
                        view: dest_view,
                        size: (source.width, source.height),
                        scissor: reads.output,
                    },
                    BlurSpec {
                        radius_x: *radius_x,
                        radius_y: *radius_y,
                        tile_mode: *edge_treatment,
                    },
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
                if !shader.substrates().is_empty() {
                    return self.encode_chained_substrates(
                        recorder,
                        device,
                        target,
                        shader,
                        reads,
                        scratch_targets,
                    );
                }
                if self.encode_shader(recorder, device, target, shader, reads.output) {
                    self.debug_effects.set(self.debug_effects.get() + 1);
                    Ok(1)
                } else {
                    Ok(self.encode_plain_copy(recorder, device, source, dest_view))
                }
            }
            RenderEffect::Chain { first, second } => {
                let intermediate = scratch_targets.next()?;
                let first_reads = EffectReads {
                    output: match **second {
                        RenderEffect::Shader { .. } => reads.shader_input,
                        _ => None,
                    },
                    shader_input: None,
                };
                let first_passes = self.encode_effect(
                    recorder,
                    device,
                    EffectTarget {
                        dest_view: &intermediate.view,
                        ..target
                    },
                    first,
                    first_reads,
                    scratch_targets,
                )?;
                let second_passes = self.encode_effect(
                    recorder,
                    device,
                    EffectTarget {
                        source: intermediate,
                        ..target
                    },
                    second,
                    reads,
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
            dest_clip: options
                .dest_viewport
                .map_or(WHOLE_TARGET_CLIP, BlitDest::clip),
            source_viewport: [
                source_viewport_uniform.0,
                source_viewport_uniform.1,
                source_viewport_uniform.2,
                source_viewport_uniform.3,
            ],
            depth: [0.0; 4],
        }
    }

    // Draws `source` into a cleared `dest_view` as it is, for an effect that
    // changes nothing. Returns the passes encoded.
    fn encode_plain_copy<C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        source: &OffscreenTarget,
        dest_view: &wgpu::TextureView,
    ) -> u32 {
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
        1
    }

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

        let Some(pipeline) = self.blit_pipeline(
            device,
            options.blend_mode,
            CompositeVariant::new(options.unmasked_nearest(), false),
        ) else {
            return;
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, texture_bind_group, &[]);
        pass.set_bind_group(1, &uniform.bind_group, &[uniform.offset]);
        if let Some((x, y, w, h)) = options.scissor {
            pass.set_scissor_rect(x, y, w, h);
        }
        pass.draw(0..4, 0..1);
    }

    pub(crate) fn prepare_composite_draw<'a, C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        device: &wgpu::Device,
        load_op: wgpu::LoadOp<wgpu::Color>,
        item: &CompositeBatchItem<'a>,
    ) -> PreparedCompositeDraw<'a> {
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
        let variant = CompositeVariant::new(options.unmasked_nearest(), item.depth.is_some());
        self.prefetch_blit(CompileLane::WarmUp, device, item.blend_mode, variant);
        let mut uniforms = Self::composite_pass_uniforms(options);
        uniforms.depth[0] = item.depth.unwrap_or(0.0);
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
        PreparedCompositeDraw {
            texture_bind_group,
            uniform,
            scissor: item.scissor,
            blend_mode: item.blend_mode,
            variant,
        }
    }

    /// Builds, once the frame ends, the blit a later frame's composite of
    /// `blend_mode`, sampled as `sample_mode` through a mask or not, draws
    /// with.
    pub(crate) fn prefetch_later_composite(
        &self,
        device: &wgpu::Device,
        blend_mode: BlendMode,
        sample_mode: CompositeSampleMode,
        masked: bool,
    ) {
        self.prefetch_blit(
            CompileLane::Later,
            device,
            blend_mode,
            CompositeVariant::new(unmasked_nearest(sample_mode, masked), false),
        );
    }

    /// Queues the build of the blit `draw` would take in a pass with a depth
    /// buffer or without one on `lane` when it is missing, handing `awaited`
    /// the pipeline while it is still to land.
    pub(crate) fn prefetch_composite_pipeline(
        &self,
        device: &wgpu::Device,
        draw: &PreparedCompositeDraw<'_>,
        (depth, lane): (bool, CompileLane),
        awaited: &mut impl FnMut(Pending<wgpu::RenderPipeline>),
    ) {
        let variant = CompositeVariant {
            depth,
            ..draw.variant
        };
        if let Some(pending) = self.prefetch_blit(lane, device, draw.blend_mode, variant) {
            awaited(pending);
        }
    }

    fn prefetch_blit(
        &self,
        lane: CompileLane,
        device: &wgpu::Device,
        blend_mode: BlendMode,
        variant: CompositeVariant,
    ) -> Option<Pending<wgpu::RenderPipeline>> {
        let (blit, _, _) = self.blit_pipeline_target(blend_mode, variant);
        blit.prefetch(lane, &self.compiler, self.adapter_backend, || {
            self.blit_pipeline_job(device, blend_mode, variant)
        })
    }

    pub(crate) fn draw_prepared_composite(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        (device, viewport): (&wgpu::Device, (u32, u32)),
        draw: &PreparedCompositeDraw<'_>,
    ) {
        let Some(pipeline) = self.blit_pipeline(device, draw.blend_mode, draw.variant) else {
            return;
        };
        pass.set_pipeline(pipeline);
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
        let variant = CompositeVariant::new(
            item.sample_mode == CompositeSampleMode::Texels,
            item.depth.is_some(),
        );
        self.prefetch_projective(CompileLane::WarmUp, device, item.blend_mode, variant);
        let vertices = item
            .dest_strip
            .map(|position| ProjectiveBlitVertex { position });
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
            source_region: item
                .source_region
                .map_or([0.0; 4], |(x, y, width, height)| [x, y, width, height]),
            depth: [item.depth.unwrap_or(0.0), 0.0, 0.0, 0.0],
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
            vertex_count: item.dest_corners,
            blend_mode: item.blend_mode,
            variant,
            scissor: item.scissor,
        }
    }

    /// Queues the build of the projective blit `draw` would take in a pass
    /// with a depth buffer or without one on `lane` when it is missing,
    /// handing `awaited` the pipeline while it is still to land.
    pub(crate) fn prefetch_projective_pipeline(
        &self,
        device: &wgpu::Device,
        draw: &PreparedProjectiveComposite<'_>,
        (depth, lane): (bool, CompileLane),
        awaited: &mut impl FnMut(Pending<wgpu::RenderPipeline>),
    ) {
        let variant = CompositeVariant {
            depth,
            ..draw.variant
        };
        if let Some(pending) = self.prefetch_projective(lane, device, draw.blend_mode, variant) {
            awaited(pending);
        }
    }

    fn prefetch_projective(
        &self,
        lane: CompileLane,
        device: &wgpu::Device,
        blend_mode: BlendMode,
        variant: CompositeVariant,
    ) -> Option<Pending<wgpu::RenderPipeline>> {
        let (projective, _, _) = self.projective_pipeline_target(blend_mode, variant);
        projective.prefetch(lane, &self.compiler, self.adapter_backend, || {
            self.projective_pipeline_job(device, blend_mode, variant)
        })
    }

    pub(crate) fn draw_prepared_projective_composite(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        (device, viewport): (&wgpu::Device, (u32, u32)),
        draw: &PreparedProjectiveComposite<'_>,
    ) {
        let Some(pipeline) = self.projective_blit_pipeline(device, draw.blend_mode, draw.variant)
        else {
            return;
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, draw.texture_bind_group, &[]);
        pass.set_bind_group(1, &draw.uniform.bind_group, &[draw.uniform.offset]);
        pass.set_vertex_buffer(0, draw.vertices.slice());
        if let Some((x, y, w, h)) = draw.scissor {
            pass.set_scissor_rect(x, y, w, h);
        } else {
            pass.set_scissor_rect(0, 0, viewport.0, viewport.1);
        }
        pass.draw(0..draw.vertex_count, 0..1);
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
        CompositeSampleMode::Texels => 2.0,
    }
}

#[cfg(test)]
#[path = "effect_renderer_tests.rs"]
mod shader_tests;

#[cfg(test)]
#[path = "tests/effect_renderer_tests.rs"]
mod tests;
