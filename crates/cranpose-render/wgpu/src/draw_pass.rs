use std::{iter::Peekable, rc::Rc, sync::Arc};

use cranpose_ui_graphics::{BlendMode, Rect, RuntimeShader};

use crate::{
    effect_renderer::{
        CompositeBatchItem, CompositeSampleMode, PreparedCompositeDraw,
        PreparedProjectiveComposite, PreparedShaderDraw, ProjectiveCompositeItem,
        RoundedCompositeMask, ShaderCompositeBatchItem, SubstrateRegions,
    },
    frame_graph::FrameCommandRecorder,
    geometry::SegmentTransform,
    offscreen::OffscreenTarget,
    render::{
        GpuRenderer, PassFrame, RunStage, StoreRunBatch, TargetRect, ViewportUniformParams,
        image_draw_bounds, run_draw_bounds, run_draw_is_visible_in_rect, run_record_bounds,
        run_record_holes, scissor_rect_for_rect, segment_scene_rect, supported_blend_mode,
        text_draw_bounds, text_draw_is_visible_in_rect,
    },
    rrect_shadow::{ShadowInstance, append_shadow_instances, rrect_shadow_bounds},
    run_store::{RunDrawCall, run_has_shapes},
    scene::{CompositorScene, DrawOp, DrawOpKind, RRectShadowDraw, RunDraw, TextDraw},
};

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
        dest_quad: [[f32; 2]; 4],
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

enum Item<'a> {
    Run(&'a RunDraw, Option<std::ops::Range<u32>>),
    Image(usize),
    Text(&'a TextDraw),
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
}

pub(crate) fn scissor_in_target(
    scissor: (f32, f32, f32, f32),
    target_size: (u32, u32),
    segment_offset: [f32; 2],
) -> Option<(u32, u32, u32, u32)> {
    let (x, y, width, height) = scissor;
    let left = (x - segment_offset[0]).floor().max(0.0);
    let top = (y - segment_offset[1]).floor().max(0.0);
    let right = (x + width - segment_offset[0])
        .ceil()
        .min(target_size.0 as f32);
    let bottom = (y + height - segment_offset[1])
        .ceil()
        .min(target_size.1 as f32);
    if right <= left || bottom <= top {
        return None;
    }
    Some((
        left as u32,
        top as u32,
        (right - left) as u32,
        (bottom - top) as u32,
    ))
}

fn intersect_scissors(
    a: Option<(u32, u32, u32, u32)>,
    b: Option<(u32, u32, u32, u32)>,
) -> Option<Option<(u32, u32, u32, u32)>> {
    match (a, b) {
        (None, None) => Some(None),
        (Some(rect), None) | (None, Some(rect)) => Some(Some(rect)),
        (Some((ax, ay, aw, ah)), Some((bx, by, bw, bh))) => {
            let left = ax.max(bx);
            let top = ay.max(by);
            let right = (ax + aw).min(bx + bw);
            let bottom = (ay + ah).min(by + bh);
            (right > left && bottom > top).then(|| Some((left, top, right - left, bottom - top)))
        }
    }
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

fn dest_in_target(dest: (f32, f32, f32, f32), segment_offset: [f32; 2]) -> (f32, f32, f32, f32) {
    (
        dest.0 - segment_offset[0],
        dest.1 - segment_offset[1],
        dest.2,
        dest.3,
    )
}

fn mask_in_target(
    mask: Option<RoundedCompositeMask>,
    segment_offset: [f32; 2],
) -> Option<RoundedCompositeMask> {
    mask.map(|mask| RoundedCompositeMask {
        rect: [
            mask.rect[0] - segment_offset[0],
            mask.rect[1] - segment_offset[1],
            mask.rect[2],
            mask.rect[3],
        ],
        radii: mask.radii,
    })
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
        PassBuffers {
            images: (!scratch.image_indices.is_empty()).then(|| {
                self.upload_image_slot(
                    recorder,
                    &scratch.image_vertices,
                    &scratch.image_indices,
                    &scratch.image_clips,
                )
            }),
            shadows: (!scratch.shadow_instances.is_empty())
                .then(|| self.upload_shadow_instances(recorder, &scratch.shadow_instances)),
            glyphs: (!scratch.glyph_instances.plain.is_empty())
                .then(|| self.upload_glyph_instances(recorder, &scratch.glyph_instances.plain)),
            turned_glyphs: (!scratch.glyph_instances.turned.is_empty())
                .then(|| self.upload_turned_glyphs(recorder, &scratch.glyph_instances.turned)),
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
        segments: &'s [PassSegment<'s>],
        load_op: wgpu::LoadOp<wgpu::Color>,
        label: &'static str,
    ) -> Result<bool, String> {
        let mut scratch = self.take_pass_scratch();
        let device = self.device.clone();
        let depth = takes_depth(segments);
        let OverlayScratch {
            mut held,
            mut hoisted,
            candidates,
        } = std::mem::take(&mut scratch.overlays);
        held.cells.reset(target.width);
        hoisted.cover.reset(target.width);
        hoisted.marks = segments
            .iter()
            .any(|segment| !segment.scene.rrect_shadows.is_empty());
        let mut prep = PassPrep {
            recorder,
            device: &device,
            target,
            load_op,
            batches: Vec::new(),
            chunk: None,
            held,
            hoisted,
            candidates,
            depth,
            mixed_turns: turns_mixed(segments),
            overlay_segment: None,
            overlay_images: None,
            depth_seq: 0,
            held_depth_end: 0,
            open: None,
            arena_draws: std::mem::take(&mut scratch.arena_draws),
            chunk_start: 0,
            chunk_base: 0,
            chunk_slot: None,
            chunk_clip: None,
        };
        let prepared = segments
            .iter()
            .try_for_each(|segment| prep.segment(self, segment, &mut scratch));
        prep.finish(self, &mut scratch);
        let batches = prep.batches;
        scratch.arena_draws = prep.arena_draws;
        scratch.overlays = OverlayScratch {
            held: prep.held,
            hoisted: prep.hoisted,
            candidates: prep.candidates,
        };
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
                        &mut pass,
                        frame,
                        &batches,
                        PassCmds {
                            images: &scratch.image_cmds,
                            glyphs: &scratch.glyph_cmds,
                            arena: &scratch.arena_draws,
                        },
                        &buffers,
                    )
                };
                recorder.record_pass();
                draw_result.map(|()| true)
            }
        };
        drop(batches);
        self.return_pass_scratch(scratch);
        result
    }

    fn take_pass_scratch(&mut self) -> PassScratch {
        let mut scratch = PassScratch {
            image_vertices: std::mem::take(&mut self.scratch_image_vertices),
            image_indices: std::mem::take(&mut self.scratch_image_indices),
            image_cmds: std::mem::take(&mut self.scratch_image_cmds),
            image_clips: std::mem::take(&mut self.scratch_image_clips),
            glyph_instances: std::mem::take(&mut self.scratch_glyph_instances),
            glyph_cmds: std::mem::take(&mut self.scratch_glyph_cmds),
            glyph_moved: std::mem::take(&mut self.scratch_glyph_moved),
            shadow_instances: std::mem::take(&mut self.scratch_shadow_instances),
            arena_draws: std::mem::take(&mut self.scratch_arena_draws),
            overlays: std::mem::take(&mut self.scratch_overlays),
        };
        scratch.arena_draws.clear();
        scratch.image_vertices.clear();
        scratch.image_indices.clear();
        scratch.image_cmds.clear();
        scratch.image_clips.clear();
        scratch.glyph_instances.clear();
        scratch.glyph_cmds.clear();
        scratch.shadow_instances.clear();
        scratch
    }

    fn return_pass_scratch(&mut self, scratch: PassScratch) {
        self.scratch_image_vertices = scratch.image_vertices;
        self.scratch_image_indices = scratch.image_indices;
        self.scratch_image_cmds = scratch.image_cmds;
        self.scratch_image_clips = scratch.image_clips;
        self.scratch_glyph_instances = scratch.glyph_instances;
        self.scratch_glyph_cmds = scratch.glyph_cmds;
        self.scratch_glyph_moved = scratch.glyph_moved;
        self.scratch_shadow_instances = scratch.shadow_instances;
        self.scratch_arena_draws = scratch.arena_draws;
        self.scratch_overlays = scratch.overlays;
    }

    fn draw_batches(
        &mut self,
        pass: &mut wgpu::RenderPass<'_>,
        frame: PassFrame,
        batches: &[Batch<'_>],
        cmds: PassCmds<'_>,
        buffers: &PassBuffers,
    ) -> Result<(), String> {
        let target_size = frame.size;
        if frame.depth {
            for batch in batches.iter().rev() {
                self.draw_shape_batch(pass, batch, cmds.arena, frame, RunStage::Interiors)?;
            }
        }
        for batch in batches {
            match batch {
                Batch::StoreRun { .. } | Batch::Arena { .. } => {
                    self.draw_shape_batch(pass, batch, cmds.arena, frame, RunStage::Paint)?;
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
                    self.draw_glyph_cmds(
                        pass,
                        (buffers.glyphs.as_ref(), buffers.turned_glyphs.as_ref()),
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
                Batch::Composite(prepared) => {
                    self.effect_renderer
                        .draw_prepared_composite(pass, target_size, prepared);
                }
                Batch::Shader(prepared) => {
                    self.effect_renderer
                        .draw_prepared_shader_src_over(pass, target_size, prepared);
                }
                Batch::Projective(prepared) => {
                    self.effect_renderer.draw_prepared_projective_composite(
                        pass,
                        target_size,
                        prepared,
                    );
                }
            }
        }
        Ok(())
    }
}

impl GpuRenderer {
    /// Records one shape batch's stage; other batches draw nothing here.
    /// An arena batch's draws are ranges of `arena`.
    fn draw_shape_batch(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        batch: &Batch<'_>,
        arena: &[RunDrawCall],
        frame: PassFrame,
        stage: RunStage,
    ) -> Result<(), String> {
        match batch {
            Batch::StoreRun { batch, scissor } => {
                match stage_scissor(frame.scissor(*scissor), batch.clip, stage) {
                    Some(scissor) => self.draw_store_run(pass, batch, scissor, stage),
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
                self.draw_arena(pass, *chunk, *uniform_slot, &arena[draws], scissor, stage)
            }
            _ => Ok(()),
        }
    }
}

impl GpuRenderer {
    /// Begins the pass `encode_pass` records into, with a depth buffer when
    /// its opaque interiors go down first.
    fn begin_scene_pass<'p, C: FrameCommandRecorder>(
        &mut self,
        recorder: &'p mut C,
        label: &'static str,
        target: PassTarget<'_>,
        load_op: wgpu::LoadOp<wgpu::Color>,
        depth: bool,
    ) -> wgpu::RenderPass<'p> {
        if depth {
            self.frame_stats
                .depth_passes
                .set(self.frame_stats.depth_passes.get() + 1);
            let depth_view = self.depth_target((target.width, target.height));
            recorder.begin_depth_pass(label, target.view, load_op, &depth_view)
        } else {
            recorder.begin_color_pass(label, target.view, load_op)
        }
    }
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

static NO_INTERIORS_FIRST: crate::debug_toggles::DebugToggle =
    crate::debug_toggles::DebugToggle::new("CRANPOSE_NO_INTERIORS_FIRST");

/// Whether a pass of `segments` lays its opaque interiors down in a depth
/// pre-pass: one with an opaque fill worth laying down, unless it
/// composites, whose composite pipelines also draw into passes without a
/// depth buffer.
fn takes_depth(segments: &[PassSegment<'_>]) -> bool {
    !NO_INTERIORS_FIRST.equals("1")
        && segments.iter().all(|segment| segment.composites.is_empty())
        && segments.iter().any(segment_has_occluders)
}

/// Whether a run `segment` draws holds an opaque fill with an interior worth
/// laying down ahead of the paint.
fn segment_has_occluders(segment: &PassSegment<'_>) -> bool {
    segment
        .ops
        .iter()
        .enumerate()
        .any(|(op_index, op)| match op.kind {
            DrawOpKind::Run(index) => {
                let window = (op_index == 0)
                    .then(|| segment.first_run_window.clone())
                    .flatten()
                    .unwrap_or(0..u32::MAX);
                let run = &segment.scene.runs[index];
                let shapes = &run.tables().shapes;
                let mut first = 0;
                run.segment_records().any(|records| {
                    let start = first;
                    first += records.count;
                    let mut drawn = start.max(window.start)..first.min(window.end);
                    if !records.occluders || drawn.is_empty() {
                        return false;
                    }
                    drawn == (start..first)
                        || drawn.any(|ordinal| {
                            shapes.occludes((records.start + ordinal - start) as usize)
                        })
                })
            }
            _ => false,
        })
}

/// The logical rect `op` may draw into: a shape, image or text by its
/// snapped bounds within its clip, an unblurred shadow by the union of its
/// parts. A blurred shadow draws nothing itself (it resolves to a
/// composite).
pub(crate) fn op_draw_bounds(
    scene: &CompositorScene,
    op: &DrawOp,
    root_scale: f32,
) -> Option<Rect> {
    match op.kind {
        DrawOpKind::Run(index) => run_draw_bounds(&scene.runs[index], root_scale),
        DrawOpKind::Image(index) => image_draw_bounds(&scene.images[index], root_scale),
        DrawOpKind::Text(index) => text_draw_bounds(&scene.texts[index], root_scale),
        DrawOpKind::RRectShadow(index) => {
            rrect_shadow_bounds(&scene.rrect_shadows[index], root_scale)
        }
        DrawOpKind::Shadow(index) => {
            let shadow = &scene.shadow_draws[index];
            if shadow.requires_surface() {
                return None;
            }
            shadow_caster_bounds(shadow, root_scale)
                .into_iter()
                .chain(
                    shadow
                        .texts
                        .iter()
                        .filter_map(|text| text_draw_bounds(text, root_scale)),
                )
                .reduce(|a, b| a.union(b))
        }
    }
}

/// The snapped bounds of an unblurred shadow's casters, within the
/// shadow's clip.
fn shadow_caster_bounds(shadow: &crate::scene::ShadowDraw, root_scale: f32) -> Option<Rect> {
    shadow
        .shapes
        .as_ref()
        .and_then(|run| run_draw_bounds(run, root_scale))
}

/// Whether `op` draws any pixel inside `viewport_rect`.
pub(crate) fn op_is_visible_in_rect(
    scene: &CompositorScene,
    op: &DrawOp,
    viewport_rect: Rect,
    root_scale: f32,
) -> bool {
    op_draw_bounds(scene, op, root_scale)
        .is_some_and(|bounds| bounds.intersect(viewport_rect).is_some())
}

/// The logical rect a segment's draws are judged against: its scissor
/// within the target, or the whole target, at the segment's offset, mapped
/// back through the segment's transform.
fn segment_viewport_rect(target: PassTarget<'_>, segment: &PassSegment<'_>) -> Rect {
    let (x, y, width, height) = segment
        .scissor
        .unwrap_or((0, 0, target.width, target.height));
    segment_scene_rect(
        segment.transform,
        Rect {
            x: segment.offset[0] + x as f32,
            y: segment.offset[1] + y as f32,
            width: width as f32,
            height: height as f32,
        },
        segment.scale,
    )
}

/// Whether drawing `segment` into `target` touches any pixel: some op or
/// composite of it reaches into its scissor, by the same test the pass
/// applies when it draws.
pub(crate) fn segment_draws_anything(target: PassTarget<'_>, segment: &PassSegment<'_>) -> bool {
    let viewport_rect = segment_viewport_rect(target, segment);
    merge_items(segment, viewport_rect, (target.width, target.height), false)
        .next()
        .is_some()
}

/// Re-bases an inverse (target pixel -> source pixel) matrix onto a target
/// whose origin is `offset` pixels into the space the matrix was built for.
fn translate_inverse(inverse: [[f32; 3]; 3], offset: [f32; 2]) -> [[f32; 3]; 3] {
    let mut shifted = inverse;
    for row in &mut shifted {
        row[2] += row[0] * offset[0] + row[1] * offset[1];
    }
    shifted
}

fn unshadowed_item<'a>(
    scene: &'a CompositorScene,
    kind: DrawOpKind,
    op_index: usize,
    first_run_window: &Option<std::ops::Range<u32>>,
    skip_text: bool,
) -> Option<Item<'a>> {
    match kind {
        DrawOpKind::Run(index) => {
            let run = &scene.runs[index];
            run_has_shapes(run).then(|| {
                let window = (op_index == 0).then(|| first_run_window.clone()).flatten();
                Item::Run(run, window)
            })
        }
        DrawOpKind::Image(index) => Some(Item::Image(index)),
        DrawOpKind::Text(_) if skip_text => None,
        DrawOpKind::Text(index) => Some(Item::Text(&scene.texts[index])),
        DrawOpKind::RRectShadow(index) => Some(Item::RRectShadow(&scene.rrect_shadows[index])),
        DrawOpKind::Shadow(_) => None,
    }
}

fn merge_items<'a>(
    segment: &PassSegment<'a>,
    viewport_rect: Rect,
    target_size: (u32, u32),
    skip_text: bool,
) -> impl Iterator<Item = Item<'a>> + use<'a> {
    let scene = segment.scene;
    let root_scale = segment.scale;
    let mut ops = segment.ops.iter().enumerate().peekable();
    let mut composites = segment.composites.iter().peekable();
    let mut shadow_texts: std::slice::Iter<'a, TextDraw> = [].iter();
    let offset = segment.offset;
    let scissor = segment.scissor;
    let first_run_window = segment.first_run_window.clone();
    std::iter::from_fn(move || {
        loop {
            if let Some(text) = shadow_texts
                .find(|text| text_draw_is_visible_in_rect(text, viewport_rect, root_scale))
            {
                return Some(Item::Text(text));
            }
            let next_z = ops.peek().map(|(_, op)| op.z_index);
            if composites
                .peek()
                .is_some_and(|composite| next_z.is_none_or(|z| composite.z_index <= z))
            {
                let composite = composites.next().expect("peeked composite");
                if composite_visible(composite, target_size, offset, scissor) {
                    return Some(Item::Composite(composite));
                }
                continue;
            }
            let (op_index, op) = ops.next()?;
            if !op_is_visible_in_rect(scene, op, viewport_rect, root_scale) {
                continue;
            }
            if let DrawOpKind::Shadow(index) = op.kind {
                let shadow = &scene.shadow_draws[index];
                shadow_texts = shadow.texts.iter();
                if let Some(run) = unblurred_shadow_run(shadow, viewport_rect, root_scale) {
                    return Some(Item::Run(run, None));
                }
                continue;
            }
            if let Some(item) =
                unshadowed_item(scene, op.kind, op_index, &first_run_window, skip_text)
            {
                return Some(item);
            }
        }
    })
}

/// An unblurred shadow's casters as a run, when any of them reaches the
/// viewport.
fn unblurred_shadow_run(
    shadow: &crate::scene::ShadowDraw,
    viewport_rect: Rect,
    root_scale: f32,
) -> Option<&RunDraw> {
    let run = shadow.shapes.as_ref()?;
    run_draw_is_visible_in_rect(run, viewport_rect, root_scale).then_some(run)
}

/// The target pixels a shape's logical `rect` can touch, a pixel wider on
/// each side for its antialiased edge.
fn shape_target_rect(rect: Rect, run: &SegmentRun<'_, '_>) -> Option<TargetRect> {
    let pixel = 1.0 / run.segment.scale;
    scissor_rect_for_rect(
        Rect {
            x: rect.x - pixel,
            y: rect.y - pixel,
            width: rect.width + 2.0 * pixel,
            height: rect.height + 2.0 * pixel,
        },
        run.segment.scale,
        run.viewport,
    )
}

/// The pass-order indices `draws`' records span, instanced from their
/// table's start.
fn records_spanned(draws: &[RunDrawCall]) -> u32 {
    draws.iter().map(|draw| draw.records.end).max().unwrap_or(0)
}

/// A held rect gathered for a run, in the scene's logical space, where the
/// run's records are checked against it: its edges, left, top, right and
/// bottom, a sixteenth of a pixel wider each way than its target pixels,
/// for the rounding of the integer rects a draw takes.
#[derive(Clone, Copy)]
struct Candidate {
    edges: [f32; 4],
    kind: Overlay,
}

impl Candidate {
    fn of(held: TargetRect, kind: Overlay, scale: f32, offset: [f32; 2]) -> Self {
        let (x, y, width, height) = held;
        let sliver = 1.0 / 16.0;
        let logical = |pixel: u32, origin: f32, grow: f32| (pixel as f32 + origin + grow) / scale;
        Self {
            edges: [
                logical(x, offset[0], -sliver),
                logical(y, offset[1], -sliver),
                logical(x.saturating_add(width), offset[0], sliver),
                logical(y.saturating_add(height), offset[1], sliver),
            ],
            kind,
        }
    }

    /// Whether a record reaching `rect`, already a target pixel wider each
    /// way for its antialiased edge, overlaps the held draw's pixels, as
    /// the integer rect it would draw in does.
    fn reached_by(&self, rect: [f32; 4]) -> bool {
        let [left, top, right, bottom] = self.edges;
        left < rect[2] && rect[0] < right && top < rect[3] && rect[1] < bottom
    }

    /// Whether every pixel of the held draw lies within one of a stroked
    /// rect's `holes`, each a target pixel narrower each way for the
    /// antialiased edge around it, as the integer rects inside them do.
    fn kept_by(&self, holes: [Rect; 2], pixel: f32) -> bool {
        let [left, top, right, bottom] = self.edges;
        holes.iter().any(|hole| {
            hole.x + pixel <= left
                && hole.y + pixel <= top
                && right <= hole.x + hole.width - pixel
                && bottom <= hole.y + hole.height - pixel
        })
    }
}

/// The kinds of `candidates` a record of `draw` in `window` shares a pixel
/// with, placing no more records once every kind of `gathered` is found.
fn records_meeting(
    (draw, window): (&RunDraw, &std::ops::Range<u32>),
    scale: f32,
    candidates: &[Candidate],
    gathered: Kinds,
) -> Kinds {
    let pixel = 1.0 / scale;
    let mut found = Kinds::default();
    for (rect, index) in run_record_bounds(draw, scale, window.clone()) {
        let reach = [
            rect.x - pixel,
            rect.y - pixel,
            rect.x + rect.width + pixel,
            rect.y + rect.height + pixel,
        ];
        for candidate in candidates {
            if !found.has(candidate.kind)
                && candidate.reached_by(reach)
                && !run_record_holes(draw, scale, index)
                    .is_some_and(|holes| candidate.kept_by(holes, pixel))
            {
                found = found | Kinds::of(candidate.kind);
            }
        }
        if found == gathered {
            break;
        }
    }
    found
}

/// The target pixels a shape run can touch.
fn run_target_bounds(draw: &RunDraw, run: &SegmentRun<'_, '_>) -> Option<TargetRect> {
    run_draw_bounds(draw, run.segment.scale).and_then(|bounds| shape_target_rect(bounds, run))
}

/// The target pixels a round rect shadow can touch.
fn shadow_target_rect(draw: &RRectShadowDraw, run: &SegmentRun<'_, '_>) -> Option<TargetRect> {
    rrect_shadow_bounds(draw, run.segment.scale)
        .and_then(|bounds| scissor_rect_for_rect(bounds, run.segment.scale, run.viewport))
}

/// The per-frame vectors a pass fills: image and glyph geometry and draw
/// commands, kept on the renderer between frames so they never reallocate.
/// The quads a pass drew from its scratch, uploaded for its draws: image
/// vertices and indices, and glyph instances.
#[derive(Default)]
struct PassBuffers {
    images: Option<crate::render::ImageSlot>,
    shadows: Option<crate::frame_graph::BufferUpload>,
    glyphs: Option<crate::frame_graph::BufferUpload>,
    turned_glyphs: Option<crate::frame_graph::BufferUpload>,
}

struct PassScratch {
    image_vertices: Vec<crate::render::Vertex>,
    image_indices: Vec<u32>,
    image_cmds: Vec<crate::render::ImageDrawCmd>,
    image_clips: Vec<crate::render::DeviceRoundedClip>,
    glyph_instances: crate::render::GlyphInstances,
    glyph_cmds: Vec<crate::render::GlyphDrawCmd>,
    /// Where a glyph batch's draws of one kind wait while it groups them.
    glyph_moved: Vec<crate::render::GlyphDrawCmd>,
    shadow_instances: Vec<ShadowInstance>,
    arena_draws: Vec<RunDrawCall>,
    overlays: OverlayScratch,
}

/// The commands a pass's batches draw ranges of.
#[derive(Clone, Copy)]
struct PassCmds<'a> {
    images: &'a [crate::render::ImageDrawCmd],
    glyphs: &'a [crate::render::GlyphDrawCmd],
    arena: &'a [RunDrawCall],
}

/// Draws each glyph batch's labels one kind, turned or upright, at a time
/// where their order allows: see [`crate::render::group_glyph_kinds`].
fn group_glyph_batches(batches: &[Batch<'_>], scratch: &mut PassScratch) {
    for batch in batches {
        if let Batch::Glyphs { cmds, .. } = batch {
            crate::render::group_glyph_kinds(
                &mut scratch.glyph_cmds[cmds.clone()],
                &mut scratch.glyph_moved,
            );
        }
    }
}

/// Most draws held back at once; past this they draw, so a long stretch of
/// shapes checks each against a bounded list.
const MAX_HELD: usize = 256;

pub(crate) fn target_rects_overlap(a: TargetRect, b: TargetRect) -> bool {
    a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
}

fn target_rect_union(a: TargetRect, b: TargetRect) -> TargetRect {
    let left = a.0.min(b.0);
    let top = a.1.min(b.1);
    let right = (a.0 + a.2).max(b.0 + b.2);
    let bottom = (a.1 + a.3).max(b.1 + b.3);
    (left, top, right - left, bottom - top)
}

/// Log2 of the side, in target pixels, of the cells [`HeldOverlays`] marks.
const HELD_CELL_SHIFT: u32 = 6;

/// Log2 of the side, in target pixels, of the cells [`HoistedShadows`]
/// marks: fine enough that a card's shadow clears the cells of the card
/// beside it.
const COVER_CELL_SHIFT: u32 = 2;

/// The square cells of `1 << SHIFT` target pixels that marked rects touch,
/// each row of cells a run of bit words.
#[derive(Default)]
struct CellGrid<const SHIFT: u32> {
    words: usize,
    cells: Vec<u64>,
    /// The rows holding a mark, which clearing zeroes.
    marked: std::ops::Range<usize>,
}

impl<const SHIFT: u32> CellGrid<SHIFT> {
    /// Forgets every mark and fits the rows to a target `width` pixels wide.
    fn reset(&mut self, width: u32) {
        self.words = (width >> SHIFT) as usize / 64 + 1;
        self.cells.clear();
        self.marked = 0..0;
    }

    fn clear(&mut self) {
        let words = self.words.max(1);
        if let Some(cells) = self
            .cells
            .get_mut(self.marked.start * words..self.marked.end * words)
        {
            cells.fill(0);
        }
        self.marked = 0..0;
    }

    fn mark(&mut self, rect: TargetRect) {
        let (rows, columns) = self.span(rect);
        let words = self.words.max(1);
        if self.cells.len() < rows.end * words {
            self.cells.resize(rows.end * words, 0);
        }
        self.marked = if self.marked.is_empty() {
            rows.clone()
        } else {
            self.marked.start.min(rows.start)..self.marked.end.max(rows.end)
        };
        for row in rows {
            for (word, mask) in column_words(columns.clone()) {
                self.cells[row * words + word] |= mask;
            }
        }
    }

    /// Whether `rect` touches a marked cell.
    fn touched(&self, rect: TargetRect) -> bool {
        let (rows, columns) = self.span(rect);
        let words = self.words.max(1);
        let rows = rows.start..rows.end.min(self.cells.len() / words);
        rows.into_iter().any(|row| {
            column_words(columns.clone())
                .any(|(word, mask)| self.cells[row * words + word] & mask != 0)
        })
    }

    /// The rows of cells `rect` touches and its columns, those past the
    /// target sharing its last. An empty side counts as one pixel:
    /// [`target_rects_overlap`] lets a rect that thin overlap one it lies
    /// inside, and the pixel it stands on is in that one too.
    fn span(&self, rect: TargetRect) -> (std::ops::Range<usize>, std::ops::RangeInclusive<usize>) {
        let (x, y, width, height) = rect;
        let last = self.words.max(1) * 64 - 1;
        let column = |pixel: u32| ((pixel >> SHIFT) as usize).min(last);
        let row = |pixel: u32| (pixel >> SHIFT) as usize;
        (
            row(y)..row(y.saturating_add(height.max(1) - 1)) + 1,
            column(x)..=column(x.saturating_add(width.max(1) - 1)),
        )
    }
}

/// The bit words a row's `columns` fall in, each with its bits of them.
fn column_words(columns: std::ops::RangeInclusive<usize>) -> impl Iterator<Item = (usize, u64)> {
    let (first, last) = (*columns.start(), *columns.end());
    (first / 64..=last / 64).map(move |word| {
        let low = if word == first / 64 { first % 64 } else { 0 };
        let high = if word == last / 64 { last % 64 } else { 63 };
        (word, (u64::MAX >> (63 - high)) & (u64::MAX << low))
    })
}

/// What a held draw is. Held draws of one kind keep their order when they
/// draw, so they may overlap each other; draws of different kinds may not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Overlay {
    Glyphs,
    Images,
    StoreRun,
}

const OVERLAYS: [Overlay; 3] = [Overlay::Glyphs, Overlay::Images, Overlay::StoreRun];

/// A set of [`Overlay`] kinds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Kinds(u8);

impl Kinds {
    const ALL: Self = Self(0b111);

    fn of(kind: Overlay) -> Self {
        Self(1 << kind as u8)
    }

    fn has(self, kind: Overlay) -> bool {
        self.0 & Self::of(kind).0 != 0
    }

    fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for Kinds {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitAnd for Kinds {
    type Output = Self;

    fn bitand(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}

/// Widens by `rect`, which a held draw of `kind` touches, the unions of
/// [`HeldOverlays::unions`] that draws of other kinds and shapes check.
fn widen(unions: &mut [Option<TargetRect>; 4], rect: TargetRect, kind: Overlay) {
    for (slot, union) in unions.iter_mut().enumerate() {
        if slot != kind as usize {
            *union = Some(union.map_or(rect, |union| target_rect_union(union, rect)));
        }
    }
}

/// A target rect a held draw touches.
struct HeldRect {
    rect: TargetRect,
    kind: Overlay,
}

/// A prepared stored run held back, with the scissor of its segment.
struct HeldStoreRun {
    batch: StoreRunBatch,
    scissor: Option<TargetRect>,
}

/// Draws held back so the shapes after them keep filling one arena chunk:
/// glyphs, src-over images and stored runs. The texts, avatar and chart of
/// a card no longer split the shapes around them into draws of their own.
/// A shape that overlaps a held draw, or a draw of another kind that does,
/// draws the held draws of that kind first, so nothing is reordered past a
/// pixel it shares; held draws of different kinds never share one.
#[derive(Default)]
struct HeldOverlays {
    glyphs: Option<std::ops::Range<usize>>,
    /// The held image commands and the viewport they draw under.
    images: Option<(std::ops::Range<usize>, ViewportUniformParams)>,
    store_runs: Vec<HeldStoreRun>,
    rects: Vec<HeldRect>,
    /// The union of the held rects a draw of each kind may not pass, by
    /// [`Overlay`], and last for a shape: those of the other kinds, and for
    /// a shape every one.
    unions: [Option<TargetRect>; 4],
    /// The cells a held rect touches. A rect touching none of them overlaps
    /// no held draw, so only one that does is checked against each: a
    /// list's cards held a few hundred draws that every shape after them
    /// was checked against.
    cells: CellGrid<HELD_CELL_SHIFT>,
    /// The held rects a run's bounds overlap, which its records are checked
    /// against.
    near: Vec<usize>,
}

impl HeldOverlays {
    /// The kinds of the draws held.
    fn kinds(&self) -> Kinds {
        let held = [
            self.glyphs.is_some(),
            self.images.is_some(),
            !self.store_runs.is_empty(),
        ];
        OVERLAYS
            .iter()
            .zip(held)
            .filter(|(_, held)| *held)
            .fold(Kinds::default(), |kinds, (kind, _)| {
                kinds | Kinds::of(*kind)
            })
    }

    fn full(&self) -> bool {
        self.rects.len() >= MAX_HELD
    }

    fn add(&mut self, rect: TargetRect, kind: Overlay) {
        widen(&mut self.unions, rect, kind);
        self.cells.mark(rect);
        self.rects.push(HeldRect { rect, kind });
    }

    /// Holds the glyph commands at `cmds`, which touch `bounds`.
    fn hold_glyphs(
        &mut self,
        cmds: std::ops::Range<usize>,
        bounds: impl IntoIterator<Item = TargetRect>,
    ) {
        self.glyphs = Some(match self.glyphs.take() {
            Some(held) => held.start..cmds.end,
            None => cmds,
        });
        for rect in bounds {
            self.add(rect, Overlay::Glyphs);
        }
    }

    /// Holds the image command at `cmds`, drawn under `viewport` within
    /// `bounds`.
    fn hold_image(
        &mut self,
        cmds: std::ops::Range<usize>,
        viewport: ViewportUniformParams,
        bounds: TargetRect,
    ) {
        let cmds = match self.images.take() {
            Some((held, _)) => held.start..cmds.end,
            None => cmds,
        };
        self.images = Some((cmds, viewport));
        self.add(bounds, Overlay::Images);
    }

    /// Holds a stored run within `bounds`; one touching nothing of the
    /// target is held without them.
    fn hold_store_run(&mut self, run: HeldStoreRun, bounds: Option<TargetRect>) {
        self.store_runs.push(run);
        if let Some(bounds) = bounds {
            self.add(bounds, Overlay::StoreRun);
        }
    }

    /// The viewport the held images draw under.
    fn image_viewport(&self) -> Option<ViewportUniformParams> {
        self.images.as_ref().map(|(_, viewport)| *viewport)
    }

    /// Whether a draw of `kind`, `None` for a shape, touching `rect` may
    /// overlap a held draw it may not pass: it lies within their union and
    /// touches a cell a held rect touches.
    fn may_touch(&self, rect: TargetRect, kind: Option<Overlay>) -> bool {
        let slot = kind.map_or(OVERLAYS.len(), |kind| kind as usize);
        self.unions[slot].is_some_and(|union| target_rects_overlap(union, rect))
            && self.cells.touched(rect)
    }

    /// The kinds of the held draws a draw of `kind`, `None` for a shape,
    /// touching `rect` has to wait for: those of other kinds it overlaps.
    fn blocking(&self, rect: TargetRect, kind: Option<Overlay>) -> Kinds {
        if !self.may_touch(rect, kind) {
            return Kinds::default();
        }
        self.rects
            .iter()
            .filter(|held| Some(held.kind) != kind && target_rects_overlap(held.rect, rect))
            .fold(Kinds::default(), |kinds, held| kinds | Kinds::of(held.kind))
    }

    /// Gathers the held rects a run of `kind` touching `bounds` may have to
    /// wait for, and returns whether there are any.
    fn gather(&mut self, bounds: TargetRect, kind: Option<Overlay>) -> bool {
        self.near.clear();
        if !self.may_touch(bounds, kind) {
            return false;
        }
        let Self { rects, near, .. } = self;
        near.extend(
            rects
                .iter()
                .enumerate()
                .filter(|(_, held)| {
                    Some(held.kind) != kind && target_rects_overlap(held.rect, bounds)
                })
                .map(|(index, _)| index),
        );
        !near.is_empty()
    }

    /// The kinds of the gathered rects.
    fn gathered_kinds(&self) -> Kinds {
        self.near.iter().fold(Kinds::default(), |kinds, index| {
            kinds | Kinds::of(self.rects[*index].kind)
        })
    }

    /// The gathered rects as candidates in the logical space of a run drawn
    /// at `scale` from `offset`.
    fn candidates(&self, scale: f32, offset: [f32; 2]) -> impl Iterator<Item = Candidate> + '_ {
        self.near.iter().map(move |index| {
            let held = &self.rects[*index];
            Candidate::of(held.rect, held.kind, scale, offset)
        })
    }

    /// Forgets the rects of the held draws of `kinds` once they have drawn;
    /// the cells stay marked until nothing is held.
    fn release(&mut self, kinds: Kinds) {
        self.rects.retain(|held| !kinds.has(held.kind));
        self.unions = [None; 4];
        for held in &self.rects {
            widen(&mut self.unions, held.rect, held.kind);
        }
        if self.rects.is_empty() {
            self.cells.clear();
        }
    }
}

/// The open batch of round rect shadows: a shadow that nothing drawn or
/// held since the batch's place overlaps joins it there, so the shadows of
/// cards set apart draw as one batch below the cards. One that overlaps
/// such a draw draws everything before it and opens the next batch.
#[derive(Default)]
struct HoistedShadows {
    /// Whether the pass draws round rect shadows at all; without any,
    /// nothing marks the cells.
    marks: bool,
    instances: Vec<ShadowInstance>,
    /// The cells of everything drawn or held since the batch's place.
    cover: CellGrid<COVER_CELL_SHIFT>,
    /// The batch's place among the pass's batches and the pass-order index
    /// there.
    batch_start: usize,
    depth_start: u32,
}

impl HoistedShadows {
    fn begin(&mut self, batch_start: usize, depth_start: u32) {
        self.cover.clear();
        self.batch_start = batch_start;
        self.depth_start = depth_start;
    }

    fn mark(&mut self, rect: Option<TargetRect>) {
        if let Some(rect) = rect.filter(|_| self.marks) {
            self.cover.mark(rect);
        }
    }
}

/// The per-pass state of held and hoisted draws, kept on the renderer
/// between frames so it never reallocates.
#[derive(Default)]
pub(crate) struct OverlayScratch {
    held: HeldOverlays,
    hoisted: HoistedShadows,
    /// The held rects gathered for one run, in its logical space.
    candidates: Vec<Candidate>,
}

/// Turns the segments of one pass into batches, one item run at a time.
struct PassPrep<'a, 's, C> {
    recorder: &'a mut C,
    device: &'a wgpu::Device,
    target: PassTarget<'a>,
    load_op: wgpu::LoadOp<wgpu::Color>,
    batches: Vec<Batch<'s>>,
    /// The arena chunk shapes are being appended to, kept open across held
    /// draws.
    chunk: Option<usize>,
    held: HeldOverlays,
    hoisted: HoistedShadows,
    /// The held rects gathered for the run being placed, in its logical
    /// space.
    candidates: Vec<Candidate>,
    /// Whether the pass has a depth buffer its opaque interiors fill first.
    depth: bool,
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
    /// The pass-order index past the records of the stored runs held so
    /// far, where the pass's order goes on once they draw.
    held_depth_end: u32,
    /// What the open chunk and held draws draw with. Consecutive segments
    /// that bind the same keep adding to them: layers drawn in place carry
    /// their turns in their records and glyphs, not in what they bind.
    open: Option<SegmentBinding>,
    /// Every arena draw of the pass, which arena batches draw ranges of.
    arena_draws: Vec<RunDrawCall>,
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
}

impl<'s, C: FrameCommandRecorder> PassPrep<'_, 's, C> {
    fn target_size(&self) -> (u32, u32) {
        (self.target.width, self.target.height)
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
        let viewport = ViewportUniformParams {
            width: self.target.width,
            height: self.target.height,
            offset: segment.offset,
            transform: segment.transform,
            origin: [0.0; 2],
            depth_base: 0.0,
        };
        let viewport_rect = segment_viewport_rect(self.target, segment);
        let bound = ViewportUniformParams {
            transform: SegmentTransform::IDENTITY,
            ..viewport
        };
        let binding = match self.open {
            Some(open) if open.bound == bound && open.scissor == segment.scissor => open,
            _ => self.bind(renderer, bound, segment.scissor, scratch),
        };
        let mut items = merge_items(
            segment,
            viewport_rect,
            self.target_size(),
            renderer.ablation.text,
        )
        .peekable();
        let run = SegmentRun {
            segment,
            viewport,
            binding,
        };
        while let Some(item) = items.peek() {
            match item {
                Item::Run(..) => {
                    self.run_items(renderer, &mut items, &run);
                    continue;
                }
                Item::Image(_) => {
                    self.image_items(renderer, &mut items, &run, scratch)?;
                    continue;
                }
                Item::Text(text) => self.text_item(renderer, text, &run, scratch)?,
                Item::RRectShadow(_) => {
                    self.shadow_items(renderer, &mut items, &run, scratch);
                    continue;
                }
                Item::Composite(composite) => self.composite_items(renderer, composite, &run)?,
            }
            items.next();
        }
        Ok(())
    }

    /// Draws what the open binding still holds and opens one binding
    /// `bound` under `scissor`.
    fn bind(
        &mut self,
        renderer: &mut GpuRenderer,
        bound: ViewportUniformParams,
        scissor: Option<TargetRect>,
        scratch: &mut PassScratch,
    ) -> SegmentBinding {
        self.finish(renderer, scratch);
        let open = SegmentBinding {
            uniform_slot: renderer.claim_uniform_slot(bound),
            bound,
            scissor,
        };
        self.open = Some(open);
        self.hoisted.begin(self.batches.len(), self.depth_seq);
        open
    }

    /// Draws what the open binding still holds and places its open shadow
    /// batch; the next segment binds afresh.
    fn finish(&mut self, renderer: &mut GpuRenderer, scratch: &mut PassScratch) {
        if let Some(open) = self.open.take() {
            self.flush(renderer, open);
            self.place_hoisted(renderer, open, scratch);
        }
    }

    /// Puts the open shadow batch at its place as one instanced batch,
    /// placed in a pass with a depth buffer at the pass-order index there,
    /// so every later opaque interior hides it.
    fn place_hoisted(
        &mut self,
        renderer: &mut GpuRenderer,
        binding: SegmentBinding,
        scratch: &mut PassScratch,
    ) {
        if self.hoisted.instances.is_empty() {
            return;
        }
        let start = scratch.shadow_instances.len() as u32;
        scratch.shadow_instances.append(&mut self.hoisted.instances);
        let uniform_slot = self.placed_slot(
            renderer,
            binding,
            binding.bound,
            Some(self.hoisted.depth_start),
        );
        self.batches.insert(
            self.hoisted.batch_start,
            Batch::Shadows {
                instances: start..scratch.shadow_instances.len() as u32,
                uniform_slot,
                scissor: binding.scissor,
            },
        );
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
        let slot = if self.depth {
            renderer.claim_uniform_slot(ViewportUniformParams {
                depth_base: self.chunk_base as f32,
                ..binding.bound
            })
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
        if end == self.chunk_start {
            return;
        }
        let uniform_slot = self.chunk_uniform_slot(renderer, binding);
        if self.depth {
            self.depth_seq = self.chunk_base.saturating_add(records);
        }
        self.batches.push(Batch::Arena {
            chunk: open,
            uniform_slot,
            paint: paint_start..end,
            interiors: Some(self.chunk_start..end),
            scissor: binding.scissor,
            clip: self.chunk_clip,
        });
    }

    /// Draws the held draws of `kinds` where the open chunk has got to, and
    /// keeps the chunk open: its shapes so far paint first and those draws
    /// above them, while its later records, and the opaque interiors of all
    /// of them, stay one chunk. Without an open chunk it draws them after
    /// what is drawn. The held draws of other kinds stay held: they share
    /// no pixel with these.
    fn draw_held(&mut self, renderer: &mut GpuRenderer, binding: SegmentBinding, kinds: Kinds) {
        let kinds = kinds & self.held.kinds();
        if kinds.is_empty() {
            return;
        }
        let Some(open) = self.chunk else {
            self.flush_held(renderer, binding, kinds);
            return;
        };
        if self.depth && kinds.has(Overlay::StoreRun) {
            // The held runs' records took pass-order indices past the
            // chunk's so far, which its later records would share.
            self.flush_held(renderer, binding, kinds);
            return;
        }
        self.cut_paint(renderer, binding, open);
        // The held draws sit where the chunk's next record will: its later
        // opaque interiors hide them, its earlier ones do not.
        let base = self
            .chunk_base
            .saturating_add(renderer.open_arena_records());
        self.emit_held(renderer, binding, Some(base), kinds);
    }

    /// Paints the open chunk's draws since its last cut, under the clip
    /// their records share, and keeps the chunk open.
    fn cut_paint(&mut self, renderer: &mut GpuRenderer, binding: SegmentBinding, open: usize) {
        let paint_start = self.arena_draws.len();
        renderer.cut_arena(open, &mut self.arena_draws);
        let end = self.arena_draws.len();
        if end > paint_start {
            let uniform_slot = self.chunk_uniform_slot(renderer, binding);
            self.batches.push(Batch::Arena {
                chunk: open,
                uniform_slot,
                paint: paint_start..end,
                interiors: None,
                scissor: binding.scissor,
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
        if self.depth {
            let base = base.unwrap_or_else(|| {
                let next = self.depth_seq;
                self.depth_seq = next.saturating_add(1);
                next
            });
            renderer.claim_uniform_slot(ViewportUniformParams {
                depth_base: base as f32,
                ..viewport
            })
        } else if viewport == binding.bound {
            binding.uniform_slot
        } else {
            renderer.claim_uniform_slot(viewport)
        }
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
        if !self.depth {
            return run.viewport;
        }
        let open = self.chunk.map_or(0, |_| renderer.open_arena_records());
        ViewportUniformParams {
            depth_base: self.depth_seq.saturating_add(open) as f32,
            ..run.viewport
        }
    }

    /// Draws everything held: the open chunk's shapes, then the held draws
    /// above them.
    fn flush(&mut self, renderer: &mut GpuRenderer, binding: SegmentBinding) {
        self.flush_held(renderer, binding, Kinds::ALL);
    }

    /// Closes the open chunk and draws the held draws of `kinds` above it.
    fn flush_held(&mut self, renderer: &mut GpuRenderer, binding: SegmentBinding, kinds: Kinds) {
        self.close_chunk(renderer, binding);
        if kinds.has(Overlay::StoreRun) {
            self.depth_seq = self.depth_seq.max(self.held_depth_end);
        }
        self.emit_held(renderer, binding, None, kinds);
    }

    /// Pushes the held draws of `kinds`, one batch of each kind, a stored
    /// run's draws on their own, placed in a pass with a depth buffer at
    /// `base` or next in its order. Held draws of different kinds never
    /// overlap, so the kinds draw in any order.
    fn emit_held(
        &mut self,
        renderer: &mut GpuRenderer,
        binding: SegmentBinding,
        base: Option<u32>,
        kinds: Kinds,
    ) {
        let kinds = kinds & self.held.kinds();
        if kinds.is_empty() {
            return;
        }
        if let Some(cmds) = self.held.glyphs.take_if(|_| kinds.has(Overlay::Glyphs)) {
            self.emit_glyphs(renderer, binding, cmds, base);
        }
        if let Some((cmds, viewport)) = self.held.images.take_if(|_| kinds.has(Overlay::Images)) {
            self.emit_images(renderer, binding, (cmds, viewport), base);
        }
        if kinds.has(Overlay::StoreRun) {
            self.batches
                .extend(self.held.store_runs.drain(..).map(|held| Batch::StoreRun {
                    batch: held.batch,
                    scissor: held.scissor,
                }));
        }
        self.held.release(kinds);
    }

    /// Pushes held glyph commands, joining the last glyph batch when they
    /// follow its commands under the same binding and nothing places them.
    fn emit_glyphs(
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

    /// Pushes held src-over image commands drawn under a viewport, joining
    /// the last image batch as [`Self::emit_glyphs`] joins glyphs.
    fn emit_images(
        &mut self,
        renderer: &mut GpuRenderer,
        binding: SegmentBinding,
        (cmds, viewport): (std::ops::Range<usize>, ViewportUniformParams),
        base: Option<u32>,
    ) {
        let continues = base.is_none() && self.overlay_images == Some(viewport);
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
            blend_mode: BlendMode::SrcOver,
            uniform_slot,
            scissor: binding.scissor,
        });
    }

    fn run_items(
        &mut self,
        renderer: &mut GpuRenderer,
        items: &mut Peekable<impl Iterator<Item = Item<'s>>>,
        run: &SegmentRun<'s, '_>,
    ) {
        while let Some(Item::Run(draw, window)) =
            items.next_if(|item| matches!(item, Item::Run(..)))
        {
            let window = window.unwrap_or(0..u32::MAX);
            let bounds = run_target_bounds(draw, run);
            self.hoisted.mark(bounds);
            if renderer.run_is_stored(draw) {
                self.hold_stored_run(renderer, (draw, window), run, bounds);
                continue;
            }
            if let Some(bounds) = bounds {
                let waits = self.run_waits((draw, &window), run, bounds, None);
                self.draw_held(renderer, run.binding, waits);
            }
            self.append_run(renderer, draw, window, run);
        }
    }

    /// The kinds of the held draws a record of `draw` in `window`, a run
    /// of `kind` within `bounds`, has to wait for. Its records are placed
    /// one by one only when its bounds overlap such a draw, and only
    /// without a turn, under which its bounds stand for them.
    fn run_waits(
        &mut self,
        (draw, window): (&RunDraw, &std::ops::Range<u32>),
        run: &SegmentRun<'s, '_>,
        bounds: TargetRect,
        kind: Option<Overlay>,
    ) -> Kinds {
        if !self.held.gather(bounds, kind) {
            return Kinds::default();
        }
        let gathered = self.held.gathered_kinds();
        if !run.viewport.transform.is_identity() {
            return gathered;
        }
        self.candidates.clear();
        self.candidates
            .extend(self.held.candidates(run.segment.scale, run.viewport.offset));
        records_meeting(
            (draw, window),
            run.segment.scale,
            &self.candidates,
            gathered,
        )
    }

    /// Holds the draws of a run kept in the run store within `bounds`, after
    /// drawing what is held when one of its records overlaps a held draw of
    /// another kind or the hold is full.
    fn hold_stored_run(
        &mut self,
        renderer: &mut GpuRenderer,
        (draw, window): (&RunDraw, std::ops::Range<u32>),
        run: &SegmentRun<'s, '_>,
        bounds: Option<TargetRect>,
    ) {
        let waits = match bounds {
            _ if self.held.full() => Kinds::ALL,
            Some(bounds) => self.run_waits((draw, &window), run, bounds, Some(Overlay::StoreRun)),
            None => Kinds::default(),
        };
        self.draw_held(renderer, run.binding, waits);
        let base = self.held_run_base(renderer);
        let batch = renderer.prepare_store_run(
            self.recorder,
            draw,
            ViewportUniformParams {
                depth_base: base as f32,
                ..run.viewport
            },
            run.segment.scale,
            &window,
            self.depth,
        );
        if self.depth {
            self.held_depth_end = base.saturating_add(records_spanned(&batch.draws));
        }
        self.held.hold_store_run(
            HeldStoreRun {
                batch,
                scissor: run.segment.scissor,
            },
            bounds,
        );
    }

    /// Where a held stored run's records start in the pass's order: after
    /// the open chunk's records so far and the runs held before it, which
    /// may share its pixels. The chunk's later records may take the same
    /// indices while it is held, since they share none of its pixels; once
    /// it draws, the pass's order goes on past it.
    fn held_run_base(&self, renderer: &GpuRenderer) -> u32 {
        if !self.depth {
            return 0;
        }
        let next = match self.chunk {
            Some(_) => self
                .chunk_base
                .saturating_add(renderer.open_arena_records()),
            None => self.depth_seq,
        };
        next.max(self.held_depth_end)
    }

    /// Appends `draw`'s records in `window` to the open arena chunk,
    /// opening a chunk when none is, cutting its paint where the run's clip
    /// differs and closing it when full.
    fn append_run(
        &mut self,
        renderer: &mut GpuRenderer,
        draw: &RunDraw,
        window: std::ops::Range<u32>,
        run: &SegmentRun<'s, '_>,
    ) {
        let total = draw.record_count().min(window.end);
        let mut from = window.start;
        let clip = crate::render::clip_scissor(&draw.placement, run.segment.scale, run.viewport);
        if let Some(open) = self.chunk
            && clip != self.chunk_clip
        {
            self.cut_paint(renderer, run.binding, open);
        }
        self.chunk_clip = clip;
        let segment_clip = crate::render::SegmentClip::of(&draw.placement, clip.is_some());
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
                run.segment.scale,
                run.viewport,
                self.mixed_turns,
                (self.depth, segment_clip),
            );
            if taken == 0 {
                self.close_chunk(renderer, run.binding);
                continue;
            }
            from += taken;
        }
    }

    /// Places the images starting a run of items: a src-over image is held
    /// with the overlays, an image of another blend mode draws what is held
    /// and starts a batch of the images after it that share its mode.
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
        self.hold_image(renderer, image, run, scratch)
    }

    /// Holds a src-over image's draw, drawing what is held first when the
    /// image overlaps a held draw of another kind, the held images draw
    /// under another viewport or the hold is full.
    fn hold_image(
        &mut self,
        renderer: &mut GpuRenderer,
        image: &crate::scene::ImageDraw,
        run: &SegmentRun<'s, '_>,
        scratch: &mut PassScratch,
    ) -> Result<(), String> {
        if self
            .held
            .image_viewport()
            .is_some_and(|viewport| viewport != run.viewport)
        {
            self.draw_held(renderer, run.binding, Kinds::of(Overlay::Images));
        }
        let start = scratch.image_cmds.len();
        renderer.append_image_draw_cmd(
            image,
            run.viewport,
            run.segment.scale,
            (&mut scratch.image_vertices, &mut scratch.image_indices),
            &mut scratch.image_clips,
            &mut scratch.image_cmds,
        )?;
        let Some(bounds) = scratch
            .image_cmds
            .get(start)
            .map(crate::render::ImageDrawCmd::bounds)
        else {
            return Ok(());
        };
        let waits = if self.held.full() {
            Kinds::ALL
        } else {
            self.held.blocking(bounds, Some(Overlay::Images))
        };
        self.draw_held(renderer, run.binding, waits);
        self.held
            .hold_image(start..scratch.image_cmds.len(), run.viewport, bounds);
        self.hoisted.mark(Some(bounds));
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
        let cmd_start = scratch.image_cmds.len();
        while let Some(Item::Image(index)) = items.next_if(|item| {
            matches!(item, Item::Image(index) if supported_blend_mode(images[*index].blend_mode) == blend_mode)
        }) {
            let image = &images[index];
            renderer.append_image_draw_cmd(
                image,
                run.viewport,
                run.segment.scale,
                (&mut scratch.image_vertices, &mut scratch.image_indices),
                &mut scratch.image_clips,
                &mut scratch.image_cmds,
            )?;
        }
        self.mark_images(&scratch.image_cmds[cmd_start..]);
        if cmd_start < scratch.image_cmds.len() {
            let uniform_slot = self.placed_slot(renderer, run.binding, run.viewport, None);
            self.overlay_images = Some(run.viewport);
            self.batches.push(Batch::Images {
                cmds: cmd_start..scratch.image_cmds.len(),
                blend_mode,
                uniform_slot,
                scissor: run.segment.scissor,
            });
        }
        Ok(())
    }

    /// Marks what image commands drawn in place cover, for the shadows
    /// hoisted past them.
    fn mark_images(&mut self, cmds: &[crate::render::ImageDrawCmd]) {
        for cmd in cmds {
            self.hoisted.mark(Some(cmd.bounds()));
        }
    }

    /// Places the round rect shadow starting a run of items. Drawn without
    /// a turn it joins the open shadow batch, after drawing everything so
    /// far and opening the next batch when it overlaps a draw since the
    /// batch's place. Under a turn, which a batch binds, it draws in place
    /// with the shadows after it.
    fn shadow_items(
        &mut self,
        renderer: &mut GpuRenderer,
        items: &mut Peekable<impl Iterator<Item = Item<'s>>>,
        run: &SegmentRun<'s, '_>,
        scratch: &mut PassScratch,
    ) {
        let Some(Item::RRectShadow(draw)) = items.peek() else {
            unreachable!("a shadow run starts at a shadow");
        };
        let draw = *draw;
        if run.viewport != run.binding.bound {
            self.flush(renderer, run.binding);
            self.shadow_run(renderer, items, run, scratch);
            return;
        }
        if shadow_target_rect(draw, run).is_some_and(|bounds| self.hoisted.cover.touched(bounds)) {
            self.flush(renderer, run.binding);
            self.place_hoisted(renderer, run.binding, scratch);
            self.hoisted.begin(self.batches.len(), self.depth_seq);
        }
        items.next();
        append_shadow_instances(draw, run.segment.scale, &mut self.hoisted.instances);
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
            self.hoisted.mark(shadow_target_rect(draw, run));
        }
        let end = scratch.shadow_instances.len();
        if start == end {
            return;
        }
        let uniform_slot = self.placed_slot(renderer, run.binding, run.viewport, None);
        self.batches.push(Batch::Shadows {
            instances: start as u32..end as u32,
            uniform_slot,
            scissor: run.segment.scissor,
        });
    }

    /// Draws one text as glyphs when its glyphs are in the atlas, held with
    /// the overlays, else as image quads joining the previous src-over
    /// image batch.
    fn text_item(
        &mut self,
        renderer: &mut GpuRenderer,
        text: &'s TextDraw,
        run: &SegmentRun<'s, '_>,
        scratch: &mut PassScratch,
    ) -> Result<(), String> {
        let glyph_start = scratch.glyph_cmds.len();
        let drew_glyphs = renderer.append_text_glyph_draws(
            text,
            self.text_viewport(renderer, run),
            run.segment.scale,
            &mut scratch.glyph_instances,
            &mut scratch.glyph_cmds,
        );
        if drew_glyphs {
            let cmds = &scratch.glyph_cmds[glyph_start..];
            if !cmds.is_empty() {
                let waits = if self.held.full() {
                    Kinds::ALL
                } else {
                    cmds.iter().fold(Kinds::default(), |kinds, cmd| {
                        kinds | self.held.blocking(cmd.bounds(), Some(Overlay::Glyphs))
                    })
                };
                self.draw_held(renderer, run.binding, waits);
                let bounds = cmds.iter().map(crate::render::GlyphDrawCmd::bounds);
                for rect in bounds.clone() {
                    self.hoisted.mark(Some(rect));
                }
                self.held
                    .hold_glyphs(glyph_start..scratch.glyph_cmds.len(), bounds);
            }
            return Ok(());
        }
        self.flush(renderer, run.binding);
        let cmd_start = scratch.image_cmds.len();
        renderer.append_text_image_draw_cmds(
            text,
            run.viewport,
            run.segment.scale,
            &mut scratch.image_vertices,
            &mut scratch.image_indices,
            &mut scratch.image_cmds,
        )?;
        self.mark_images(&scratch.image_cmds[cmd_start..]);
        if cmd_start < scratch.image_cmds.len() {
            let continues = self.overlay_images == Some(run.viewport);
            match self.batches.last_mut() {
                Some(Batch::Images {
                    cmds, blend_mode, ..
                }) if cmds.end == cmd_start && *blend_mode == BlendMode::SrcOver && continues => {
                    cmds.end = scratch.image_cmds.len();
                }
                _ => {
                    let uniform_slot = self.placed_slot(renderer, run.binding, run.viewport, None);
                    self.overlay_images = Some(run.viewport);
                    self.batches.push(Batch::Images {
                        cmds: cmd_start..scratch.image_cmds.len(),
                        blend_mode: BlendMode::SrcOver,
                        uniform_slot,
                        scissor: run.segment.scissor,
                    });
                }
            }
        }
        Ok(())
    }

    /// Draws what is held, then prepares one resolved composite and marks
    /// what it covers for the shadows hoisted past it.
    fn composite_items(
        &mut self,
        renderer: &mut GpuRenderer,
        composite: &'s ResolvedComposite,
        run: &SegmentRun<'s, '_>,
    ) -> Result<(), String> {
        self.flush(renderer, run.binding);
        let offset = run.segment.offset;
        let target_size = self.target_size();
        self.hoisted.mark(
            scissor_in_target(composite.dest, target_size, offset)
                .and_then(|dest| intersect_scissors(Some(dest), run.segment.scissor).flatten()),
        );
        self.composite_item(renderer, composite, run)
    }

    /// Prepares one resolved composite where it lands in the target,
    /// skipping it when its scissor falls outside.
    fn composite_item(
        &mut self,
        renderer: &mut GpuRenderer,
        composite: &'s ResolvedComposite,
        run: &SegmentRun<'s, '_>,
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
                    dest_viewport: Some(dest),
                    source_viewport: *source_viewport,
                    sample_mode: *sample_mode,
                };
                let prepared = renderer.effect_renderer.prepare_composite_draw(
                    self.recorder,
                    self.device,
                    self.load_op,
                    &item,
                );
                self.batches.push(Batch::Composite(prepared));
            }
            ResolvedCompositeKind::Shader {
                shader,
                layer_pixel_rect,
                source_region,
                source_logical_size,
                substrate_regions,
                rounded_mask,
                alpha,
            } => {
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
                };
                let prepared = renderer
                    .effect_renderer
                    .prepare_shader_draw(self.recorder, self.device, &item)
                    .ok_or_else(|| "shader composite preparation failed".to_string())?;
                self.batches.push(Batch::Shader(prepared));
            }
            ResolvedCompositeKind::Projective {
                dest_quad,
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
                    dest_quad: dest_quad.map(|[x, y]| [x - offset[0], y - offset[1]]),
                    inverse: translate_inverse(*inverse, offset),
                    alpha: *alpha,
                    blend_mode: supported_blend_mode(*blend_mode),
                    sample_mode: *sample_mode,
                    scissor,
                };
                let prepared = renderer.effect_renderer.prepare_projective_composite_draw(
                    self.recorder,
                    self.device,
                    &item,
                );
                self.batches.push(Batch::Projective(prepared));
            }
        }
        Ok(())
    }
}

/// One segment's viewport, turn included, and what its shapes and glyphs
/// bind while its items are batched.
struct SegmentRun<'s, 'a> {
    segment: &'a PassSegment<'s>,
    viewport: ViewportUniformParams,
    binding: SegmentBinding,
}

/// What a segment's shape and glyph batches bind: its viewport without its
/// turn, which its records and glyphs carry, the slot claimed for that and
/// its scissor.
#[derive(Clone, Copy)]
struct SegmentBinding {
    uniform_slot: usize,
    bound: ViewportUniformParams,
    scissor: Option<(u32, u32, u32, u32)>,
}

#[cfg(test)]
#[path = "tests/draw_pass_tests.rs"]
mod tests;
