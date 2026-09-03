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

/// A resolved texture drawn into the pass at its z, described in the scene's
/// device space so one description serves every target the scene is drawn
/// into.
pub(crate) struct ResolvedComposite {
    pub(crate) z_index: usize,
    pub(crate) source: Rc<OffscreenTarget>,
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
        let mut batches: Vec<Batch<'s>> = Vec::new();
        let mut image_vertices = std::mem::take(&mut self.scratch_image_vertices);
        let mut image_indices = std::mem::take(&mut self.scratch_image_indices);
        let mut image_cmds = std::mem::take(&mut self.scratch_image_cmds);
        let mut glyph_cmds = std::mem::take(&mut self.scratch_glyph_cmds);
        image_vertices.clear();
        image_indices.clear();
        image_cmds.clear();
        glyph_cmds.clear();
        let target_size = (target.width, target.height);
        let device = self.device.clone();

        let mut prepare = |renderer: &mut Self,
                           batches: &mut Vec<Batch<'s>>,
                           image_vertices: &mut Vec<crate::render::Vertex>,
                           image_indices: &mut Vec<u32>,
                           image_cmds: &mut Vec<crate::render::ImageDrawCmd>,
                           glyph_cmds: &mut Vec<crate::render::GlyphDrawCmd>|
         -> Result<(), String> {
            for segment in segments {
                let viewport = ViewportUniformParams {
                    width: target.width,
                    height: target.height,
                    offset: segment.offset,
                };
                let viewport_rect = match segment.scissor {
                    Some((x, y, width, height)) => PassTarget {
                        offset: [segment.offset[0] + x as f32, segment.offset[1] + y as f32],
                        width,
                        height,
                        ..target
                    },
                    None => PassTarget {
                        offset: segment.offset,
                        ..target
                    },
                }
                .logical_rect(root_scale);
                let uniform_slot = renderer.claim_uniform_slot(viewport);
                let items = merge_items(segment, viewport_rect, root_scale, target_size);
                let mut index = 0;
                while index < items.len() {
                    match &items[index] {
                        Item::Shape { shape, brushes } => {
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
                            if let Some(batch) = renderer.prepare_shape_batch(
                                shapes,
                                brushes,
                                root_scale,
                                uniform_slot,
                            ) {
                                batches.push(Batch::Shapes {
                                    batch,
                                    blend_mode,
                                    scissor: segment.scissor,
                                });
                            }
                            index = end;
                        }
                        Item::Image(image_index) => {
                            let blend_mode =
                                supported_blend_mode(segment.scene.images[*image_index].blend_mode);
                            let cmd_start = image_cmds.len();
                            let mut end = index;
                            while end < items.len() {
                                let Item::Image(other) = &items[end] else {
                                    break;
                                };
                                let image = &segment.scene.images[*other];
                                if supported_blend_mode(image.blend_mode) != blend_mode {
                                    break;
                                }
                                renderer.append_image_draw_cmd(
                                    image,
                                    viewport,
                                    root_scale,
                                    image_vertices,
                                    image_indices,
                                    image_cmds,
                                )?;
                                end += 1;
                            }
                            if cmd_start < image_cmds.len() {
                                batches.push(Batch::Images {
                                    cmds: cmd_start..image_cmds.len(),
                                    blend_mode,
                                    uniform_slot,
                                    scissor: segment.scissor,
                                });
                            }
                            index = end;
                        }
                        Item::Text(text) => {
                            let glyph_start = glyph_cmds.len();
                            let drew_glyphs = renderer.append_text_glyph_draws(
                                std::iter::once(*text),
                                viewport,
                                root_scale,
                                image_vertices,
                                image_indices,
                                glyph_cmds,
                            )?;
                            if drew_glyphs {
                                if glyph_start < glyph_cmds.len() {
                                    match batches.last_mut() {
                                        Some(Batch::Glyphs {
                                            cmds,
                                            uniform_slot: slot,
                                            ..
                                        }) if cmds.end == glyph_start && *slot == uniform_slot => {
                                            cmds.end = glyph_cmds.len();
                                        }
                                        _ => batches.push(Batch::Glyphs {
                                            cmds: glyph_start..glyph_cmds.len(),
                                            uniform_slot,
                                            scissor: segment.scissor,
                                        }),
                                    }
                                }
                            } else {
                                let cmd_start = image_cmds.len();
                                renderer.append_text_image_draw_cmds(
                                    std::iter::once(*text),
                                    viewport,
                                    root_scale,
                                    image_vertices,
                                    image_indices,
                                    image_cmds,
                                )?;
                                if cmd_start < image_cmds.len() {
                                    match batches.last_mut() {
                                        Some(Batch::Images {
                                            cmds,
                                            blend_mode,
                                            uniform_slot: slot,
                                            ..
                                        }) if cmds.end == cmd_start
                                            && *blend_mode == BlendMode::SrcOver
                                            && *slot == uniform_slot =>
                                        {
                                            cmds.end = image_cmds.len();
                                        }
                                        _ => batches.push(Batch::Images {
                                            cmds: cmd_start..image_cmds.len(),
                                            blend_mode: BlendMode::SrcOver,
                                            uniform_slot,
                                            scissor: segment.scissor,
                                        }),
                                    }
                                }
                            }
                            index += 1;
                        }
                        Item::Composite(composite) => {
                            let own_scissor = composite.scissor.and_then(|scissor| {
                                scissor_in_target(scissor, target_size, segment.offset)
                            });
                            if composite.scissor.is_some() && own_scissor.is_none() {
                                index += 1;
                                continue;
                            }
                            let Some(scissor) = intersect_scissors(own_scissor, segment.scissor)
                            else {
                                index += 1;
                                continue;
                            };
                            let dest = dest_in_target(composite.dest, segment.offset);
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
                                        rounded_mask: mask_in_target(*rounded_mask, segment.offset),
                                        blend_mode: supported_blend_mode(*blend_mode),
                                        dest_viewport: Some(dest),
                                        source_viewport: *source_viewport,
                                        sample_mode: *sample_mode,
                                    };
                                    let prepared =
                                        renderer.effect_renderer.prepare_composite_batch_draws(
                                            recorder,
                                            &device,
                                            load_op,
                                            std::slice::from_ref(&item),
                                        );
                                    batches.extend(prepared.into_iter().map(Batch::Composite));
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
                                        rounded_mask: mask_in_target(*rounded_mask, segment.offset),
                                        alpha: *alpha,
                                        scissor,
                                        dest_viewport: dest,
                                    };
                                    let prepared = renderer
                                        .effect_renderer
                                        .prepare_shader_batch_draws(
                                            recorder,
                                            &device,
                                            std::slice::from_ref(&item),
                                        )
                                        .ok_or_else(|| {
                                            "shader composite preparation failed".to_string()
                                        })?;
                                    renderer.effect_renderer.record_composite_pass();
                                    batches.extend(prepared.into_iter().map(Batch::Shader));
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
                                        viewport: target_size,
                                        dest_quad: dest_quad.map(|[x, y]| {
                                            [x - segment.offset[0], y - segment.offset[1]]
                                        }),
                                        inverse: translate_inverse(*inverse, segment.offset),
                                        alpha: *alpha,
                                        blend_mode: supported_blend_mode(*blend_mode),
                                        sample_mode: *sample_mode,
                                    };
                                    let prepared =
                                        renderer.effect_renderer.prepare_projective_composite_draw(
                                            recorder, &device, &item,
                                        );
                                    batches.push(Batch::Projective(prepared));
                                }
                            }
                            index += 1;
                        }
                    }
                }
            }
            Ok(())
        };
        let prepared = prepare(
            self,
            &mut batches,
            &mut image_vertices,
            &mut image_indices,
            &mut image_cmds,
            &mut glyph_cmds,
        );
        let image_slot = match &prepared {
            Ok(()) if !image_indices.is_empty() => {
                Some(self.upload_image_slot(&image_vertices, &image_indices))
            }
            _ => None,
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
                let draw_result = {
                    let mut pass = recorder.begin_timed_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some(label),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: target.view,
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
                    self.draw_batches(
                        &mut pass,
                        target_size,
                        &batches,
                        &image_cmds,
                        &glyph_cmds,
                        image_slot,
                    )
                };
                recorder.record_pass();
                draw_result.map(|()| true)
            }
        };
        drop(batches);
        self.scratch_image_vertices = image_vertices;
        self.scratch_image_indices = image_indices;
        self.scratch_image_cmds = image_cmds;
        self.scratch_glyph_cmds = glyph_cmds;
        result
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
