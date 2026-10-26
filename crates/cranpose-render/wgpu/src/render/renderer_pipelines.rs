//! The renderer's pipelines: the shape, image, glyph, shadow and mesh
//! pipelines with their shaders, layouts and samplers, the pipeline cache,
//! and the compiler and recorder that build and remember them.

use std::{cell::Cell, sync::Arc};

use cranpose_ui_graphics::{BlendMode, ImagePixelFormat};

use super::{
    CompileLane, CompilerSend, FixedPipeline, GlyphDrawCmd, GlyphQuads, ImageBindGroups,
    ImageDrawCmd, ImageVariant, PassDepth, Pending, ShapePipelines, SharedShader,
    create_glyph_atlas_pipeline, create_image_pipeline,
};
use crate::{
    gpu_stats::FrameStats, pipeline_compiler::PipelineCompiler,
    rrect_shadow::create_rrect_shadow_pipeline, run_store::RunDrawCall,
    shape_pipelines::PipelineDemand,
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
    /// The device, target format and backend every pipeline here builds
    /// for; a job that builds one off the frame holds the device.
    pub(super) device: Arc<wgpu::Device>,
    pub(crate) composition_format: wgpu::TextureFormat,
    pub(crate) adapter_backend: wgpu::Backend,
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
    /// Indexed by depth, then turned: see [`Self::glyph_atlas_pipeline`].
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
        &pipelines[variant.depth.index()
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

    /// See [`crate::WgpuRenderer::first_screen_pipelines`].
    #[cold]
    #[inline(never)]
    pub(crate) fn first_screen_pipelines(&self) -> Option<Vec<u8>> {
        self.recorder.first_screen_records(self.adapter_backend)
    }

    /// One of an app shader's pipelines once built; `None` while `job`
    /// builds it, which the frame notes, or when the shader does not build.
    pub(super) fn app_shader_pipeline<'p, J>(
        &'p self,
        stats: &FrameStats,
        pipeline: &'p crate::scene_pass::ShaderPipeline,
        job: impl FnOnce() -> J,
    ) -> Option<&'p wgpu::RenderPipeline>
    where
        J: FnOnce() -> Option<wgpu::RenderPipeline> + CompilerSend + 'static,
    {
        let ready =
            pipeline.when_ready((&self.recorder, &self.compiler, self.adapter_backend), job);
        match ready {
            Some(built) => built.as_ref(),
            None => {
                self.note_placeholder(stats);
                None
            }
        }
    }

    pub(super) fn image_pipeline_job(
        &self,
        blend_mode: BlendMode,
        variant: ImageVariant,
    ) -> impl FnOnce() -> wgpu::RenderPipeline + CompilerSend + 'static {
        let device = Arc::clone(&self.device);
        let cache = self.pipeline_cache.clone();
        let format = self.composition_format;
        let shader = self.image_shader.clone();
        move || {
            create_image_pipeline(
                &device,
                cache.as_ref(),
                format,
                &shader,
                blend_mode,
                variant,
            )
        }
    }

    /// Queues the builds of the image pipelines `cmds` draw with that are
    /// missing on `lane` (see [`FixedPipeline::prefetch`]), handing
    /// `awaited` each still to land.
    pub(crate) fn prefetch_image_pipelines(
        &self,
        cmds: &[ImageDrawCmd],
        blend_mode: BlendMode,
        (depth, lane): (PassDepth, CompileLane),
        awaited: &mut impl FnMut(Pending<wgpu::RenderPipeline>),
    ) {
        let mut last = None;
        for cmd in cmds {
            let variant = ImageVariant {
                depth,
                alpha_mask: cmd.image.alpha_mask,
                rounded: cmd.clip.is_some(),
            };
            if last.replace(variant) == Some(variant) {
                continue;
            }
            if let Some(pending) = self.image_pipeline_resource(blend_mode, variant).prefetch(
                lane,
                &self.compiler,
                self.adapter_backend,
                || self.image_pipeline_job(blend_mode, variant),
            ) {
                awaited(pending);
            }
        }
    }

    /// Queues the builds of the glyph pipelines `cmds` draw with that are
    /// missing on `lane` (see [`FixedPipeline::prefetch`]), handing
    /// `awaited` each still to land.
    pub(crate) fn prefetch_glyph_pipelines(
        &self,
        cmds: &[GlyphDrawCmd],
        (depth, lane): (PassDepth, CompileLane),
        awaited: &mut impl FnMut(Pending<wgpu::RenderPipeline>),
    ) {
        let mut last = None;
        for cmd in cmds {
            let quads = cmd.quads();
            if last.replace(quads) == Some(quads) {
                continue;
            }
            if let Some(pending) = self.glyph_atlas_pipeline_resource(depth, quads).prefetch(
                lane,
                &self.compiler,
                self.adapter_backend,
                || self.glyph_atlas_pipeline_job(depth, quads),
            ) {
                awaited(pending);
            }
        }
    }

    /// Queues the build of the shadow pipeline on `lane` when it is missing
    /// (see [`FixedPipeline::prefetch`]), handing `awaited` the pipeline
    /// while it is still to land.
    pub(crate) fn prefetch_shadow_pipeline(
        &self,
        (depth, lane): (PassDepth, CompileLane),
        awaited: &mut impl FnMut(Pending<wgpu::RenderPipeline>),
    ) {
        if let Some(pending) = self.rrect_shadow_pipeline[depth.index()].prefetch(
            lane,
            &self.compiler,
            self.adapter_backend,
            || self.rrect_shadow_pipeline_job(depth),
        ) {
            awaited(pending);
        }
    }

    /// The image pipeline for a draw, or `None` while it compiles: the draw
    /// is left out, and the frame after the pipeline lands draws it.
    pub(crate) fn image_pipeline(
        &self,
        stats: &FrameStats,
        blend_mode: BlendMode,
        variant: ImageVariant,
    ) -> Option<&wgpu::RenderPipeline> {
        self.ready_or_left_out(
            stats,
            self.image_pipeline_resource(blend_mode, variant),
            || self.image_pipeline_job(blend_mode, variant),
        )
    }

    /// `pipeline` for a draw, or `None` while it compiles, noting the draw
    /// left out (see [`FixedPipeline::when_ready`]).
    pub(super) fn ready_or_left_out<'a, J>(
        &self,
        stats: &FrameStats,
        pipeline: &'a FixedPipeline,
        job: impl FnOnce() -> J,
    ) -> Option<&'a wgpu::RenderPipeline>
    where
        J: FnOnce() -> wgpu::RenderPipeline + CompilerSend + 'static,
    {
        let ready = pipeline.when_ready(&self.recorder, &self.compiler, self.adapter_backend, job);
        if ready.is_none() {
            self.note_placeholder(stats);
        }
        ready
    }

    pub(super) fn glyph_atlas_pipeline_job(
        &self,
        depth: PassDepth,
        quads: GlyphQuads,
    ) -> impl FnOnce() -> wgpu::RenderPipeline + CompilerSend + 'static {
        let device = Arc::clone(&self.device);
        let cache = self.pipeline_cache.clone();
        let format = self.composition_format;
        let shader = self.glyph_atlas_shader.clone();
        move || {
            create_glyph_atlas_pipeline(&device, cache.as_ref(), format, &shader, (depth, quads))
        }
    }

    /// The glyph pipeline for a pass with a depth buffer or not, drawing
    /// `quads`, or `None` while it compiles.
    pub(super) fn glyph_atlas_pipeline(
        &self,
        stats: &FrameStats,
        depth: PassDepth,
        quads: GlyphQuads,
    ) -> Option<&wgpu::RenderPipeline> {
        let resource = self.glyph_atlas_pipeline_resource(depth, quads);
        let ready =
            resource.when_ready(&self.recorder, &self.compiler, self.adapter_backend, || {
                self.glyph_atlas_pipeline_job(depth, quads)
            });
        // Quads on whole pixels draw the same through the plain pipeline
        // while their own compiles.
        let ready = ready.or_else(|| {
            (quads == GlyphQuads::Aligned).then_some(())?;
            self.glyph_atlas_pipeline_resource(depth, GlyphQuads::Plain)
                .built()
        });
        if ready.is_none() {
            self.note_placeholder(stats);
        }
        ready
    }

    /// Queues, ahead of every other pipeline, the fallback pipelines shape
    /// draws take while their own compile.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn warm_mesh_fallback(&self) {
        for (depth, pipeline) in PassDepth::BOTH.into_iter().zip(&self.mesh_pipelines) {
            pipeline.queue(
                &self.compiler,
                CompileLane::Demanded,
                self.adapter_backend,
                crate::mesh_fallback::mesh_pipeline_job(
                    &self.device,
                    self.pipeline_cache.clone(),
                    self.composition_format,
                    depth,
                ),
            );
        }
    }

    pub(super) fn rrect_shadow_pipeline_job(
        &self,
        depth: PassDepth,
    ) -> impl FnOnce() -> wgpu::RenderPipeline + CompilerSend + 'static {
        let device = Arc::clone(&self.device);
        let cache = self.pipeline_cache.clone();
        let format = self.composition_format;
        let shader = self.rrect_shadow_shader.clone();
        move || create_rrect_shadow_pipeline(&device, cache.as_ref(), format, &shader, depth)
    }

    pub(super) fn glyph_atlas_pipeline_resource(
        &self,
        depth: PassDepth,
        quads: GlyphQuads,
    ) -> &FixedPipeline {
        let kind = match quads {
            GlyphQuads::Plain => 0,
            GlyphQuads::Aligned => 1,
            GlyphQuads::Turned => 2,
        };
        &self.glyph_atlas_pipeline[depth.index() + 2 * kind]
    }

    /// The fallback pipeline for a pass with a depth buffer or not, once it
    /// is built; none on the web, which leaves every fallback out of its
    /// build.
    #[cfg_attr(target_arch = "wasm32", expect(unused_variables, reason = "no meshes"))]
    pub(super) fn mesh_pipeline(&self, depth: PassDepth) -> Option<&wgpu::RenderPipeline> {
        #[cfg(not(target_arch = "wasm32"))]
        return self.mesh_pipelines[depth.index()].built();
        #[cfg(target_arch = "wasm32")]
        None
    }

    /// Lays out as fallback triangles the run draws of `draws` in `range`
    /// whose shape pipelines still compile, each record of `tables` placed
    /// by `placement`.
    pub(super) fn mesh_unready(
        &self,
        draws: &[RunDrawCall],
        range: std::ops::Range<usize>,
        tables: crate::mesh_fallback::MeshRecords<'_>,
        placement: &dyn Fn(
            &cranpose_ui_graphics::ShapeRecordBody,
        ) -> Option<crate::mesh_fallback::MeshPlacement>,
        target: crate::mesh_fallback::MeshTarget,
        meshes: &mut crate::mesh_fallback::PassMeshes,
    ) {
        for index in range {
            let Some(draw) = draws.get(index) else {
                continue;
            };
            if self.shape_pipelines.get(draw.key).is_some() {
                continue;
            }
            let indices =
                meshes
                    .builder
                    .push_records(tables, draw.records.clone(), placement, target);
            meshes.set(index, indices);
        }
    }

    /// Notes an effect drawn as its placeholder, or a draw left out, while
    /// its pipelines compile, and waits for the next one to land.
    pub(crate) fn note_placeholder(&self, stats: &FrameStats) {
        stats
            .placeholder_draws
            .set(stats.placeholder_draws.get() + 1);
        self.await_landing();
    }

    /// Notes a shape drawn as fallback triangles while its pipeline
    /// compiles, and waits for the next one to land.
    pub(super) fn note_mesh_fallback(&self, stats: &FrameStats) {
        stats
            .shape_pipeline_fallback_draws
            .set(stats.shape_pipeline_fallback_draws.get() + 1);
        self.await_landing();
    }

    /// Asks for the frame after the next pipeline lands: this one drew
    /// something in place of what that pipeline draws.
    fn await_landing(&self) {
        self.drew_placeholder.set(true);
        self.recorder.hold_first_screen();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(landing) = &self.landing {
            landing.await_since(self.built_at_frame_start);
        }
    }

    /// Whether a frame must follow to replace a placeholder: a pipeline it
    /// waited for landed, or, with nothing to wake the app when one does,
    /// this frame drew one.
    pub(super) fn placeholder_awaits_frame(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(landing) = &self.landing {
            return landing.landed() || (self.drew_placeholder.get() && !landing.has_wake());
        }
        self.drew_placeholder.get()
    }

    /// Whether the last frame drew a placeholder and pipelines are still
    /// compiling, so its picture is not final yet.
    pub(crate) fn awaits_pipelines(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(landing) = &self.landing {
            return self.drew_placeholder.get() && landing.in_flight();
        }
        self.drew_placeholder.get()
    }

    // The bindings of a texture of `format` pixels, nearest and linear.
    pub(super) fn image_bind_groups(
        &self,
        view: &wgpu::TextureView,
        format: ImagePixelFormat,
    ) -> ImageBindGroups {
        ImageBindGroups {
            alpha_mask: matches!(format, ImagePixelFormat::Alpha8 { .. }),
            nearest: self.image_bind_group(view, &self.image_nearest_sampler),
            linear: self.image_bind_group(view, &self.image_linear_sampler),
        }
    }

    fn image_bind_group(
        &self,
        view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Image Texture Bind Group"),
            layout: &self.image_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }
}
