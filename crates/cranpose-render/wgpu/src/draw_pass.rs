use std::{iter::Peekable, rc::Rc, sync::Arc};

use cranpose_ui_graphics::{BlendMode, FrontPolygon, Rect, RuntimeShader};

use crate::{
    effect_renderer::{
        BlitDest, CompositeBatchItem, CompositeSampleMode, PreparedCompositeDraw,
        PreparedProjectiveComposite, PreparedShaderDraw, ProjectiveCompositeItem, QuadPlacement,
        RoundedCompositeMask, ShaderCompositeBatchItem, SubstrateRegions,
    },
    frame_graph::FrameCommandRecorder,
    geometry::SegmentTransform,
    instant_triangles::{InstantDraw, InstantStandIn, LayerTexels},
    lazy_resource::Pending,
    offscreen::OffscreenTarget,
    pipeline_compiler::CompileLane,
    render::{
        ArenaPlacement, DrawSpace, GpuRenderer, ImageSlot, PassDepth, PassFrame, RunStage,
        StoreRunBatch, TargetRect, TurnedGlyphs, ViewportUniformParams, scissor_rect_for_rect,
        supported_blend_mode,
    },
    rrect_shadow::{ShadowInstance, append_shadow_instances},
    run_store::RunDrawCall,
    scene::{CompositorScene, DrawOp, RRectShadowDraw, RunDraw, TextDraw},
    scissor::{intersect_scissors, scissor_in_target},
    shape_pipelines::PipelineDemand,
};

mod held_draws;
use held_draws::{HeldCmds, HeldDraws};
mod tracked_pass;
pub(crate) use tracked_pass::TrackedPass;
mod op_items;
pub(crate) use op_items::segment_draws_anything;
use op_items::{OpItems, Texts, merge_items, segment_viewport_rect};
mod op_bounds;
pub(crate) use op_bounds::{op_draw_bounds, op_is_visible_in_rect};
mod interiors_first;
mod pass_scratch;
mod target_space;
pub(crate) use interiors_first::lays_interiors_first;
use interiors_first::takes_depth;
pub(crate) use pass_scratch::PassScratch;
use target_space::{dest_in_target, mask_in_target, translate_inverse};

/// A render target and its size in pixels.
#[derive(Clone, Copy)]
pub(crate) struct PassTarget<'a> {
    pub(crate) view: &'a wgpu::TextureView,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// What a composite's texture holds beyond this frame: a retained texture
/// keeps the pixels its cache key names for as long as it lives, so the key's
/// hash identifies them; a transient one is drawn anew every frame and
/// identifies nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceContent {
    Retained(u64),
    Transient,
}

impl SourceContent {
    pub(crate) fn retained(key: &impl std::hash::Hash) -> Self {
        let mut hasher = cranpose_ui_graphics::FxHasher::default();
        key.hash(&mut hasher);
        Self::Retained(std::hash::Hasher::finish(&hasher))
    }

    /// The hash naming a retained texture's pixels; none for a transient.
    pub(crate) fn retained_hash(self) -> Option<u64> {
        match self {
            Self::Retained(hash) => Some(hash),
            Self::Transient => None,
        }
    }

    /// The content of a texture derived from this one by `step`, retained
    /// exactly when this one is.
    pub(crate) fn derived(self, step: &impl std::hash::Hash) -> Self {
        match self {
            Self::Retained(hash) => Self::retained(&(hash, step)),
            Self::Transient => Self::Transient,
        }
    }
}

/// A resolved texture drawn into the pass at its z, described in the scene's
/// device space so one description serves every target the scene is drawn
/// into.
#[derive(Clone)]
pub(crate) struct ResolvedComposite {
    pub(crate) z_index: usize,
    pub(crate) source: Rc<OffscreenTarget>,
    pub(crate) content: SourceContent,
    pub(crate) dest: (f32, f32, f32, f32),
    pub(crate) scissor: Option<(f32, f32, f32, f32)>,
    pub(crate) kind: ResolvedCompositeKind,
}

#[derive(Clone)]
pub(crate) enum ResolvedCompositeKind {
    Blit {
        alpha: f32,
        blend_mode: BlendMode,
        rounded_mask: Option<RoundedCompositeMask>,
        sample_mode: CompositeSampleMode,
        source_viewport: Option<(f32, f32, f32, f32)>,
    },
    Shader {
        shader: Arc<RuntimeShader>,
        layer_pixel_rect: [f32; 4],
        source_region: Option<(f32, f32, f32, f32)>,
        source_logical_size: Option<(f32, f32)>,
        substrate_regions: SubstrateRegions,
        rounded_mask: Option<RoundedCompositeMask>,
        alpha: f32,
    },
    Projective {
        dest: FrontPolygon,
        inverse: [[f32; 3]; 3],
        alpha: f32,
        blend_mode: BlendMode,
        sample_mode: CompositeSampleMode,
        source_region: Option<(f32, f32, f32, f32)>,
    },
}

/// One scene's contribution to a pass: its ops in z order, the composites
/// resolved for it, where its device space origin sits in the target's
/// scene space, the target pixels it may touch (the whole target when
/// `None`), the transform its device space is drawn under: the identity,
/// except for a layer drawn in place, whose segments carry no composites,
/// and the scale from its scene's logical space to device pixels.
#[derive(Clone)]
pub(crate) struct PassSegment<'a> {
    pub(crate) scene: &'a CompositorScene,
    pub(crate) ops: &'a [DrawOp],
    pub(crate) composites: &'a [ResolvedComposite],
    pub(crate) offset: [f32; 2],
    pub(crate) scissor: Option<(u32, u32, u32, u32)>,
    pub(crate) first_run_window: Option<std::ops::Range<u32>>,
    pub(crate) transform: SegmentTransform,
    pub(crate) scale: f32,
}

impl<'a> PassSegment<'a> {
    // All of `scene`'s ops, with no composites or scissor, untransformed.
    pub(crate) fn plain(scene: &'a CompositorScene, offset: [f32; 2], scale: f32) -> Self {
        Self {
            scene,
            ops: &scene.draw_ops,
            composites: &[],
            offset,
            scissor: None,
            first_run_window: None,
            transform: SegmentTransform::IDENTITY,
            scale,
        }
    }
}

/// How the segments of a pass lie: in order, each over the ones before it,
/// or side by side, as the members of an atlas lie, each within its
/// scissor, which no other segment's touches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SegmentLayout {
    Stacked,
    SideBySide,
}

/// Whether side-by-side segments `a` and `b`, next to each other, share one
/// binding as members (see [`PassPrep::members`]).
fn joins_members(a: &PassSegment<'_>, b: &PassSegment<'_>) -> bool {
    is_member(a) && is_member(b)
}

/// Whether a side-by-side segment can share a binding with others: it is
/// untransformed, without composites and within its scissor, and its offset
/// is whole pixels, so its records snapped to the device grid stay on it in
/// the target's space.
fn is_member(segment: &PassSegment<'_>) -> bool {
    segment.transform.is_identity()
        && segment.composites.is_empty()
        && segment.scissor.is_some()
        && segment.offset[0].fract() == 0.0
        && segment.offset[1].fract() == 0.0
}

enum Item<'a> {
    /// A run, the window of its records the segment draws, and its draw
    /// bounds: see [`run_draw_bounds`].
    Run(&'a RunDraw, Option<std::ops::Range<u32>>, Rect),
    Image(usize),
    /// A text with its logical geometry: see
    /// [`crate::render::text_logical_geometry_for_draw`].
    Text(&'a TextDraw, (Rect, f32)),
    RRectShadow(&'a RRectShadowDraw),
    Composite(&'a ResolvedComposite),
}

enum Batch<'a> {
    StoreRun {
        batch: StoreRunBatch,
        scissor: Option<(u32, u32, u32, u32)>,
    },
    /// Draws of an arena chunk, as ranges of the pass's arena draws: the
    /// ones it paints here, and on the batch that closes the chunk, every
    /// draw of the chunk, whose opaque interiors go down together.
    Arena {
        chunk: usize,
        uniform_slot: usize,
        paint: std::ops::Range<usize>,
        interiors: Option<std::ops::Range<usize>>,
        scissor: Option<(u32, u32, u32, u32)>,
        /// The scissor the painted draws' unturned clip puts on them.
        clip: Option<(u32, u32, u32, u32)>,
    },
    Images {
        cmds: std::ops::Range<usize>,
        blend_mode: BlendMode,
        uniform_slot: usize,
        scissor: Option<(u32, u32, u32, u32)>,
    },
    Glyphs {
        cmds: std::ops::Range<usize>,
        uniform_slot: usize,
        scissor: Option<(u32, u32, u32, u32)>,
    },
    Shadows {
        instances: std::ops::Range<u32>,
        uniform_slot: usize,
        scissor: Option<(u32, u32, u32, u32)>,
    },
    Composite(PreparedCompositeDraw<'a>),
    Shader(PreparedShaderDraw<'a>),
    Projective(PreparedProjectiveComposite<'a>),
    Instant(InstantDraw<'a>),
}

/// The scissor a shape batch's `stage` draws under: the batch's, and for its
/// paint only the pixels its unturned clip keeps, `None` when none are left.
/// The interiors keep the batch's: their pipeline cuts each to its clip.
fn stage_scissor(
    scissor: TargetRect,
    clip: Option<TargetRect>,
    stage: RunStage,
) -> Option<TargetRect> {
    match (stage, clip) {
        (RunStage::Paint, Some(clip)) => crate::render::intersect_target_rects(scissor, clip),
        _ => Some(scissor),
    }
}

fn composite_visible(
    composite: &ResolvedComposite,
    target_size: (u32, u32),
    segment_offset: [f32; 2],
    segment_scissor: Option<(u32, u32, u32, u32)>,
) -> bool {
    let (x, y, width, height) = dest_in_target(composite.dest, segment_offset);
    let (left, top, right, bottom) = match segment_scissor {
        Some((sx, sy, sw, sh)) => (sx as f32, sy as f32, (sx + sw) as f32, (sy + sh) as f32),
        None => (0.0, 0.0, target_size.0 as f32, target_size.1 as f32),
    };
    if x >= right || y >= bottom || x + width <= left || y + height <= top {
        return false;
    }
    composite
        .scissor
        .is_none_or(|scissor| scissor_in_target(scissor, target_size, segment_offset).is_some())
}

impl GpuRenderer {
    /// Uploads the quads a prepared pass drew into its scratch: image
    /// vertices, shadow instances and glyph instances, each when there are
    /// any.
    fn upload_pass_buffers<C: FrameCommandRecorder>(
        &self,
        recorder: &mut C,
        scratch: &PassScratch,
    ) -> PassBuffers {
        let builder = &scratch.meshes.builder;
        PassBuffers {
            meshes: (!builder.is_empty()).then(|| crate::mesh_fallback::MeshBuffers {
                vertices: recorder.upload_buffer(
                    crate::frame_graph::UploadAllocatorSpec::vertex(
                        "Mesh Fallback Vertex Buffer",
                        size_of::<crate::mesh_fallback::MeshVertex>() as u64,
                    ),
                    &self.device,
                    bytemuck::cast_slice(builder.vertices()),
                ),
                indices: recorder.upload_buffer(
                    crate::frame_graph::UploadAllocatorSpec::index(
                        "Mesh Fallback Index Buffer",
                        size_of::<u16>() as u64,
                    ),
                    &self.device,
                    bytemuck::cast_slice(builder.indices()),
                ),
            }),
            images: (!scratch.images.indices.is_empty()).then(|| {
                ImageSlot::upload(
                    recorder,
                    &self.device,
                    &scratch.images.vertices,
                    &scratch.images.indices,
                    &scratch.images.clips,
                )
            }),
            shadows: (!scratch.shadow_instances.is_empty())
                .then(|| self.upload_shadow_instances(recorder, &scratch.shadow_instances)),
            glyphs: (!scratch.glyph_instances.plain.is_empty())
                .then(|| self.upload_glyph_instances(recorder, &scratch.glyph_instances.plain)),
            turned_glyphs: (!scratch.glyph_instances.turned.is_empty())
                .then(|| self.upload_turned_glyphs(recorder, &scratch.glyph_instances.turned)),
            instant: scratch.instant.upload(recorder, &self.device),
        }
    }

    /// Draws the segments into the target as one render pass, ops and
    /// composites interleaved in z order. Returns whether anything was drawn;
    /// when nothing draws and the load op clears, a clear pass runs instead so
    /// the target still holds its base.
    pub(crate) fn encode_pass<'s, C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        target: PassTarget<'_>,
        (segments, layout): (&'s [PassSegment<'s>], SegmentLayout),
        load_op: wgpu::LoadOp<wgpu::Color>,
        label: &'static str,
    ) -> Result<bool, String> {
        let mut scratch = self.take_pass_scratch();
        let (depth, depth_keys) = self.pass_depth(segments);
        let mut prep = PassPrep::new(
            recorder,
            (target, load_op),
            (depth, depth_keys),
            turns_mixed(segments),
            &mut scratch,
        );
        let prepared = prep.segments(self, (segments, layout), &mut scratch);
        prep.finish(self);
        let mut parts = self.draw_parts();
        parts.text.flush_glyph_uploads(&mut parts.upload);
        self.prefetch_overlay_pipelines(
            &prep.batches,
            &scratch,
            (depth, self.pipelines.frame_lane()),
            &mut |_| {},
        );
        if let Some(depth_keys) = prep.depth_keys.take() {
            self.await_depth_pipelines(&prep.batches, &scratch, &depth_keys);
        }
        self.pipelines.ensure_run_pipelines(&mut prep.pipelines);
        let laid = (&scratch.glyph_instances, &mut scratch.instant);
        self.pipelines.lay_turned_glyphs(depth, laid.0, laid.1);
        let batches = prep.batches;
        scratch.arena_draws = prep.arena_draws;
        scratch.meshes = prep.meshes;
        scratch.pipelines = prep.pipelines;
        scratch.held = prep.held;
        let buffers = if prepared.is_ok() {
            self.upload_pass_buffers(recorder, &scratch)
        } else {
            PassBuffers::default()
        };
        let result = match prepared {
            Err(error) => Err(error),
            Ok(()) if batches.is_empty() => {
                if matches!(load_op, wgpu::LoadOp::Clear(_)) {
                    self.clear_target(recorder, target.view, load_op);
                }
                Ok(false)
            }
            Ok(()) => {
                let composite_draws = batches
                    .iter()
                    .filter(|batch| matches!(batch, Batch::Composite(_) | Batch::Shader(_)))
                    .count() as u32;
                if composite_draws > 0 {
                    self.effect_renderer.record_composite_pass();
                    self.frame_stats.add_draw_calls(composite_draws);
                }
                group_glyph_batches(&batches, &mut scratch);
                let draw_result = {
                    let frame = PassFrame {
                        size: (target.width, target.height),
                        depth,
                    };
                    let mut pass = self.begin_scene_pass(recorder, label, target, load_op, depth);
                    self.draw_batches(
                        &mut TrackedPass::new(&mut pass),
                        frame,
                        &batches,
                        PassCmds {
                            images: &scratch.images.cmds,
                            glyphs: &scratch.glyph_cmds,
                            arena: &scratch.arena_draws,
                            meshes: &scratch.meshes,
                        },
                        &buffers,
                    )
                };
                recorder.record_pass();
                draw_result.map(|()| true)
            }
        };
        drop(batches);
        self.pass_scratch = scratch;
        result
    }

    // The depth a pass over `segments` draws with, and the shape pipelines
    // it collects for a depth buffer when it could take one but its gate is
    // still shut.
    fn pass_depth(&self, segments: &[PassSegment<'_>]) -> (PassDepth, Option<PipelineDemand>) {
        let takes_depth = self.depth_gate.may() && takes_depth(segments);
        if takes_depth && self.depth_gate.is_open() {
            (PassDepth::On, None)
        } else {
            (PassDepth::Off, takes_depth.then(PipelineDemand::default))
        }
    }

    fn take_pass_scratch(&mut self) -> PassScratch {
        let mut scratch = std::mem::take(&mut self.pass_scratch);
        scratch.meshes.clear();
        scratch.arena_draws.clear();
        scratch.images.vertices.clear();
        scratch.images.indices.clear();
        scratch.images.cmds.clear();
        scratch.images.clips.clear();
        scratch.glyph_instances.clear();
        scratch.glyph_cmds.clear();
        scratch.shadow_instances.clear();
        scratch.instant.clear();
        scratch
    }

    /// Starts on `lane` the builds of the image, glyph and shadow pipelines a
    /// prepared pass draws with, with a depth buffer or not, that are
    /// missing, handing `awaited` each still to land;
    /// [`GpuRenderer::ensure_run_pipelines`] starts its shapes' and the
    /// pass's composites start theirs as it prepares them. A launch's first
    /// frame needs about fifteen; built one after another where the pass
    /// reached each, they held the gauntlet's first frame 1.1 s on an Apple
    /// M5 with no Metal shader cached. Where the compiler's threads run, the
    /// pass leaves out a draw whose pipeline is still to land; elsewhere it
    /// builds each where it draws with it.
    fn prefetch_overlay_pipelines(
        &self,
        batches: &[Batch<'_>],
        scratch: &PassScratch,
        depth_lane: (PassDepth, CompileLane),
        awaited: &mut impl FnMut(Pending<wgpu::RenderPipeline>),
    ) {
        for batch in batches {
            match batch {
                Batch::Images {
                    cmds, blend_mode, ..
                } => self.pipelines.prefetch_image_pipelines(
                    &scratch.images.cmds[cmds.clone()],
                    *blend_mode,
                    depth_lane,
                    awaited,
                ),
                Batch::Glyphs { cmds, .. } => self.pipelines.prefetch_glyph_pipelines(
                    &scratch.glyph_cmds[cmds.clone()],
                    depth_lane,
                    awaited,
                ),
                Batch::Shadows { .. } => {
                    self.pipelines.prefetch_shadow_pipeline(depth_lane, awaited);
                }
                Batch::Composite(prepared) => self.effect_renderer.prefetch_composite_pipeline(
                    &self.device,
                    prepared,
                    depth_lane,
                    awaited,
                ),
                Batch::Projective(prepared) => self.effect_renderer.prefetch_projective_pipeline(
                    &self.device,
                    prepared,
                    depth_lane,
                    awaited,
                ),
                Batch::StoreRun { .. }
                | Batch::Arena { .. }
                | Batch::Shader(_)
                | Batch::Instant(_) => {}
            }
        }
    }

    /// Queues for a later frame the pipelines a pass drawn without the
    /// depth pre-pass it takes would have drawn with: its shapes' with
    /// `depth_keys` and the interiors they lay down, and its images', glyphs',
    /// shadows' and blits' with a depth buffer. The depth gate opens once they
    /// land.
    fn await_depth_pipelines(
        &mut self,
        batches: &[Batch<'_>],
        scratch: &PassScratch,
        depth_keys: &PipelineDemand,
    ) {
        let mut awaited = self.depth_gate.take_awaited();
        self.pipelines.warm_shapes_later(depth_keys, &mut awaited);
        self.prefetch_overlay_pipelines(
            batches,
            scratch,
            (PassDepth::On, CompileLane::Later),
            &mut |pending| awaited.push(pending),
        );
        self.depth_gate.waited_for(awaited);
    }

    fn draw_batches(
        &mut self,
        pass: &mut TrackedPass<'_, '_>,
        frame: PassFrame,
        batches: &[Batch<'_>],
        cmds: PassCmds<'_>,
        buffers: &PassBuffers,
    ) -> Result<(), String> {
        if frame.depth.is_on() {
            for batch in batches.iter().rev() {
                self.draw_shape_batch(pass, batch, cmds.arena, None, frame, RunStage::Interiors)?;
            }
        }
        let meshes = buffers.run_meshes(cmds.meshes);
        for batch in batches {
            match batch {
                Batch::StoreRun { .. } | Batch::Arena { .. } => {
                    self.draw_shape_batch(pass, batch, cmds.arena, meshes, frame, RunStage::Paint)?;
                }
                Batch::Images {
                    cmds: range,
                    blend_mode,
                    uniform_slot,
                    scissor,
                } => {
                    let slot = buffers
                        .images
                        .as_ref()
                        .ok_or_else(|| "image batch without an image slot".to_string())?;
                    self.draw_image_cmds(
                        pass,
                        slot,
                        *uniform_slot,
                        &cmds.images[range.clone()],
                        (*blend_mode, frame.depth),
                        *scissor,
                    )?;
                }
                Batch::Glyphs {
                    cmds: range,
                    uniform_slot,
                    scissor,
                } => {
                    let stand_in = self.pipelines.instant_stand_in(frame.depth);
                    self.draw_glyph_cmds(
                        pass,
                        (buffers.glyphs.as_ref(), buffers.turned_glyphs(stand_in)),
                        *uniform_slot,
                        &cmds.glyphs[range.clone()],
                        *scissor,
                        frame,
                    )?;
                }
                Batch::Shadows {
                    instances,
                    uniform_slot,
                    scissor,
                } => {
                    let buffer = buffers
                        .shadows
                        .as_ref()
                        .ok_or_else(|| "shadow batch without shadow instances".to_string())?;
                    self.draw_shadow_instances(
                        pass,
                        buffer,
                        *uniform_slot,
                        instances.clone(),
                        *scissor,
                        frame,
                    )?;
                }
                Batch::Composite(_)
                | Batch::Shader(_)
                | Batch::Projective(_)
                | Batch::Instant(_) => self.draw_layer(pass, frame, batch, buffers)?,
            }
        }
        Ok(())
    }

    // Draws a layer's batch: its composite, through a runtime shader or
    // tilted, or the instant triangles standing in for it; other batches
    // draw nothing here.
    fn draw_layer(
        &mut self,
        pass: &mut TrackedPass<'_, '_>,
        frame: PassFrame,
        batch: &Batch<'_>,
        buffers: &PassBuffers,
    ) -> Result<(), String> {
        let target_size = frame.size;
        match batch {
            Batch::Composite(prepared) => self.effect_renderer.draw_prepared_composite(
                pass.untracked(),
                (&self.device, target_size),
                prepared,
            ),
            Batch::Shader(prepared) => self.effect_renderer.draw_prepared_shader_src_over(
                pass.untracked(),
                target_size,
                prepared,
            ),
            Batch::Projective(prepared) => self.effect_renderer.draw_prepared_projective_composite(
                pass.untracked(),
                (&self.device, target_size),
                prepared,
            ),
            Batch::Instant(draw) => {
                let context = (&self.viewport_uniforms, &self.frame_stats);
                let laid = buffers.instant.as_ref();
                return self
                    .pipelines
                    .draw_instant(pass, (frame, draw), laid, context);
            }
            _ => {}
        }
        Ok(())
    }

    /// Begins the pass `encode_pass` records into, with a depth buffer when
    /// its opaque interiors go down first.
    fn begin_scene_pass<'p, C: FrameCommandRecorder>(
        &mut self,
        recorder: &'p mut C,
        label: &'static str,
        target: PassTarget<'_>,
        load_op: wgpu::LoadOp<wgpu::Color>,
        depth: PassDepth,
    ) -> wgpu::RenderPass<'p> {
        if depth.is_on() {
            self.frame_stats
                .depth_passes
                .set(self.frame_stats.depth_passes.get() + 1);
            let depth_view = self
                .depth_targets
                .interior(&self.device, (target.width, target.height));
            recorder.begin_depth_pass(label, target.view, load_op, &depth_view)
        } else {
            recorder.begin_color_pass(label, target.view, load_op)
        }
    }

    /// Records one shape batch's stage; other batches draw nothing here.
    /// An arena batch's draws are ranges of `arena`.
    fn draw_shape_batch(
        &self,
        pass: &mut TrackedPass<'_, '_>,
        batch: &Batch<'_>,
        arena: &[RunDrawCall],
        meshes: Option<crate::mesh_fallback::RunMeshes<'_>>,
        frame: PassFrame,
        stage: RunStage,
    ) -> Result<(), String> {
        let meshes = meshes.map(|meshes| (meshes, frame.depth));
        match batch {
            Batch::StoreRun { batch, scissor } => {
                match stage_scissor(frame.scissor(*scissor), batch.clip, stage) {
                    Some(scissor) => {
                        self.draw_store_run(pass, batch, arena, scissor, stage, meshes)
                    }
                    None => Ok(()),
                }
            }
            Batch::Arena {
                chunk,
                uniform_slot,
                paint,
                interiors,
                scissor,
                clip,
            } => {
                let draws = match stage {
                    RunStage::Paint => paint.clone(),
                    RunStage::Interiors => match interiors {
                        Some(interiors) => interiors.clone(),
                        None => return Ok(()),
                    },
                };
                let Some(scissor) = stage_scissor(frame.scissor(*scissor), *clip, stage) else {
                    return Ok(());
                };
                let meshes = meshes.map(|(meshes, depth)| (meshes.of(draws.clone()), depth));
                let calls = RunCalls {
                    uniform_slot: *uniform_slot,
                    draws: &arena[draws],
                    scissor,
                    stage,
                    meshes,
                };
                self.draw_arena(pass, *chunk, calls)
            }
            _ => Ok(()),
        }
    }
}

// A batch of run draw calls: the uniform slot and scissor they draw under,
// the calls, the stage they draw and the meshes a call falls back to while
// its pipeline compiles, with the depth buffer or not.
#[derive(Clone, Copy)]
pub(crate) struct RunCalls<'a> {
    pub(crate) uniform_slot: usize,
    pub(crate) draws: &'a [RunDrawCall],
    pub(crate) scissor: (u32, u32, u32, u32),
    pub(crate) stage: RunStage,
    pub(crate) meshes: Option<(crate::mesh_fallback::RunMeshes<'a>, PassDepth)>,
}

/// Changes between flat and turned segments a pass takes before its shapes
/// draw with the pipeline that tells them apart per record: below it, a draw
/// per change costs less than the flag test every fragment pays there.
const MIXED_TURN_CHANGES: usize = 8;

/// Whether the flat and turned segments of a pass alternate at least
/// `MIXED_TURN_CHANGES` times, as a grid of cells tilted by turns does when
/// some cells are not tilted.
fn turns_mixed(segments: &[PassSegment<'_>]) -> bool {
    segments
        .windows(2)
        .filter(|pair| pair[0].transform.is_identity() != pair[1].transform.is_identity())
        .count()
        >= MIXED_TURN_CHANGES
}

/// The per-frame vectors a pass fills: image and glyph geometry and draw
/// commands, kept on the renderer between frames so they never reallocate.
/// The quads a pass drew from its scratch, uploaded for its draws: fallback
/// triangles, image vertices and indices, and glyph instances.
#[derive(Default)]
struct PassBuffers {
    meshes: Option<crate::mesh_fallback::MeshBuffers>,
    images: Option<ImageSlot>,
    shadows: Option<crate::frame_graph::BufferUpload>,
    glyphs: Option<crate::frame_graph::BufferUpload>,
    turned_glyphs: Option<crate::frame_graph::BufferUpload>,
    instant: Option<crate::instant_triangles::InstantBuffers>,
}

impl PassBuffers {
    // Where the pass draws its turned glyphs from: see `TurnedGlyphs`.
    fn turned_glyphs<'a>(&'a self, stand_in: Option<InstantStandIn<'a>>) -> TurnedGlyphs<'a> {
        let laid = self
            .instant
            .as_ref()
            .and_then(|laid| laid.glyphs(stand_in?));
        laid.map_or(
            TurnedGlyphs::Own(self.turned_glyphs.as_ref()),
            TurnedGlyphs::Instant,
        )
    }

    /// The pass's fallback triangles, with the index range of each run draw
    /// of `meshes` that takes some.
    fn run_meshes<'a>(
        &'a self,
        meshes: &'a crate::mesh_fallback::PassMeshes,
    ) -> Option<crate::mesh_fallback::RunMeshes<'a>> {
        self.meshes
            .as_ref()
            .map(|buffers| crate::mesh_fallback::RunMeshes {
                ranges: &meshes.ranges,
                chunks: meshes.builder.chunks(),
                buffers,
            })
    }
}

/// The commands a pass's batches draw ranges of.
#[derive(Clone, Copy)]
struct PassCmds<'a> {
    images: &'a [crate::render::ImageDrawCmd],
    glyphs: &'a [crate::render::GlyphDrawCmd],
    arena: &'a [RunDrawCall],
    /// The pass's fallback triangles: the index range of each of `arena`'s
    /// draws that takes one, and the chunks the indices count in.
    meshes: &'a crate::mesh_fallback::PassMeshes,
}

/// Draws each glyph batch's retained runs after its shared quads, and its
/// labels one kind, turned or upright, at a time, where their order allows:
/// see [`crate::render::defer_retained_glyph_runs`] and
/// [`crate::render::group_glyph_kinds`].
fn group_glyph_batches(batches: &[Batch<'_>], scratch: &mut PassScratch) {
    for batch in batches {
        if let Batch::Glyphs { cmds, .. } = batch {
            let cmds = &mut scratch.glyph_cmds[cmds.clone()];
            let shared = crate::render::defer_retained_glyph_runs(cmds, &mut scratch.glyph_moved);
            crate::render::group_glyph_kinds(&mut cmds[..shared], &mut scratch.glyph_moved);
        }
    }
}

pub(crate) fn target_rects_overlap(a: TargetRect, b: TargetRect) -> bool {
    a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
}

/// Turns the segments of one pass into batches, one item run at a time.
struct PassPrep<'a, 's, C> {
    recorder: &'a mut C,
    target: PassTarget<'a>,
    load_op: wgpu::LoadOp<wgpu::Color>,
    batches: Vec<Batch<'s>>,
    /// The arena chunk shapes are being appended to, kept open across held
    /// draws.
    chunk: Option<usize>,
    held: HeldDraws,
    /// Whether the pass has a depth buffer its opaque interiors fill first.
    depth: PassDepth,
    /// Whether the pass's flat and turned segments alternate often enough
    /// that its shapes draw with the pipeline telling them apart per record.
    mixed_turns: bool,
    /// The binding slot of the last glyph batch pushed, which a later
    /// draw under that binding may extend.
    overlay_segment: Option<usize>,
    /// The viewport of the last image batch pushed, which a later image
    /// draw under it may extend.
    overlay_images: Option<ViewportUniformParams>,
    /// The pass-order index the next shape batch's records start at.
    depth_seq: u32,
    /// What the open chunk and held glyphs draw with. Consecutive segments
    /// that bind the same keep adding to them: layers drawn in place carry
    /// their turns in their records and glyphs, not in what they bind.
    open: Option<SegmentBinding>,
    /// Every run draw of the pass, arena chunks' and stored runs', which
    /// their batches draw ranges of.
    arena_draws: Vec<RunDrawCall>,
    /// The fallback triangles of the run draws whose pipelines compile.
    meshes: crate::mesh_fallback::PassMeshes,
    /// The shape pipelines the pass's run draws take, prepared once its
    /// batches are made.
    pipelines: PipelineDemand,
    /// While the depth gate holds back a pre-pass the pass takes, the shape
    /// pipelines its run draws would have taken with it.
    depth_keys: Option<PipelineDemand>,
    /// Where the open chunk's draws start in `arena_draws`.
    chunk_start: usize,
    /// The pass-order index of the open chunk's first record.
    chunk_base: u32,
    /// The uniform slot the open chunk's batches bind, once one is pushed.
    chunk_slot: Option<usize>,
    /// The scissor the unturned clip of the records appended since the
    /// chunk's last cut puts on their paint: a run clipped otherwise cuts
    /// the chunk first.
    chunk_clip: Option<TargetRect>,
    /// The scissor every draw of the open chunk takes, its interiors' too:
    /// its binding's, or the slot of a member whose run may reach past it.
    /// A run that takes another closes the chunk first.
    chunk_scissor: Option<TargetRect>,
}

impl<'a, 's, C: FrameCommandRecorder> PassPrep<'a, 's, C> {
    fn new(
        recorder: &'a mut C,
        (target, load_op): (PassTarget<'a>, wgpu::LoadOp<wgpu::Color>),
        (depth, depth_keys): (PassDepth, Option<PipelineDemand>),
        mixed_turns: bool,
        scratch: &mut PassScratch,
    ) -> Self {
        Self {
            recorder,
            target,
            load_op,
            batches: Vec::new(),
            chunk: None,
            held: std::mem::take(&mut scratch.held),
            depth,
            mixed_turns,
            overlay_segment: None,
            overlay_images: None,
            depth_seq: 0,
            open: None,
            arena_draws: std::mem::take(&mut scratch.arena_draws),
            meshes: std::mem::take(&mut scratch.meshes),
            pipelines: std::mem::take(&mut scratch.pipelines),
            depth_keys,
            chunk_start: 0,
            chunk_base: 0,
            chunk_slot: None,
            chunk_clip: None,
            chunk_scissor: None,
        }
    }

    fn target_size(&self) -> (u32, u32) {
        (self.target.width, self.target.height)
    }

    /// Batches the pass's segments in order; side-by-side members that share
    /// a binding go together (see [`Self::members`]).
    fn segments(
        &mut self,
        renderer: &mut GpuRenderer,
        (segments, layout): (&'s [PassSegment<'s>], SegmentLayout),
        scratch: &mut PassScratch,
    ) -> Result<(), String> {
        match layout {
            SegmentLayout::Stacked => segments
                .iter()
                .try_for_each(|segment| self.segment(renderer, segment, scratch)),
            SegmentLayout::SideBySide => {
                segments
                    .chunk_by(joins_members)
                    .try_for_each(|members| match members {
                        [segment] => self.segment(renderer, segment, scratch),
                        members => self.members(renderer, members, scratch),
                    })
            }
        }
    }

    /// The viewport `segment` draws under in the pass's target.
    fn viewport_of(&self, segment: &PassSegment<'_>) -> ViewportUniformParams {
        ViewportUniformParams {
            width: self.target.width,
            height: self.target.height,
            offset: segment.offset,
            transform: segment.transform,
            origin: [0.0; 2],
            depth_base: 0.0,
        }
    }

    /// The open binding when it binds `bound` under `scissor`, else a fresh
    /// one, once what the open one holds is drawn.
    fn bind(
        &mut self,
        renderer: &mut GpuRenderer,
        bound: ViewportUniformParams,
        scissor: Option<TargetRect>,
    ) -> SegmentBinding {
        match self.open {
            Some(open) if open.bound == bound && open.scissor == scissor => open,
            _ => {
                self.finish(renderer);
                let open = SegmentBinding {
                    uniform_slot: renderer
                        .viewport_uniforms
                        .claim_viewport(&renderer.pipelines, bound),
                    bound,
                    scissor,
                };
                self.open = Some(open);
                // A batch pushed under another binding takes no later draws.
                self.overlay_segment = None;
                self.overlay_images = None;
                open
            }
        }
    }

    fn segment(
        &mut self,
        renderer: &mut GpuRenderer,
        segment: &PassSegment<'s>,
        scratch: &mut PassScratch,
    ) -> Result<(), String> {
        debug_assert!(
            segment.transform.is_identity() || segment.composites.is_empty(),
            "a transformed segment places its composites nowhere"
        );
        let viewport = self.viewport_of(segment);
        let bound = ViewportUniformParams {
            transform: SegmentTransform::IDENTITY,
            ..viewport
        };
        let binding = self.bind(renderer, bound, segment.scissor);
        let run = SegmentRun {
            segment,
            viewport,
            binding,
            shift: [0.0; 2],
            slot: None,
        };
        let viewport_rect = segment_viewport_rect(self.target, segment);
        let mut items = merge_items(
            segment,
            viewport_rect,
            self.target_size(),
            Texts::under(&renderer.ablation),
        )
        .peekable();
        self.items(renderer, &mut items, &run, scratch)
    }

    /// Draws `members`, segments side by side that [`is_member`] lets share
    /// one binding, without their scissors: the binding draws the target's
    /// own space, each member's records, glyphs and images move into it by
    /// the member's offset, and a draw that may reach past a member's
    /// scissor keeps to it. A glyph or image quad moves on the CPU by the
    /// one subtraction the vertex stage made of it, so it lands where it
    /// did. Their ops go in lockstep, every member's first, then every
    /// member's second, so members drawn from one template, as an atlas's
    /// cards are, fill each batch together; no pixel of one is another's,
    /// so the order between them shows nowhere.
    fn members(
        &mut self,
        renderer: &mut GpuRenderer,
        members: &[PassSegment<'s>],
        scratch: &mut PassScratch,
    ) -> Result<(), String> {
        let Some(first) = members.first() else {
            return Ok(());
        };
        let bound = ViewportUniformParams {
            offset: [0.0; 2],
            ..self.viewport_of(first)
        };
        let binding = self.bind(renderer, bound, None);
        let ops = members
            .iter()
            .map(|member| member.ops.len())
            .max()
            .unwrap_or(0);
        for op_index in 0..ops {
            for segment in members {
                let Some(op) = segment.ops.get(op_index) else {
                    continue;
                };
                let run = SegmentRun {
                    segment,
                    viewport: self.viewport_of(segment),
                    binding,
                    shift: [-segment.offset[0], -segment.offset[1]],
                    slot: segment.scissor,
                };
                let window = (op_index == 0)
                    .then(|| segment.first_run_window.clone())
                    .flatten();
                let viewport_rect = segment_viewport_rect(self.target, segment);
                let mut items = OpItems::of(
                    segment.scene,
                    op,
                    window,
                    (
                        viewport_rect,
                        segment.scale,
                        Texts::under(&renderer.ablation),
                    ),
                )
                .peekable();
                self.items(renderer, &mut items, &run, scratch)?;
            }
        }
        Ok(())
    }

    /// Batches `items`, drawn as `run` places them.
    fn items(
        &mut self,
        renderer: &mut GpuRenderer,
        items: &mut Peekable<impl Iterator<Item = Item<'s>>>,
        run: &SegmentRun<'s, '_>,
        scratch: &mut PassScratch,
    ) -> Result<(), String> {
        while let Some(item) = items.peek() {
            match item {
                Item::Run(..) => {
                    self.run_items(renderer, items, run);
                    continue;
                }
                Item::Image(_) => {
                    self.image_items(renderer, items, run, scratch)?;
                    continue;
                }
                Item::Text(text, geometry) => {
                    self.text_item(renderer, (text, *geometry), run, scratch)?;
                }
                Item::RRectShadow(_) => {
                    self.flush(renderer, run.binding);
                    self.shadow_run(renderer, items, run, scratch);
                    continue;
                }
                Item::Composite(composite) => {
                    self.flush(renderer, run.binding);
                    self.composite_item(renderer, composite, run, scratch)?;
                }
            }
            items.next();
        }
        Ok(())
    }

    /// Draws what the open binding still holds; the next segment binds
    /// afresh.
    fn finish(&mut self, renderer: &mut GpuRenderer) {
        if let Some(open) = self.open.take() {
            self.flush(renderer, open);
        }
    }

    /// Opens the arena chunk shapes append to next, its records placed
    /// next in the pass's order.
    fn open_chunk(&mut self, renderer: &mut GpuRenderer) -> usize {
        let open = renderer.open_arena();
        self.chunk = Some(open);
        self.chunk_start = self.arena_draws.len();
        self.chunk_base = self.depth_seq;
        self.chunk_slot = None;
        open
    }

    /// The uniform slot the open chunk's batches bind: the segment's, or in a
    /// pass with a depth buffer one that places the chunk's records from its
    /// base in the pass's order.
    fn chunk_uniform_slot(&mut self, renderer: &mut GpuRenderer, binding: SegmentBinding) -> usize {
        if let Some(slot) = self.chunk_slot {
            return slot;
        }
        let slot = if self.depth.is_on() {
            renderer.viewport_uniforms.claim_viewport(
                &renderer.pipelines,
                ViewportUniformParams {
                    depth_base: self.chunk_base as f32,
                    ..binding.bound
                },
            )
        } else {
            binding.uniform_slot
        };
        self.chunk_slot = Some(slot);
        slot
    }

    /// Closes the open arena chunk: a batch paints its draws since the last
    /// cut and lays down the opaque interiors of all of its draws.
    fn close_chunk(&mut self, renderer: &mut GpuRenderer, binding: SegmentBinding) {
        let Some(open) = self.chunk.take() else {
            return;
        };
        let records = renderer.open_arena_records();
        let paint_start = self.arena_draws.len();
        renderer.close_arena(open, &mut self.arena_draws);
        let end = self.arena_draws.len();
        self.mesh_arena(renderer, binding, paint_start..end);
        if end == self.chunk_start {
            return;
        }
        let uniform_slot = self.chunk_uniform_slot(renderer, binding);
        if self.depth.is_on() {
            self.depth_seq = self.chunk_base.saturating_add(records);
        }
        self.batches.push(Batch::Arena {
            chunk: open,
            uniform_slot,
            paint: paint_start..end,
            interiors: Some(self.chunk_start..end),
            scissor: self.chunk_scissor,
            clip: self.chunk_clip,
        });
    }

    /// Draws what is held where the open chunk has got to, and keeps the
    /// chunk open: its shapes so far paint first and the held draws above
    /// them, while its later records, and the opaque interiors of all of
    /// them, stay one chunk. Without an open chunk it draws what is held.
    fn draw_held(&mut self, renderer: &mut GpuRenderer, binding: SegmentBinding) {
        let Some(open) = self.chunk else {
            self.flush(renderer, binding);
            return;
        };
        let held = self.held.take();
        if held.glyphs.is_none() && held.images.is_none() {
            return;
        }
        self.cut_paint(renderer, binding, open);
        // The held draws sit where the chunk's next record will: its later
        // opaque interiors hide them, its earlier ones do not.
        let base = self
            .chunk_base
            .saturating_add(renderer.open_arena_records());
        self.push_held(renderer, binding, held, Some(base));
    }

    /// Lays out as fallback triangles the chunk's draws in `range`, just
    /// cut or closed, whose shape pipelines still compile, placed as the
    /// chunk's uniform places them.
    fn mesh_arena(
        &mut self,
        renderer: &GpuRenderer,
        binding: SegmentBinding,
        range: std::ops::Range<usize>,
    ) {
        let viewport = ViewportUniformParams {
            depth_base: self.chunk_base as f32,
            ..binding.bound
        };
        renderer.mesh_arena_draws(
            viewport,
            range,
            &self.arena_draws,
            self.depth,
            &mut self.meshes,
        );
    }

    /// Paints the open chunk's draws since its last cut, under the clip
    /// their records share, and keeps the chunk open.
    fn cut_paint(&mut self, renderer: &mut GpuRenderer, binding: SegmentBinding, open: usize) {
        let paint_start = self.arena_draws.len();
        renderer.cut_arena(open, &mut self.arena_draws);
        let end = self.arena_draws.len();
        self.mesh_arena(renderer, binding, paint_start..end);
        if end > paint_start {
            let uniform_slot = self.chunk_uniform_slot(renderer, binding);
            self.batches.push(Batch::Arena {
                chunk: open,
                uniform_slot,
                paint: paint_start..end,
                interiors: None,
                scissor: self.chunk_scissor,
                clip: self.chunk_clip,
            });
        }
    }

    /// The uniform slot a batch of overlays drawn under `viewport` binds.
    /// In a pass with a depth buffer it places them at `base` in the pass's
    /// order, or next in it without a base, so later opaque interiors hide
    /// them and earlier ones do not; otherwise it is the binding's own slot
    /// when that binds `viewport`.
    fn placed_slot(
        &mut self,
        renderer: &mut GpuRenderer,
        binding: SegmentBinding,
        viewport: ViewportUniformParams,
        base: Option<u32>,
    ) -> usize {
        if self.depth.is_on() {
            let base = base.unwrap_or_else(|| self.next_place());
            renderer.viewport_uniforms.claim_viewport(
                &renderer.pipelines,
                ViewportUniformParams {
                    depth_base: base as f32,
                    ..viewport
                },
            )
        } else if viewport == binding.bound {
            binding.uniform_slot
        } else {
            renderer
                .viewport_uniforms
                .claim_viewport(&renderer.pipelines, viewport)
        }
    }

    /// Takes the next place in the pass's order.
    fn next_place(&mut self) -> u32 {
        let place = self.depth_seq;
        self.depth_seq = place.saturating_add(1);
        place
    }

    /// A composite's place in a pass with a depth buffer: next in its order,
    /// so later opaque interiors hide it and earlier ones do not.
    fn composite_place(&mut self) -> Option<u32> {
        self.depth.is_on().then(|| self.next_place())
    }

    // Pushes a layer's composite or, while its pipeline compiles and the
    // instant one is built, the instant triangles `lay` lays with its
    // surface `stand_in`, drawn at `place` within `scissor`.
    fn push_layer(
        &mut self,
        (renderer, run): (&mut GpuRenderer, &SegmentRun<'s, '_>),
        (composite, stand_in): (Batch<'s>, Option<&'s wgpu::BindGroup>),
        (scissor, place): (Option<TargetRect>, Option<u32>),
        lay: impl FnOnce() -> std::ops::Range<u32>,
    ) {
        let instant = renderer.pipelines.instant_stand_in(self.depth);
        let Some(texture) = stand_in.filter(|_| instant.is_some()) else {
            return self.batches.push(composite);
        };
        let uniform_slot = self.placed_slot(renderer, run.binding, run.binding.bound, place);
        let draw = InstantDraw::layer(texture, uniform_slot, lay(), scissor);
        self.batches.push(Batch::Instant(draw));
    }

    /// The viewport a text draws its glyphs under: in a pass with a depth
    /// buffer, placed where the text falls in the pass's order, after the
    /// records the open chunk holds. A retained run claims its own slot
    /// from it; later records of the chunk do not touch the held text.
    fn text_viewport(
        &self,
        renderer: &GpuRenderer,
        run: &SegmentRun<'s, '_>,
    ) -> ViewportUniformParams {
        if !self.depth.is_on() {
            return run.viewport;
        }
        let open = self.chunk.map_or(0, |_| renderer.open_arena_records());
        ViewportUniformParams {
            depth_base: self.depth_seq.saturating_add(open) as f32,
            ..run.viewport
        }
    }

    /// Reserves the pass-order indices of the records of the run draws in
    /// `draws`, which are instanced from their table's start.
    fn take_depth_range(&mut self, draws: std::ops::Range<usize>) {
        let records = self.arena_draws[draws]
            .iter()
            .map(|draw| draw.records.end)
            .max()
            .unwrap_or(0);
        self.depth_seq = self.depth_seq.saturating_add(records);
    }

    /// Draws everything held: the open chunk's shapes, then the held draws
    /// above them.
    fn flush(&mut self, renderer: &mut GpuRenderer, binding: SegmentBinding) {
        self.close_chunk(renderer, binding);
        let held = self.held.take();
        self.push_held(renderer, binding, held, None);
    }

    /// Pushes the held glyphs, then the held images, placed in a pass with
    /// a depth buffer at `base` or next in its order.
    fn push_held(
        &mut self,
        renderer: &mut GpuRenderer,
        binding: SegmentBinding,
        held: HeldCmds,
        base: Option<u32>,
    ) {
        if let Some(cmds) = held.glyphs {
            self.push_glyphs(renderer, binding, cmds, base);
        }
        if let Some((cmds, viewport)) = held.images {
            self.push_images(
                renderer,
                (binding, viewport),
                cmds,
                (BlendMode::SrcOver, base),
            );
        }
    }

    /// Pushes glyph commands, joining the last glyph batch when they follow
    /// its commands under the same binding and nothing places them.
    fn push_glyphs(
        &mut self,
        renderer: &mut GpuRenderer,
        binding: SegmentBinding,
        cmds: std::ops::Range<usize>,
        base: Option<u32>,
    ) {
        let continues = base.is_none() && self.overlay_segment == Some(binding.uniform_slot);
        if let Some(Batch::Glyphs { cmds: last, .. }) = self.batches.last_mut()
            && continues
            && last.end == cmds.start
        {
            last.end = cmds.end;
            return;
        }
        let uniform_slot = self.placed_slot(renderer, binding, binding.bound, base);
        self.overlay_segment = Some(binding.uniform_slot);
        self.batches.push(Batch::Glyphs {
            cmds,
            uniform_slot,
            scissor: binding.scissor,
        });
    }

    /// Pushes image commands drawn under `viewport`, joining the last image
    /// batch when they are src-over and follow its commands under the same
    /// binding and viewport and nothing places them.
    fn push_images(
        &mut self,
        renderer: &mut GpuRenderer,
        (binding, viewport): (SegmentBinding, ViewportUniformParams),
        cmds: std::ops::Range<usize>,
        (blend_mode, base): (BlendMode, Option<u32>),
    ) {
        if cmds.is_empty() {
            return;
        }
        let continues = base.is_none()
            && blend_mode == BlendMode::SrcOver
            && self.overlay_images == Some(viewport);
        if let Some(Batch::Images {
            cmds: last,
            blend_mode: BlendMode::SrcOver,
            ..
        }) = self.batches.last_mut()
            && continues
            && last.end == cmds.start
        {
            last.end = cmds.end;
            return;
        }
        let uniform_slot = self.placed_slot(renderer, binding, viewport, base);
        self.overlay_images = Some(viewport);
        self.batches.push(Batch::Images {
            cmds,
            blend_mode,
            uniform_slot,
            scissor: binding.scissor,
        });
    }

    /// The target pixels a shape run can touch, a pixel wider on each side
    /// for its antialiased edge.
    fn run_target_bounds(&self, bounds: Rect, run: &SegmentRun<'s, '_>) -> Option<TargetRect> {
        let pixel = 1.0 / run.segment.scale;
        scissor_rect_for_rect(
            Rect {
                x: bounds.x - pixel,
                y: bounds.y - pixel,
                width: bounds.width + 2.0 * pixel,
                height: bounds.height + 2.0 * pixel,
            },
            run.segment.scale,
            run.viewport,
        )
    }

    /// How an arena run's clip applies: the scissor its paint takes, how its
    /// pipeline tests the clip, and the scissor its chunk takes. A rect clip
    /// becomes the paint's scissor, and the shader tests what a scissor
    /// cannot. A member's rect clip goes to the shader instead, so the clips
    /// of members leave their records one batch, and a member's run that
    /// may reach past its slot takes a chunk scissored to the slot.
    fn arena_clip(&self, draw: &RunDraw, bounds: Rect, run: &SegmentRun<'s, '_>) -> ArenaClip {
        let clip = crate::render::clip_scissor(&draw.placement, run.segment.scale, run.viewport);
        let Some(slot) = run.slot else {
            return ArenaClip {
                paint: clip,
                tested: crate::render::SegmentClip::of(&draw.placement, clip.is_some()),
                chunk: run.binding.scissor,
            };
        };
        let paint = clip.filter(|_| draw.placement.clip_rounded());
        let reach = self
            .run_target_bounds(bounds, run)
            .and_then(|reach| match clip {
                Some(clip) => crate::render::intersect_target_rects(reach, clip),
                None => Some(reach),
            });
        ArenaClip {
            paint,
            tested: crate::render::SegmentClip::of(&draw.placement, paint.is_some()),
            chunk: reach
                .is_some_and(|reach| !crate::render::target_rect_within(reach, slot))
                .then_some(slot),
        }
    }

    fn run_items(
        &mut self,
        renderer: &mut GpuRenderer,
        items: &mut Peekable<impl Iterator<Item = Item<'s>>>,
        run: &SegmentRun<'s, '_>,
    ) {
        while let Some(Item::Run(draw, window, bounds)) =
            items.next_if(|item| matches!(item, Item::Run(..)))
        {
            if !self.held.is_empty()
                && self
                    .run_target_bounds(bounds, run)
                    .is_some_and(|bounds| self.held.overlaps(bounds))
            {
                self.draw_held(renderer, run.binding);
            }
            let window = window.unwrap_or(0..u32::MAX);
            if renderer.run_is_stored(draw) {
                self.close_chunk(renderer, run.binding);
                let space = DrawSpace {
                    viewport: ViewportUniformParams {
                        depth_base: self.depth_seq as f32,
                        ..run.viewport
                    },
                    scale: run.segment.scale,
                };
                let batch = renderer.prepare_store_run(
                    self.recorder,
                    draw,
                    space,
                    &window,
                    (self.depth, self.depth_keys.as_mut()),
                    (&mut self.arena_draws, &mut self.pipelines),
                );
                renderer.mesh_store_run(
                    draw,
                    space,
                    batch.draws.clone(),
                    &self.arena_draws,
                    self.depth,
                    &mut self.meshes,
                );
                if self.depth.is_on() {
                    self.take_depth_range(batch.draws.clone());
                }
                self.batches.push(Batch::StoreRun {
                    batch,
                    scissor: run.segment.scissor,
                });
            } else {
                let total = draw.record_count().min(window.end);
                let mut from = window.start;
                let clip = self.arena_clip(draw, bounds, run);
                if self.chunk.is_some() && clip.chunk != self.chunk_scissor {
                    self.close_chunk(renderer, run.binding);
                }
                self.chunk_scissor = clip.chunk;
                if let Some(open) = self.chunk
                    && clip.paint != self.chunk_clip
                {
                    self.cut_paint(renderer, run.binding, open);
                }
                self.chunk_clip = clip.paint;
                let segment_clip = clip.tested;
                let at = ArenaPlacement {
                    space: run.space(),
                    shift: run.shift,
                    mixed_turns: self.mixed_turns,
                    depth: self.depth,
                };
                while from < total {
                    if self
                        .chunk
                        .is_some_and(|open| !renderer.arena_accepts(open, draw))
                    {
                        self.close_chunk(renderer, run.binding);
                    }
                    let open = match self.chunk {
                        Some(open) => open,
                        None => self.open_chunk(renderer),
                    };
                    let taken = renderer.append_arena_run(
                        open,
                        draw,
                        from..total,
                        at,
                        segment_clip,
                        (&mut self.pipelines, self.depth_keys.as_mut()),
                    );
                    if taken == 0 {
                        self.close_chunk(renderer, run.binding);
                        continue;
                    }
                    from += taken;
                }
            }
        }
    }

    /// Places the images starting a run of items: a src-over image is held
    /// beside the held glyphs, an image of another blend mode draws what is
    /// held and starts a batch of the images after it that share its mode.
    fn image_items(
        &mut self,
        renderer: &mut GpuRenderer,
        items: &mut Peekable<impl Iterator<Item = Item<'s>>>,
        run: &SegmentRun<'s, '_>,
        scratch: &mut PassScratch,
    ) -> Result<(), String> {
        let Some(Item::Image(index)) = items.peek() else {
            unreachable!("an image run starts at an image");
        };
        let image = &run.segment.scene.images[*index];
        if supported_blend_mode(image.blend_mode) != BlendMode::SrcOver {
            self.flush(renderer, run.binding);
            return self.image_run(renderer, items, run, scratch);
        }
        items.next();
        let viewport = run.image_viewport();
        if self.held.full()
            || self
                .held
                .image_viewport()
                .is_some_and(|held| held != viewport)
        {
            self.draw_held(renderer, run.binding);
        }
        let mark = scratch.image_mark();
        let mut parts = renderer.draw_parts();
        let images = (parts.images, &mut parts.upload);
        scratch.images.push_image_draw(image, run.space(), images)?;
        scratch.place_images(mark, run);
        if let Some(bounds) = scratch
            .images
            .cmds
            .get(mark.cmds)
            .map(crate::render::ImageDrawCmd::bounds)
        {
            self.held
                .hold_image(mark.cmds..scratch.images.cmds.len(), viewport, bounds);
        }
        Ok(())
    }

    fn image_run(
        &mut self,
        renderer: &mut GpuRenderer,
        items: &mut Peekable<impl Iterator<Item = Item<'s>>>,
        run: &SegmentRun<'s, '_>,
        scratch: &mut PassScratch,
    ) -> Result<(), String> {
        let Some(Item::Image(first)) = items.peek() else {
            unreachable!("image run starts at an image");
        };
        let images = &run.segment.scene.images;
        let blend_mode = supported_blend_mode(images[*first].blend_mode);
        let mark = scratch.image_mark();
        while let Some(Item::Image(index)) = items.next_if(|item| {
            matches!(item, Item::Image(index) if supported_blend_mode(images[*index].blend_mode) == blend_mode)
        }) {
            let image = &images[index];
            let mut parts = renderer.draw_parts();
            let images = (parts.images, &mut parts.upload);
            scratch.images.push_image_draw(image, run.space(), images)?;
        }
        scratch.place_images(mark, run);
        self.push_images(
            renderer,
            (run.binding, run.image_viewport()),
            mark.cmds..scratch.images.cmds.len(),
            (blend_mode, None),
        );
        Ok(())
    }

    /// Draws a run of round rect shadows as one instanced batch, placed in
    /// the pass's order as an image batch is.
    fn shadow_run(
        &mut self,
        renderer: &mut GpuRenderer,
        items: &mut Peekable<impl Iterator<Item = Item<'s>>>,
        run: &SegmentRun<'s, '_>,
        scratch: &mut PassScratch,
    ) {
        let start = scratch.shadow_instances.len();
        while let Some(Item::RRectShadow(draw)) =
            items.next_if(|item| matches!(item, Item::RRectShadow(_)))
        {
            append_shadow_instances(draw, run.segment.scale, &mut scratch.shadow_instances);
        }
        let end = scratch.shadow_instances.len();
        if start == end {
            return;
        }
        let uniform_slot = self.placed_slot(renderer, run.binding, run.viewport, None);
        let scissor = run.segment.scissor;
        if renderer.pipelines.shadows_stand_in(self.depth) {
            let parts = scratch.shadow_instances[start..end].iter();
            let indices = scratch.instant.lay_shadows(parts.map(ShadowInstance::part));
            let draw = InstantDraw::shadows(uniform_slot, indices, scissor);
            return self.batches.push(Batch::Instant(draw));
        }
        self.batches.push(Batch::Shadows {
            instances: start as u32..end as u32,
            uniform_slot,
            scissor,
        });
    }

    /// Draws one text as glyphs when its glyphs are in the atlas, held to
    /// join the previous glyph batch, else as image quads joining the
    /// previous src-over image batch.
    fn text_item(
        &mut self,
        renderer: &mut GpuRenderer,
        (text, geometry): (&'s TextDraw, (Rect, f32)),
        run: &SegmentRun<'s, '_>,
        scratch: &mut PassScratch,
    ) -> Result<(), String> {
        let glyph_start = scratch.glyph_cmds.len();
        let instance_start = scratch.glyph_instances.plain.len();
        let viewport = self.text_viewport(renderer, run);
        let mut parts = renderer.draw_parts();
        let drew_glyphs = parts.text.append_text_glyph_draws(
            (text, geometry),
            viewport,
            run.segment.scale,
            &mut scratch.glyph_instances,
            &mut scratch.glyph_cmds,
            &mut parts.upload,
        );
        scratch.place_glyphs((instance_start, glyph_start), run);
        if drew_glyphs {
            let glyph_end = scratch.glyph_cmds.len();
            if glyph_start < glyph_end {
                let cmds = &scratch.glyph_cmds[glyph_start..glyph_end];
                if self.held.full() || cmds.iter().any(|cmd| self.held.covers_image(cmd.bounds())) {
                    self.draw_held(renderer, run.binding);
                }
                self.held.hold(
                    glyph_start..glyph_end,
                    cmds.iter().map(crate::render::GlyphDrawCmd::bounds),
                );
            }
            return Ok(());
        }
        self.flush(renderer, run.binding);
        let mark = scratch.image_mark();
        let mut parts = renderer.draw_parts();
        let images = (&mut scratch.images, parts.images);
        parts
            .text
            .append_text_image_draw_cmds(text, run.space(), images, &mut parts.upload)?;
        scratch.place_images(mark, run);
        self.push_images(
            renderer,
            (run.binding, run.image_viewport()),
            mark.cmds..scratch.images.cmds.len(),
            (BlendMode::SrcOver, None),
        );
        Ok(())
    }

    /// Prepares one resolved composite where it lands in the target,
    /// skipping it when its scissor falls outside.
    fn composite_item(
        &mut self,
        renderer: &mut GpuRenderer,
        composite: &'s ResolvedComposite,
        run: &SegmentRun<'s, '_>,
        scratch: &mut PassScratch,
    ) -> Result<(), String> {
        let offset = run.segment.offset;
        let own_scissor = composite
            .scissor
            .and_then(|scissor| scissor_in_target(scissor, self.target_size(), offset));
        if composite.scissor.is_some() && own_scissor.is_none() {
            return Ok(());
        }
        let Some(scissor) = intersect_scissors(own_scissor, run.segment.scissor) else {
            return Ok(());
        };
        let dest = dest_in_target(composite.dest, offset);
        let place = self.composite_place();
        match &composite.kind {
            ResolvedCompositeKind::Blit {
                alpha,
                blend_mode,
                rounded_mask,
                sample_mode,
                source_viewport,
            } => {
                let item = CompositeBatchItem {
                    source: composite.source.as_ref(),
                    alpha: *alpha,
                    scissor,
                    rounded_mask: mask_in_target(*rounded_mask, offset),
                    blend_mode: supported_blend_mode(*blend_mode),
                    dest_viewport: Some(BlitDest {
                        rect: dest,
                        target: self.target_size(),
                    }),
                    source_viewport: *source_viewport,
                    sample_mode: *sample_mode,
                    depth: place.map(crate::render::pass_order_depth),
                };
                let prepared = renderer.effect_renderer.prepare_composite_draw(
                    self.recorder,
                    &renderer.device,
                    self.load_op,
                    &item,
                );
                let effects = &renderer.effect_renderer;
                let stand_in = effects.composite_stand_in(&renderer.device, &prepared);
                let layer = (Batch::Composite(prepared), stand_in);
                self.push_layer((renderer, run), layer, (scissor, place), || {
                    let texels = LayerTexels::new(&composite.source, *source_viewport);
                    scratch
                        .instant
                        .lay_composite(composite.dest, texels, *alpha)
                });
            }
            ResolvedCompositeKind::Shader { .. } => {
                self.shader_item(renderer, composite, (dest, scissor, offset))?;
            }
            ResolvedCompositeKind::Projective {
                dest,
                inverse,
                alpha,
                blend_mode,
                sample_mode,
                source_region,
            } => {
                let item = ProjectiveCompositeItem {
                    source: composite.source.as_ref(),
                    source_region: *source_region,
                    viewport: self.target_size(),
                    dest_strip: dest.strip().map(|[x, y]| [x - offset[0], y - offset[1]]),
                    dest_corners: dest.corners().len() as u32,
                    inverse: translate_inverse(*inverse, offset),
                    alpha: *alpha,
                    blend_mode: supported_blend_mode(*blend_mode),
                    sample_mode: *sample_mode,
                    scissor,
                    depth: place.map(crate::render::pass_order_depth),
                };
                let prepared = renderer.effect_renderer.prepare_projective_composite_draw(
                    self.recorder,
                    &renderer.device,
                    &item,
                );
                let effects = &renderer.effect_renderer;
                let stand_in = effects.projective_stand_in(&renderer.device, &prepared);
                let layer = (Batch::Projective(prepared), stand_in);
                self.push_layer((renderer, run), layer, (scissor, place), || {
                    let strip = &dest.strip()[..dest.corners().len()];
                    let texels = LayerTexels::new(&composite.source, *source_region);
                    scratch
                        .instant
                        .lay_projective(strip, (*inverse, texels), *alpha)
                });
            }
        }
        Ok(())
    }

    // Prepares `composite`, drawn through its runtime shader, over `dest`
    // within `scissor`, both in the target, which starts `offset` into the
    // segment's device space.
    fn shader_item(
        &mut self,
        renderer: &mut GpuRenderer,
        composite: &'s ResolvedComposite,
        (dest, scissor, offset): ((f32, f32, f32, f32), Option<TargetRect>, [f32; 2]),
    ) -> Result<(), String> {
        let ResolvedCompositeKind::Shader {
            shader,
            layer_pixel_rect,
            source_region,
            source_logical_size,
            substrate_regions,
            rounded_mask,
            alpha,
        } = &composite.kind
        else {
            return Ok(());
        };
        let Some(placement) = QuadPlacement::in_target(dest, self.target_size()) else {
            return Ok(());
        };
        let item = ShaderCompositeBatchItem {
            source: composite.source.as_ref(),
            shader: shader.as_ref(),
            layer_pixel_rect: *layer_pixel_rect,
            source_region: *source_region,
            source_logical_size: *source_logical_size,
            substrate_regions: *substrate_regions,
            rounded_mask: mask_in_target(*rounded_mask, offset),
            alpha: *alpha,
            scissor,
            dest_viewport: dest,
            placement,
        };
        let prepared = renderer
            .effect_renderer
            .prepare_shader_draw(self.recorder, &renderer.device, &item)
            .ok_or_else(|| "shader composite preparation failed".to_string())?;
        self.batches.push(Batch::Shader(prepared));
        Ok(())
    }
}

/// One segment's viewport, turn included, and what its shapes and glyphs
/// bind while its items are batched.
struct SegmentRun<'s, 'a> {
    segment: &'a PassSegment<'s>,
    viewport: ViewportUniformParams,
    binding: SegmentBinding,
    /// What moves the segment's device space into the binding's: whole
    /// pixels for a member of side-by-side segments, else zero.
    shift: [f32; 2],
    /// A member's scissor, which its binding leaves to each draw that may
    /// reach past it.
    slot: Option<TargetRect>,
}

/// How an arena run's clip applies: see [`PassPrep::arena_clip`].
struct ArenaClip {
    paint: Option<TargetRect>,
    tested: crate::render::SegmentClip,
    chunk: Option<TargetRect>,
}

impl SegmentRun<'_, '_> {
    fn space(&self) -> DrawSpace {
        DrawSpace {
            viewport: self.viewport,
            scale: self.segment.scale,
        }
    }

    /// The viewport the segment's images draw under: a member's are moved
    /// into its binding's space.
    fn image_viewport(&self) -> ViewportUniformParams {
        if self.slot.is_some() {
            self.binding.bound
        } else {
            self.viewport
        }
    }
}

/// What a segment's shape and glyph batches bind: its viewport without its
/// turn, which its records and glyphs carry, the slot claimed for that and
/// its scissor. Members side by side bind the target's own space without a
/// scissor: their records and glyphs carry their offsets.
#[derive(Clone, Copy)]
struct SegmentBinding {
    uniform_slot: usize,
    bound: ViewportUniformParams,
    scissor: Option<(u32, u32, u32, u32)>,
}

#[cfg(test)]
#[path = "tests/draw_pass_tests.rs"]
mod tests;
