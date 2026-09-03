use std::rc::Rc;

use cranpose_ui_graphics::{BlendMode, Brush, Rect, RuntimeShader};

use crate::{
    effect_renderer::{
        CompositeBatchItem, CompositeSampleMode, PreparedCompositeDraw,
        PreparedProjectiveComposite, PreparedShaderDraw, ProjectiveCompositeItem,
        RoundedCompositeMask, ShaderCompositeBatchItem,
    },
    frame_graph::FrameCommandRecorder,
    offscreen::OffscreenTarget,
    render::{
        GpuRenderer, ShapeBatch, ViewportUniformParams, shape_draw_is_visible_in_rect,
        supported_blend_mode,
    },
    scene::{CompositorScene, DrawOp, DrawOpKind, DrawShape, TextDraw},
};

/// A render target and where its origin sits in the scene's device space.
#[derive(Clone, Copy)]
pub(crate) struct PassTarget<'a> {
    pub(crate) view: &'a wgpu::TextureView,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) offset: [f32; 2],
}

impl PassTarget<'_> {
    pub(crate) fn logical_rect(&self, root_scale: f32) -> Rect {
        Rect {
            x: self.offset[0] / root_scale,
            y: self.offset[1] / root_scale,
            width: self.width as f32 / root_scale,
            height: self.height as f32 / root_scale,
        }
    }
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
pub(crate) struct ResolvedComposite {
    pub(crate) z_index: usize,
    pub(crate) source: Rc<OffscreenTarget>,
    pub(crate) content: SourceContent,
    pub(crate) dest: (f32, f32, f32, f32),
    pub(crate) scissor: Option<(f32, f32, f32, f32)>,
    pub(crate) kind: ResolvedCompositeKind,
}

pub(crate) enum ResolvedCompositeKind {
    Blit {
        alpha: f32,
        blend_mode: BlendMode,
        rounded_mask: Option<RoundedCompositeMask>,
        sample_mode: CompositeSampleMode,
        source_viewport: Option<(f32, f32, f32, f32)>,
    },
    Shader {
        shader: Rc<RuntimeShader>,
        layer_pixel_rect: [f32; 4],
        source_region: Option<(f32, f32, f32, f32)>,
        rounded_mask: Option<RoundedCompositeMask>,
        alpha: f32,
    },
    Projective {
        dest_quad: [[f32; 2]; 4],
        inverse: [[f32; 3]; 3],
        alpha: f32,
        blend_mode: BlendMode,
        sample_mode: CompositeSampleMode,
    },
}

/// One scene's contribution to a pass: its ops in z order, the composites
/// resolved for it, where its device space origin sits in the target's
/// scene space, and the target pixels it may touch (the whole target when
/// `None`).
pub(crate) struct PassSegment<'a> {
    pub(crate) scene: &'a CompositorScene,
    pub(crate) ops: &'a [DrawOp],
    pub(crate) composites: &'a [ResolvedComposite],
    pub(crate) offset: [f32; 2],
    pub(crate) scissor: Option<(u32, u32, u32, u32)>,
}

enum Item<'a> {
    Shape {
        shape: &'a DrawShape,
        brushes: &'a [Brush],
    },
    Image(usize),
    Text(&'a TextDraw),
    Composite(&'a ResolvedComposite),
}

enum Batch<'a> {
    Shapes {
        batch: ShapeBatch,
        blend_mode: BlendMode,
        scissor: Option<(u32, u32, u32, u32)>,
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
) -> bool {
    let (x, y, width, height) = dest_in_target(composite.dest, segment_offset);
    if x >= target_size.0 as f32
        || y >= target_size.1 as f32
        || x + width <= 0.0
        || y + height <= 0.0
    {
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
        root_scale: f32,
        label: &'static str,
    ) -> Result<bool, String> {
        let mut scratch = self.take_pass_scratch();
        let device = self.device.clone();
        let mut prep = PassPrep {
            recorder,
            device: &device,
            target,
            root_scale,
            load_op,
            batches: Vec::new(),
        };
        let prepared = segments
            .iter()
            .try_for_each(|segment| prep.segment(self, segment, &mut scratch));
        let image_slot = match &prepared {
            Ok(()) if !scratch.image_indices.is_empty() => {
                Some(self.upload_image_slot(&scratch.image_vertices, &scratch.image_indices))
            }
            _ => None,
        };
        let batches = prep.batches;
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
                let draw_result = {
                    let mut pass = recorder.begin_color_pass(label, target.view, load_op);
                    self.draw_batches(
                        &mut pass,
                        (target.width, target.height),
                        &batches,
                        &scratch.image_cmds,
                        &scratch.glyph_cmds,
                        image_slot,
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
            glyph_cmds: std::mem::take(&mut self.scratch_glyph_cmds),
        };
        scratch.image_vertices.clear();
        scratch.image_indices.clear();
        scratch.image_cmds.clear();
        scratch.glyph_cmds.clear();
        scratch
    }

    fn return_pass_scratch(&mut self, scratch: PassScratch) {
        self.scratch_image_vertices = scratch.image_vertices;
        self.scratch_image_indices = scratch.image_indices;
        self.scratch_image_cmds = scratch.image_cmds;
        self.scratch_glyph_cmds = scratch.glyph_cmds;
    }

    fn draw_batches(
        &mut self,
        pass: &mut wgpu::RenderPass<'_>,
        target_size: (u32, u32),
        batches: &[Batch<'_>],
        image_cmds: &[crate::render::ImageDrawCmd],
        glyph_cmds: &[crate::render::GlyphDrawCmd],
        image_slot: Option<usize>,
    ) -> Result<(), String> {
        for batch in batches {
            match batch {
                Batch::Shapes {
                    batch,
                    blend_mode,
                    scissor,
                } => {
                    self.draw_shape_batch(pass, *blend_mode, batch, target_size, *scissor)?;
                }
                Batch::Images {
                    cmds,
                    blend_mode,
                    uniform_slot,
                    scissor,
                } => {
                    let slot = image_slot
                        .ok_or_else(|| "image batch without an image slot".to_string())?;
                    self.draw_image_cmds(
                        pass,
                        slot,
                        *uniform_slot,
                        &image_cmds[cmds.clone()],
                        *blend_mode,
                        *scissor,
                    )?;
                }
                Batch::Glyphs {
                    cmds,
                    uniform_slot,
                    scissor,
                } => {
                    self.draw_glyph_cmds(
                        pass,
                        image_slot,
                        *uniform_slot,
                        &glyph_cmds[cmds.clone()],
                        *scissor,
                    )?;
                }
                Batch::Composite(prepared) => {
                    self.effect_renderer
                        .draw_prepared_composite(pass, target_size, prepared);
                }
                Batch::Shader(prepared) => {
                    self.effect_renderer.draw_prepared_shader_src_over(
                        &self.device,
                        pass,
                        target_size,
                        prepared,
                    );
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

/// Re-bases an inverse (target pixel -> source pixel) matrix onto a target
/// whose origin is `offset` pixels into the space the matrix was built for.
fn translate_inverse(inverse: [[f32; 3]; 3], offset: [f32; 2]) -> [[f32; 3]; 3] {
    let mut shifted = inverse;
    for row in &mut shifted {
        row[2] += row[0] * offset[0] + row[1] * offset[1];
    }
    shifted
}

fn merge_items<'a>(
    segment: &PassSegment<'a>,
    viewport_rect: Rect,
    root_scale: f32,
    target_size: (u32, u32),
) -> Vec<Item<'a>> {
    let mut items = Vec::with_capacity(segment.ops.len() + segment.composites.len());
    let mut composites = segment.composites.iter().peekable();
    let mut push_composites_below = |items: &mut Vec<Item<'a>>, z: usize| {
        while let Some(composite) = composites.peek() {
            if composite.z_index > z {
                break;
            }
            let composite = composites.next().expect("peeked composite");
            if composite_visible(composite, target_size, segment.offset) {
                items.push(Item::Composite(composite));
            }
        }
    };
    for op in segment.ops {
        push_composites_below(&mut items, op.z_index);
        match op.kind {
            DrawOpKind::Shape(index) => {
                let shape = &segment.scene.shapes[index];
                if shape_draw_is_visible_in_rect(shape, viewport_rect, root_scale) {
                    items.push(Item::Shape {
                        shape,
                        brushes: &segment.scene.brushes,
                    });
                }
            }
            DrawOpKind::Image(index) => items.push(Item::Image(index)),
            DrawOpKind::Text(index) => items.push(Item::Text(&segment.scene.texts[index])),
            DrawOpKind::Shadow(index) => {
                let shadow = &segment.scene.shadow_draws[index];
                if shadow.blur_radius > 0.0 {
                    continue;
                }
                for (shape, _) in &shadow.shapes {
                    if shape_draw_is_visible_in_rect(shape, viewport_rect, root_scale) {
                        items.push(Item::Shape {
                            shape,
                            brushes: &shadow.brushes,
                        });
                    }
                }
                for text in &shadow.texts {
                    items.push(Item::Text(text));
                }
            }
        }
    }
    push_composites_below(&mut items, usize::MAX);
    items
}

/// The per-frame vectors a pass fills: image and glyph geometry and draw
/// commands, kept on the renderer between frames so they never reallocate.
struct PassScratch {
    image_vertices: Vec<crate::render::Vertex>,
    image_indices: Vec<u32>,
    image_cmds: Vec<crate::render::ImageDrawCmd>,
    glyph_cmds: Vec<crate::render::GlyphDrawCmd>,
}

/// Turns the segments of one pass into batches, one item run at a time.
struct PassPrep<'a, 's, C> {
    recorder: &'a mut C,
    device: &'a wgpu::Device,
    target: PassTarget<'a>,
    root_scale: f32,
    load_op: wgpu::LoadOp<wgpu::Color>,
    batches: Vec<Batch<'s>>,
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
        let viewport = ViewportUniformParams {
            width: self.target.width,
            height: self.target.height,
            offset: segment.offset,
        };
        let viewport_rect = match segment.scissor {
            Some((x, y, width, height)) => PassTarget {
                offset: [segment.offset[0] + x as f32, segment.offset[1] + y as f32],
                width,
                height,
                ..self.target
            },
            None => PassTarget {
                offset: segment.offset,
                ..self.target
            },
        }
        .logical_rect(self.root_scale);
        let uniform_slot = renderer.claim_uniform_slot(viewport);
        let items = merge_items(segment, viewport_rect, self.root_scale, self.target_size());
        let run = SegmentRun {
            segment,
            viewport,
            uniform_slot,
        };
        let mut index = 0;
        while index < items.len() {
            index = match &items[index] {
                Item::Shape { .. } => self.shape_run(renderer, &items, index, &run),
                Item::Image(_) => self.image_run(renderer, &items, index, &run, scratch)?,
                Item::Text(text) => {
                    self.text_item(renderer, text, &run, scratch)?;
                    index + 1
                }
                Item::Composite(composite) => {
                    self.composite_item(renderer, composite, &run)?;
                    index + 1
                }
            };
        }
        Ok(())
    }

    /// Batches the shapes from `index` that share a blend mode and a brush
    /// table, up to the batch limit; returns where the run ends.
    fn shape_run(
        &mut self,
        renderer: &mut GpuRenderer,
        items: &[Item<'s>],
        index: usize,
        run: &SegmentRun<'s, '_>,
    ) -> usize {
        let Item::Shape { shape, brushes } = &items[index] else {
            unreachable!("shape run starts at a shape");
        };
        let blend_mode = supported_blend_mode(shape.blend_mode);
        let brushes_ptr = brushes.as_ptr();
        let limit = renderer.max_shapes_per_batch();
        let mut end = index;
        while end < items.len()
            && end - index < limit
            && matches!(
                &items[end],
                Item::Shape { shape, brushes }
                    if supported_blend_mode(shape.blend_mode) == blend_mode
                        && brushes.as_ptr() == brushes_ptr
            )
        {
            end += 1;
        }
        let shapes = items[index..end].iter().map(|item| match item {
            Item::Shape { shape, .. } => *shape,
            _ => unreachable!("shape run holds only shapes"),
        });
        if let Some(batch) =
            renderer.prepare_shape_batch(shapes, brushes, self.root_scale, run.uniform_slot)
        {
            self.batches.push(Batch::Shapes {
                batch,
                blend_mode,
                scissor: run.segment.scissor,
            });
        }
        end
    }

    /// Batches the images from `index` that share a blend mode; returns
    /// where the run ends.
    fn image_run(
        &mut self,
        renderer: &mut GpuRenderer,
        items: &[Item<'s>],
        index: usize,
        run: &SegmentRun<'s, '_>,
        scratch: &mut PassScratch,
    ) -> Result<usize, String> {
        let Item::Image(first) = &items[index] else {
            unreachable!("image run starts at an image");
        };
        let images = &run.segment.scene.images;
        let blend_mode = supported_blend_mode(images[*first].blend_mode);
        let cmd_start = scratch.image_cmds.len();
        let mut end = index;
        while let Some(Item::Image(other)) = items.get(end) {
            let image = &images[*other];
            if supported_blend_mode(image.blend_mode) != blend_mode {
                break;
            }
            renderer.append_image_draw_cmd(
                image,
                run.viewport,
                self.root_scale,
                &mut scratch.image_vertices,
                &mut scratch.image_indices,
                &mut scratch.image_cmds,
            )?;
            end += 1;
        }
        if cmd_start < scratch.image_cmds.len() {
            self.batches.push(Batch::Images {
                cmds: cmd_start..scratch.image_cmds.len(),
                blend_mode,
                uniform_slot: run.uniform_slot,
                scissor: run.segment.scissor,
            });
        }
        Ok(end)
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
            std::iter::once(text),
            run.viewport,
            self.root_scale,
            &mut scratch.image_vertices,
            &mut scratch.image_indices,
            &mut scratch.glyph_cmds,
        )?;
        if drew_glyphs {
            if glyph_start < scratch.glyph_cmds.len() {
                match self.batches.last_mut() {
                    Some(Batch::Glyphs {
                        cmds,
                        uniform_slot: slot,
                        ..
                    }) if cmds.end == glyph_start && *slot == run.uniform_slot => {
                        cmds.end = scratch.glyph_cmds.len();
                    }
                    _ => self.batches.push(Batch::Glyphs {
                        cmds: glyph_start..scratch.glyph_cmds.len(),
                        uniform_slot: run.uniform_slot,
                        scissor: run.segment.scissor,
                    }),
                }
            }
            return Ok(());
        }
        let cmd_start = scratch.image_cmds.len();
        renderer.append_text_image_draw_cmds(
            std::iter::once(text),
            run.viewport,
            self.root_scale,
            &mut scratch.image_vertices,
            &mut scratch.image_indices,
            &mut scratch.image_cmds,
        )?;
        if cmd_start < scratch.image_cmds.len() {
            match self.batches.last_mut() {
                Some(Batch::Images {
                    cmds,
                    blend_mode,
                    uniform_slot: slot,
                    ..
                }) if cmds.end == cmd_start
                    && *blend_mode == BlendMode::SrcOver
                    && *slot == run.uniform_slot =>
                {
                    cmds.end = scratch.image_cmds.len();
                }
                _ => self.batches.push(Batch::Images {
                    cmds: cmd_start..scratch.image_cmds.len(),
                    blend_mode: BlendMode::SrcOver,
                    uniform_slot: run.uniform_slot,
                    scissor: run.segment.scissor,
                }),
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
                let prepared = renderer.effect_renderer.prepare_composite_batch_draws(
                    self.recorder,
                    self.device,
                    self.load_op,
                    std::slice::from_ref(&item),
                );
                self.batches
                    .extend(prepared.into_iter().map(Batch::Composite));
            }
            ResolvedCompositeKind::Shader {
                shader,
                layer_pixel_rect,
                source_region,
                rounded_mask,
                alpha,
            } => {
                let item = ShaderCompositeBatchItem {
                    source: composite.source.as_ref(),
                    shader: shader.as_ref(),
                    layer_pixel_rect: *layer_pixel_rect,
                    source_region: *source_region,
                    rounded_mask: mask_in_target(*rounded_mask, offset),
                    alpha: *alpha,
                    scissor,
                    dest_viewport: dest,
                };
                let prepared = renderer
                    .effect_renderer
                    .prepare_shader_batch_draws(
                        self.recorder,
                        self.device,
                        std::slice::from_ref(&item),
                    )
                    .ok_or_else(|| "shader composite preparation failed".to_string())?;
                self.batches.extend(prepared.into_iter().map(Batch::Shader));
            }
            ResolvedCompositeKind::Projective {
                dest_quad,
                inverse,
                alpha,
                blend_mode,
                sample_mode,
            } => {
                let item = ProjectiveCompositeItem {
                    source: composite.source.as_ref(),
                    viewport: self.target_size(),
                    dest_quad: dest_quad.map(|[x, y]| [x - offset[0], y - offset[1]]),
                    inverse: translate_inverse(*inverse, offset),
                    alpha: *alpha,
                    blend_mode: supported_blend_mode(*blend_mode),
                    sample_mode: *sample_mode,
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

/// One segment's viewport and uniform slot while its items are batched.
struct SegmentRun<'s, 'a> {
    segment: &'a PassSegment<'s>,
    viewport: ViewportUniformParams,
    uniform_slot: usize,
}
