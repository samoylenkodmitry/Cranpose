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
        image_draw_bounds, run_draw_bounds, run_draw_is_visible_in_rect, scissor_rect_for_rect,
        segment_scene_rect, supported_blend_mode, text_draw_bounds, text_draw_is_visible_in_rect,
    },
    run_store::{RunDrawCall, run_has_shapes},
    scene::{CompositorScene, DrawOp, DrawOpKind, RunDraw, TextDraw},
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
        let mut prep = PassPrep {
            recorder,
            device: &device,
            target,
            load_op,
            batches: Vec::new(),
            chunk: None,
            pending_glyphs: PendingGlyphs::default(),
            depth,
            mixed_turns: turns_mixed(segments),
            overlay_segment: None,
            overlay_images: None,
            depth_seq: 0,
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
        prep.finish(self);
        let batches = prep.batches;
        scratch.arena_draws = prep.arena_draws;
        let buffers = PassBuffers {
            images: match &prepared {
                Ok(()) if !scratch.image_indices.is_empty() => Some(self.upload_image_slot(
                    recorder,
                    &scratch.image_vertices,
                    &scratch.image_indices,
                )),
                _ => None,
            },
            glyphs: match &prepared {
                Ok(()) if !scratch.glyph_instances.plain.is_empty() => {
                    Some(self.upload_glyph_instances(recorder, &scratch.glyph_instances.plain))
                }
                _ => None,
            },
            turned_glyphs: match &prepared {
                Ok(()) if !scratch.glyph_instances.turned.is_empty() => {
                    Some(self.upload_turned_glyphs(recorder, &scratch.glyph_instances.turned))
                }
                _ => None,
            },
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
            glyph_instances: std::mem::take(&mut self.scratch_glyph_instances),
            glyph_cmds: std::mem::take(&mut self.scratch_glyph_cmds),
            glyph_moved: std::mem::take(&mut self.scratch_glyph_moved),
            arena_draws: std::mem::take(&mut self.scratch_arena_draws),
        };
        scratch.arena_draws.clear();
        scratch.image_vertices.clear();
        scratch.image_indices.clear();
        scratch.image_cmds.clear();
        scratch.glyph_instances.clear();
        scratch.glyph_cmds.clear();
        scratch
    }

    fn return_pass_scratch(&mut self, scratch: PassScratch) {
        self.scratch_image_vertices = scratch.image_vertices;
        self.scratch_image_indices = scratch.image_indices;
        self.scratch_image_cmds = scratch.image_cmds;
        self.scratch_glyph_instances = scratch.glyph_instances;
        self.scratch_glyph_cmds = scratch.glyph_cmds;
        self.scratch_glyph_moved = scratch.glyph_moved;
        self.scratch_arena_draws = scratch.arena_draws;
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
    segment.ops.iter().any(|op| match op.kind {
        DrawOpKind::Run(index) => segment.scene.runs[index]
            .segment_records()
            .any(|records| records.occluders),
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

/// The per-frame vectors a pass fills: image and glyph geometry and draw
/// commands, kept on the renderer between frames so they never reallocate.
/// The quads a pass drew from its scratch, uploaded for its draws: image
/// vertices and indices, and glyph instances.
struct PassBuffers {
    images: Option<crate::render::ImageSlot>,
    glyphs: Option<crate::frame_graph::BufferUpload>,
    turned_glyphs: Option<crate::frame_graph::BufferUpload>,
}

struct PassScratch {
    image_vertices: Vec<crate::render::Vertex>,
    image_indices: Vec<u32>,
    image_cmds: Vec<crate::render::ImageDrawCmd>,
    glyph_instances: crate::render::GlyphInstances,
    glyph_cmds: Vec<crate::render::GlyphDrawCmd>,
    /// Where a glyph batch's draws of one kind wait while it groups them.
    glyph_moved: Vec<crate::render::GlyphDrawCmd>,
    arena_draws: Vec<RunDrawCall>,
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

/// Most glyph draws held back at once; past this they draw, so a long
/// stretch of shapes checks each against a bounded list.
const MAX_PENDING_GLYPHS: usize = 256;

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

/// Log2 of the side, in target pixels, of the cells [`PendingGlyphs`] marks.
const HELD_CELL_SHIFT: u32 = 6;

/// The rows of cells `rect` touches and the mask of its columns in each,
/// columns past the 63rd sharing the last bit. An empty side counts as one
/// pixel: [`target_rects_overlap`] lets a rect that thin overlap one it lies
/// inside, and the pixel it stands on is in that one too.
fn held_cells(rect: TargetRect) -> (std::ops::Range<usize>, u64) {
    let (x, y, width, height) = rect;
    let last_column = (x.saturating_add(width.max(1) - 1) >> HELD_CELL_SHIFT).min(63);
    let first_column = (x >> HELD_CELL_SHIFT).min(63);
    let columns = last_column - first_column + 1;
    let mask = if columns >= 64 {
        u64::MAX
    } else {
        ((1_u64 << columns) - 1) << first_column
    };
    let first_row = (y >> HELD_CELL_SHIFT) as usize;
    let last_row = (y.saturating_add(height.max(1) - 1) >> HELD_CELL_SHIFT) as usize;
    (first_row..last_row + 1, mask)
}

/// Glyph draws held back so the shapes after them keep filling one arena
/// chunk: the text of a card no longer splits the backgrounds around it
/// into draws of their own. A shape that overlaps a held draw, or any draw
/// of another kind, draws them first, so nothing is reordered past a pixel
/// it shares.
#[derive(Default)]
struct PendingGlyphs {
    cmds: Option<std::ops::Range<usize>>,
    bounds: Vec<TargetRect>,
    union: Option<TargetRect>,
    /// Per band of cell rows, the cell columns a held draw touches. A shape
    /// touching none of them overlaps no held draw, so only one that does is
    /// checked against each: a list's cards held a few hundred glyph draws
    /// that every background after them was checked against.
    cells: Vec<u64>,
}

impl PendingGlyphs {
    /// Holds the glyph commands at `cmds`, which touch `bounds`.
    fn hold(&mut self, cmds: std::ops::Range<usize>, bounds: impl IntoIterator<Item = TargetRect>) {
        self.cmds = Some(match self.cmds.take() {
            Some(held) => held.start..cmds.end,
            None => cmds,
        });
        for rect in bounds {
            self.union = Some(
                self.union
                    .map_or(rect, |union| target_rect_union(union, rect)),
            );
            self.bounds.push(rect);
            let (rows, mask) = held_cells(rect);
            if self.cells.len() < rows.end {
                self.cells.resize(rows.end, 0);
            }
            for row in &mut self.cells[rows] {
                *row |= mask;
            }
        }
    }

    /// Whether a draw touching `rect` would cover a held glyph draw.
    fn overlaps(&self, rect: TargetRect) -> bool {
        self.union
            .is_some_and(|union| target_rects_overlap(union, rect))
            && self.cells_touched(rect)
            && self
                .bounds
                .iter()
                .any(|held| target_rects_overlap(*held, rect))
    }

    /// Whether `rect` touches a cell a held draw touches.
    fn cells_touched(&self, rect: TargetRect) -> bool {
        let (rows, mask) = held_cells(rect);
        let end = rows.end.min(self.cells.len());
        let start = rows.start.min(end);
        self.cells[start..end].iter().any(|row| row & mask != 0)
    }

    fn full(&self) -> bool {
        self.bounds.len() >= MAX_PENDING_GLYPHS
    }

    fn take(&mut self) -> Option<std::ops::Range<usize>> {
        self.bounds.clear();
        self.cells.clear();
        self.union = None;
        self.cmds.take()
    }
}

/// Turns the segments of one pass into batches, one item run at a time.
struct PassPrep<'a, 's, C> {
    recorder: &'a mut C,
    device: &'a wgpu::Device,
    target: PassTarget<'a>,
    load_op: wgpu::LoadOp<wgpu::Color>,
    batches: Vec<Batch<'s>>,
    /// The arena chunk shapes are being appended to, kept open across held
    /// glyph draws.
    chunk: Option<usize>,
    pending_glyphs: PendingGlyphs,
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
    /// What the open chunk and held glyphs draw with. Consecutive segments
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
            _ => {
                self.finish(renderer);
                let open = SegmentBinding {
                    uniform_slot: renderer.claim_uniform_slot(bound),
                    bound,
                    scissor: segment.scissor,
                };
                self.open = Some(open);
                open
            }
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
                    self.flush(renderer, run.binding);
                    self.image_run(renderer, &mut items, &run, scratch)?;
                    continue;
                }
                Item::Text(text) => self.text_item(renderer, text, &run, scratch)?,
                Item::Composite(composite) => {
                    self.flush(renderer, run.binding);
                    self.composite_item(renderer, composite, &run)?;
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

    /// Draws the held glyphs where the open chunk has got to, and keeps the
    /// chunk open: its shapes so far paint first and the glyphs above them,
    /// while its later records, and the opaque interiors of all of them,
    /// stay one chunk. Without an open chunk it draws what is held.
    fn draw_held_glyphs(&mut self, renderer: &mut GpuRenderer, binding: SegmentBinding) {
        let Some(open) = self.chunk else {
            self.flush(renderer, binding);
            return;
        };
        let Some(cmds) = self.pending_glyphs.take() else {
            return;
        };
        self.cut_paint(renderer, binding, open);
        // The glyphs sit where the chunk's next record will: its later
        // opaque interiors hide them, its earlier ones do not.
        let uniform_slot = if self.depth {
            renderer.claim_uniform_slot(ViewportUniformParams {
                depth_base: self
                    .chunk_base
                    .saturating_add(renderer.open_arena_records())
                    as f32,
                ..binding.bound
            })
        } else {
            binding.uniform_slot
        };
        self.overlay_segment = Some(binding.uniform_slot);
        self.batches.push(Batch::Glyphs {
            cmds,
            uniform_slot,
            scissor: binding.scissor,
        });
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

    /// The uniform slot a new glyph batch binds: the binding's, or in a pass
    /// with a depth buffer one that places the batch next in the pass's
    /// order, so later opaque interiors hide it and earlier ones do not.
    fn overlay_slot(&mut self, renderer: &mut GpuRenderer, binding: SegmentBinding) -> usize {
        self.overlay_segment = Some(binding.uniform_slot);
        self.claim_overlay(renderer, binding.bound, binding.uniform_slot)
    }

    /// The uniform slot a new image batch binds: its segment's viewport,
    /// turn included, since image quads carry none of their own.
    fn image_slot(&mut self, renderer: &mut GpuRenderer, run: &SegmentRun<'s, '_>) -> usize {
        self.overlay_images = Some(run.viewport);
        let slot = if run.viewport == run.binding.bound {
            run.binding.uniform_slot
        } else {
            renderer.claim_uniform_slot(run.viewport)
        };
        self.claim_overlay(renderer, run.viewport, slot)
    }

    /// `slot`, which binds `viewport`, or in a pass with a depth buffer a
    /// slot binding it next in the pass's order.
    fn claim_overlay(
        &mut self,
        renderer: &mut GpuRenderer,
        viewport: ViewportUniformParams,
        slot: usize,
    ) -> usize {
        if !self.depth {
            return slot;
        }
        let base = self.depth_seq;
        self.depth_seq = base.saturating_add(1);
        renderer.claim_uniform_slot(ViewportUniformParams {
            depth_base: base as f32,
            ..viewport
        })
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

    /// Reserves the pass-order indices of `draws`' records, which are
    /// instanced from their table's start, and returns the first.
    fn take_depth_range(&mut self, draws: &[RunDrawCall]) -> f32 {
        let base = self.depth_seq;
        let records = draws.iter().map(|draw| draw.records.end).max().unwrap_or(0);
        self.depth_seq = base.saturating_add(records);
        base as f32
    }

    /// Draws everything held: the open chunk's shapes, then the held glyphs
    /// above them.
    fn flush(&mut self, renderer: &mut GpuRenderer, binding: SegmentBinding) {
        self.close_chunk(renderer, binding);
        let Some(cmds) = self.pending_glyphs.take() else {
            return;
        };
        let continues = self.overlay_segment == Some(binding.uniform_slot);
        match self.batches.last_mut() {
            Some(Batch::Glyphs { cmds: last, .. }) if last.end == cmds.start && continues => {
                last.end = cmds.end;
            }
            _ => {
                let uniform_slot = self.overlay_slot(renderer, binding);
                self.batches.push(Batch::Glyphs {
                    cmds,
                    uniform_slot,
                    scissor: binding.scissor,
                });
            }
        }
    }

    /// The target pixels a shape run can touch, a pixel wider on each side
    /// for its antialiased edge.
    fn run_target_bounds(&self, draw: &RunDraw, run: &SegmentRun<'s, '_>) -> Option<TargetRect> {
        let bounds = run_draw_bounds(draw, run.segment.scale)?;
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

    fn run_items(
        &mut self,
        renderer: &mut GpuRenderer,
        items: &mut Peekable<impl Iterator<Item = Item<'s>>>,
        run: &SegmentRun<'s, '_>,
    ) {
        while let Some(Item::Run(draw, window)) =
            items.next_if(|item| matches!(item, Item::Run(..)))
        {
            if self
                .run_target_bounds(draw, run)
                .is_some_and(|bounds| self.pending_glyphs.overlaps(bounds))
            {
                self.draw_held_glyphs(renderer, run.binding);
            }
            let window = window.unwrap_or(0..u32::MAX);
            if renderer.run_is_stored(draw) {
                self.close_chunk(renderer, run.binding);
                let viewport = ViewportUniformParams {
                    depth_base: self.depth_seq as f32,
                    ..run.viewport
                };
                let batch = renderer.prepare_store_run(
                    self.recorder,
                    draw,
                    viewport,
                    run.segment.scale,
                    &window,
                    self.depth,
                );
                if self.depth {
                    self.take_depth_range(&batch.draws);
                }
                self.batches.push(Batch::StoreRun {
                    batch,
                    scissor: run.segment.scissor,
                });
            } else {
                let total = draw.record_count().min(window.end);
                let mut from = window.start;
                let clip =
                    crate::render::clip_scissor(&draw.placement, run.segment.scale, run.viewport);
                if let Some(open) = self.chunk
                    && clip != self.chunk_clip
                {
                    self.cut_paint(renderer, run.binding, open);
                }
                self.chunk_clip = clip;
                let clipped = draw.placement.clip.is_some() && clip.is_none();
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
                        (self.depth, clipped),
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
                &mut scratch.image_vertices,
                &mut scratch.image_indices,
                &mut scratch.image_cmds,
            )?;
        }
        if cmd_start < scratch.image_cmds.len() {
            let uniform_slot = self.image_slot(renderer, run);
            self.batches.push(Batch::Images {
                cmds: cmd_start..scratch.image_cmds.len(),
                blend_mode,
                uniform_slot,
                scissor: run.segment.scissor,
            });
        }
        Ok(())
    }

    /// Draws one text as glyphs when its glyphs are in the atlas, joining
    /// the previous glyph batch, else as image quads joining the previous
    /// src-over image batch.
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
            let glyph_end = scratch.glyph_cmds.len();
            if glyph_start < glyph_end {
                if self.pending_glyphs.full() {
                    self.flush(renderer, run.binding);
                }
                self.pending_glyphs.hold(
                    glyph_start..glyph_end,
                    scratch.glyph_cmds[glyph_start..glyph_end]
                        .iter()
                        .map(crate::render::GlyphDrawCmd::bounds),
                );
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
        if cmd_start < scratch.image_cmds.len() {
            let continues = self.overlay_images == Some(run.viewport);
            match self.batches.last_mut() {
                Some(Batch::Images {
                    cmds, blend_mode, ..
                }) if cmds.end == cmd_start && *blend_mode == BlendMode::SrcOver && continues => {
                    cmds.end = scratch.image_cmds.len();
                }
                _ => {
                    let uniform_slot = self.image_slot(renderer, run);
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
