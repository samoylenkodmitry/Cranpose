//! The renderer's pipelines: the shape, image, glyph, shadow and mesh
//! pipelines with their shaders, layouts and samplers, the pipeline cache,
//! and the compiler and recorder that build and remember them.

use std::cell::Cell;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

use cranpose_ui_graphics::BlendMode;

use super::{CompileLane, FixedPipeline, ImageVariant, Pending, ShapePipelines, SharedShader};
use crate::{
    pipeline_compiler::PipelineCompiler, run_store::RunDrawCall, shape_pipelines::PipelineDemand,
};

pub(super) const IMAGE_SRC_OVER_PIPELINES: [&str; 8] = [
    "image/src-over",
    "image/src-over/depth",
    "image/src-over/alpha-mask",
    "image/src-over/alpha-mask/depth",
    "image/src-over/rounded",
    "image/src-over/rounded/depth",
    "image/src-over/alpha-mask/rounded",
    "image/src-over/alpha-mask/rounded/depth",
];

pub(super) const IMAGE_DST_OUT_PIPELINES: [&str; 8] = [
    "image/dst-out",
    "image/dst-out/depth",
    "image/dst-out/alpha-mask",
    "image/dst-out/alpha-mask/depth",
    "image/dst-out/rounded",
    "image/dst-out/rounded/depth",
    "image/dst-out/alpha-mask/rounded",
    "image/dst-out/alpha-mask/rounded/depth",
];

pub(super) const GLYPH_ATLAS_PIPELINES: [&str; 6] = [
    "glyph/atlas",
    "glyph/atlas/depth",
    "glyph/atlas/aligned",
    "glyph/atlas/aligned/depth",
    "glyph/atlas/turned",
    "glyph/atlas/turned/depth",
];

pub(super) const RRECT_SHADOW_PIPELINES: [&str; 2] = ["shadow/rrect", "shadow/rrect/depth"];

#[cfg(not(target_arch = "wasm32"))]
pub(super) const MESH_FALLBACK_PIPELINES: [&str; 2] = ["mesh/fallback", "mesh/fallback/depth"];

pub(crate) struct RendererPipelines {
    pub(crate) pipeline_cache: Option<wgpu::PipelineCache>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) _pipeline_cache_watcher: Option<crate::pipeline_disk_cache::PersistWatcher>,
    /// The pipelines recent launches of another build drew with, built once
    /// this renderer's first frame is drawn.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) recent_after_update: Option<Arc<crate::pipeline_records::PipelineRecords>>,
    /// What this renderer draws with, for the next launches.
    pub(crate) recorder: crate::pipeline_recorder::PipelineRecorder,
    /// The background compiler, which builds a pass's missing pipelines side
    /// by side.
    pub(crate) compiler: PipelineCompiler,
    /// Where the background compiler says a pipeline a placeholder waited
    /// for landed.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) landing: Option<Arc<crate::pipeline_compiler::Landing>>,
    /// The pipelines the background threads had built when this frame
    /// started.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) built_at_frame_start: u64,
    /// Whether this frame drew a placeholder in place of an effect whose
    /// pipelines were compiling, or left out a draw whose pipeline was.
    pub(crate) drew_placeholder: Cell<bool>,
    pub(crate) shape_pipelines: ShapePipelines,
    /// Image and glyph pipelines for passes without and with a depth buffer.
    pub(crate) image_pipeline: [FixedPipeline; 8],
    pub(crate) image_pipeline_dst_out: [FixedPipeline; 8],
    /// Indexed by depth, then turned: see [`GpuRenderer::glyph_atlas_pipeline`].
    pub(crate) glyph_atlas_pipeline: [FixedPipeline; 6],
    /// Round rect shadow pipelines without and with a depth buffer.
    pub(crate) rrect_shadow_pipeline: [FixedPipeline; 2],
    /// The pipelines shape draws take while their own compile, without and
    /// with a depth buffer (see [`crate::mesh_fallback`]). The web's frames
    /// build their shape pipelines, or stand one in.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) mesh_pipelines: [FixedPipeline; 2],
    pub(crate) image_shader: SharedShader,
    pub(crate) glyph_atlas_shader: SharedShader,
    pub(crate) rrect_shadow_shader: SharedShader,
    pub(crate) uniform_bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) image_bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) image_nearest_sampler: wgpu::Sampler,
    pub(crate) image_linear_sampler: wgpu::Sampler,
}

impl RendererPipelines {
    /// Whether a draw in `range` of `draws` still waits for its shape
    /// pipeline, so it takes fallback triangles.
    pub(crate) fn awaits_shape_pipeline(
        &self,
        draws: &[RunDrawCall],
        range: std::ops::Range<usize>,
    ) -> bool {
        draws.get(range).is_some_and(|draws| {
            draws
                .iter()
                .any(|draw| self.shape_pipelines.get(draw.key).is_none())
        })
    }

    /// See [`crate::WgpuRenderer::compile_pipelines_in_background`].
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn compile_pipelines_in_background(&mut self) {
        self.shape_pipelines.build_in_background();
    }

    /// Whether the background compiler has a queued pipeline left to build.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn compiling(&self) -> bool {
        self.landing
            .as_ref()
            .is_some_and(|landing| landing.in_flight())
    }

    /// Readies the pipeline of each key `pipelines` holds and, in a pass
    /// with a depth buffer, the one laying down its opaque interiors (see
    /// [`ShapePipelines::ensure`]), leaving `pipelines` empty.
    pub(crate) fn ensure_run_pipelines(&mut self, pipelines: &mut PipelineDemand) {
        for key in pipelines.drain() {
            self.shape_pipelines.ensure(key);
            if let Some(interior) = key.interior() {
                self.shape_pipelines.ensure(interior);
            }
        }
    }

    /// The lane a pass queues its image, glyph, shadow and composite
    /// pipelines on. Where the compiler's threads run, those draws are left
    /// out until their pipelines land, so they go on the demand lane, ahead
    /// of the shape pipelines, whose draws take fallback triangles.
    pub(crate) fn frame_lane(&self) -> CompileLane {
        if self.compiler.is_active() {
            CompileLane::Demanded
        } else {
            CompileLane::WarmUp
        }
    }

    pub(crate) fn image_pipeline_resource(
        &self,
        blend_mode: BlendMode,
        variant: ImageVariant,
    ) -> &FixedPipeline {
        let pipelines = match blend_mode {
            BlendMode::DstOut => &self.image_pipeline_dst_out,
            _ => &self.image_pipeline,
        };
        &pipelines[usize::from(variant.depth)
            + 2 * usize::from(variant.alpha_mask)
            + 4 * usize::from(variant.rounded)]
    }

    /// The compiler's account of the pipelines it builds, for a runtime that
    /// reads it off the present thread.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn landing(&self) -> Option<Arc<crate::pipeline_compiler::Landing>> {
        self.landing.clone()
    }

    /// Calls `wake` from the compiling thread when a pipeline a placeholder
    /// waited for lands, so an app with nothing else to draw draws again.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn wake_on_landing(&self, wake: Box<dyn Fn() + Send + Sync>) {
        if let Some(landing) = &self.landing {
            landing.wake_with(wake);
        }
    }

    /// Queues for a later frame the shape pipelines `keys` name and the
    /// interiors they lay down, handing `awaited` each still to land.
    pub(crate) fn warm_shapes_later(
        &mut self,
        keys: &PipelineDemand,
        awaited: &mut Vec<Pending<wgpu::RenderPipeline>>,
    ) {
        for key in keys.keys() {
            self.shape_pipelines
                .warm_later(key, &mut |pending| awaited.push(pending));
        }
    }
}
