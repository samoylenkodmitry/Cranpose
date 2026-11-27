// A layer's flat children resolved ahead of their z: from the cache, drawn
// in place, or rendered with the others into atlases.

use std::{borrow::Cow, rc::Rc};

use cranpose_render_common::raster_cache::LayerRasterCacheKey;

use super::{
    AtlasPacker, AtlasPlacement, BatchMember, Beneath, ChildFrame, CompositeSource, DeviceRect,
    FrameExecutor, LayerPass, Resolved, SourceDecision, StandInPixels, SurfacePlan, SurfaceRender,
    child_surface_bound, composites_nothing, renders_flat, square_width, z_ordered_ops,
};
use crate::{
    collect::{ChildLayer, LayerScene},
    draw_pass::{PassSegment, PassTarget, SegmentLayout, SourceContent},
    frame_graph::{FrameCommandRecorder, TextureRegionCopy, UploadMode, copy_compatible},
    geometry::SegmentTransform,
    layer_cache::Retained,
    offscreen::OffscreenTarget,
    render::admission_gate::AdmissionGate,
    scene::DrawOp,
};

#[expect(clippy::multiple_inherent_impl, reason = "flat children in their file")]
impl<C: FrameCommandRecorder> FrameExecutor<'_, '_, C> {
    /// Whether a child that can draw in place does this frame: unless its
    /// surface is cached or retained this frame, which it then resolves into
    /// the pass for `resolve_child`. Content that holds still costs less to
    /// composite from its cached surface than to draw again, and content that
    /// changes every frame, whose surface the cache stops keeping, would be
    /// drawn afresh into a surface it throws away.
    pub(super) fn draws_in_place(
        &mut self,
        pass: &mut LayerPass<'_>,
        index: usize,
        child: &ChildLayer,
    ) -> Result<bool, String> {
        let scale = pass.scale;
        let target = pass.window();
        let frame = ChildFrame::of(child, scale, &target);
        let Some(visible) = frame.visible else {
            return Ok(true);
        };
        let shown = child_surface_bound(child, frame.snap, scale, &target).unwrap_or(visible);
        let Some(plan) = SurfacePlan::of(child, scale, frame.grid, shown) else {
            return Ok(true);
        };
        let surface = match self.source_decision(child, &plan, AdmissionGate::drawn_in_place) {
            SourceDecision::Cached(surface) => surface,
            SourceDecision::Render(None) => return Ok(true),
            SourceDecision::Render(retain) => {
                let cleared = Beneath::over(wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
                self.render_planned_surface(child, &plan, retain, &cleared)?
            }
        };
        pass.resolve(index, Resolved::Surface(Some(surface)));
        Ok(false)
    }

    /// Resolves every flat child ahead of its z: from the cache, drawn in
    /// place, or rendered with the others into atlases, so a frame that
    /// renders many flat surfaces -- a grid laid out again, or one whose cells
    /// just held still long enough to be kept -- renders them in a few passes.
    pub(super) fn resolve_flat_children(&mut self, pass: &mut LayerPass<'_>) -> Result<(), String> {
        let layer = pass.layer;
        if !layer.children.iter().any(renders_flat) {
            return Ok(());
        }
        let scale = pass.scale;
        let target = pass.window();
        let mut resolved: Vec<Option<Resolved>> = std::iter::repeat_with(|| None)
            .take(layer.children.len())
            .collect();
        let mut batch: Vec<BatchMember> = Vec::new();
        for (index, child) in layer.children.iter().enumerate() {
            if !renders_flat(child) || !child.mesh.is_empty() {
                continue;
            }
            let frame = ChildFrame::of(child, scale, &target);
            let Some(visible) = frame.visible else {
                continue;
            };
            if composites_nothing(child) {
                continue;
            }
            let in_place = pass.can_draw_in_place(child);
            let shown = child_surface_bound(child, frame.snap, scale, &target).unwrap_or(visible);
            let Some(plan) = SurfacePlan::of(child, scale, frame.grid, shown) else {
                resolved[index] = Some(if in_place {
                    Resolved::InPlace
                } else {
                    Resolved::Surface(None)
                });
                continue;
            };
            // A member rendered with the others lands in an atlas: a kept
            // one is read there in place copy-free, else copied out of it.
            let gate = if in_place {
                AdmissionGate::drawn_in_place
            } else if self.renderer.copy_free == UploadMode::Mapped {
                AdmissionGate::rendered
            } else {
                AdmissionGate::copied_out_of_atlas
            };
            resolved[index] = match self.source_decision(child, &plan, gate) {
                SourceDecision::Cached(surface) => Some(Resolved::Surface(Some(surface))),
                SourceDecision::Render(None) if in_place => Some(Resolved::InPlace),
                SourceDecision::Render(retain) => {
                    batch.push(BatchMember {
                        index,
                        plan,
                        retain,
                    });
                    None
                }
            };
        }
        for (index, surface) in self.render_surface_batch(layer, batch)? {
            resolved[index] = Some(Resolved::Surface(Some(surface)));
        }
        pass.resolved = resolved;
        Ok(())
    }

    /// Renders the batch's members together. Copy-free, those the cache
    /// keeps go into an atlas of their own, which the cache keeps while it
    /// keeps any of them and reads in place, and the others into one the
    /// frame lets go; otherwise all share one atlas the kept ones are
    /// copied out of. See [`UploadMode::copy_free`].
    fn render_surface_batch(
        &mut self,
        layer: &LayerScene,
        mut batch: Vec<BatchMember>,
    ) -> Result<Vec<(usize, SurfaceRender)>, String> {
        let mut surfaces = Vec::with_capacity(batch.len());
        let kept = if self.renderer.copy_free == UploadMode::Mapped {
            batch.sort_by_key(|member| member.retain.is_none());
            batch.partition_point(|member| member.retain.is_some())
        } else {
            0
        };
        let (kept, drawn) = batch.split_at(kept);
        for (group, keeps) in [(kept, true), (drawn, false)] {
            match group {
                [] => {}
                [member] => {
                    surfaces.push((member.index, self.render_member_alone(layer, member)?));
                }
                group => self.render_surface_atlas(layer, group, keeps, &mut surfaces)?,
            }
        }
        Ok(surfaces)
    }

    fn render_member_alone(
        &mut self,
        layer: &LayerScene,
        member: &BatchMember,
    ) -> Result<SurfaceRender, String> {
        let cleared = Beneath::over(wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
        self.render_planned_surface(
            &layer.children[member.index],
            &member.plan,
            member.retain,
            &cleared,
        )
    }

    fn render_surface_atlas(
        &mut self,
        layer: &LayerScene,
        group: &[BatchMember],
        keeps: bool,
        surfaces: &mut Vec<(usize, SurfaceRender)>,
    ) -> Result<(), String> {
        let limit = self.renderer.max_texture_dim();
        let shelf_width = if keeps {
            square_width(group, limit)
        } else {
            self.renderer.surface_atlases.shelf_width(group, limit)
        };
        let mut packer = AtlasPacker::new(limit).with_shelf_width(shelf_width);
        // A kept surface is read in place or copied out, so where it lands
        // costs nothing later: the tallest go first, into an atlas near
        // square.
        let mut tallest_first: Vec<usize> = (0..group.len()).collect();
        tallest_first.sort_by_key(|&index| std::cmp::Reverse(group[index].plan.height));
        let mut placements: Vec<Option<AtlasPlacement>> = vec![None; group.len()];
        for index in tallest_first {
            let plan = &group[index].plan;
            placements[index] = packer.place(plan.width, plan.height);
        }
        // A kept atlas stays in the cache for as long as its members do: it
        // takes its own size from the retained surfaces' pool, as the kept
        // surfaces did. A pooled size that serves later frames' atlases
        // held a kept one at up to four times the area its members need.
        let atlases: Vec<Rc<OffscreenTarget>> = packer
            .atlases
            .iter()
            .map(|atlas| {
                let renderer = &mut *self.renderer;
                if keeps {
                    let size = atlas.padded_size(limit);
                    Rc::new(renderer.acquire_retained_surface(size.0, size.1))
                } else {
                    let stats = &renderer.frame_stats;
                    let atlases = &mut renderer.surface_atlases;
                    atlases.acquire(&renderer.device, atlas, limit, stats)
                }
            })
            .collect();
        // Members share their passes, so one stand-in marks them all.
        let pixels = self.encode_atlases(layer, group, &placements, &atlases)?;
        for (member, placement) in group.iter().zip(placements) {
            let child = &layer.children[member.index];
            let surface = match placement {
                Some(placement) => {
                    self.renderer.frame_stats.record_isolated_layer_render(
                        member.plan.width,
                        member.plan.height,
                        child.node_id,
                        member.plan.surface_logical,
                    );
                    self.retain_from_atlas(
                        child,
                        member,
                        &atlases[placement.atlas],
                        (placement, keeps, pixels),
                    )
                }
                None => self.render_member_alone(layer, member)?,
            };
            surfaces.push((member.index, surface));
        }
        Ok(())
    }

    // Draws `group`'s members into `atlases` where `placements` put them,
    // and the pixels it drew.
    fn encode_atlases(
        &mut self,
        layer: &LayerScene,
        group: &[BatchMember],
        placements: &[Option<AtlasPlacement>],
        atlases: &[Rc<OffscreenTarget>],
    ) -> Result<StandInPixels, String> {
        let mark = self.renderer.stand_in_mark();
        let ops: Vec<Cow<'_, [DrawOp]>> = group
            .iter()
            .map(|member| z_ordered_ops(&layer.children[member.index].content.scene.draw_ops))
            .collect();
        for (atlas_index, atlas) in atlases.iter().enumerate() {
            let segments: Vec<PassSegment<'_>> = group
                .iter()
                .zip(placements)
                .zip(&ops)
                .filter_map(|((member, placement), ops)| {
                    let placement = placement.filter(|placement| placement.atlas == atlas_index)?;
                    let plan = &member.plan;
                    Some(PassSegment {
                        scene: &layer.children[member.index].content.scene,
                        ops,
                        composites: &[],
                        offset: [
                            plan.surface_rect.x - placement.x as f32,
                            plan.surface_rect.y - placement.y as f32,
                        ],
                        scissor: Some((placement.x, placement.y, plan.width, plan.height)),
                        first_run_window: None,
                        transform: SegmentTransform::IDENTITY,
                        scale: plan.surface_scale,
                    })
                })
                .collect();
            let target = PassTarget {
                view: &atlas.view,
                width: atlas.width,
                height: atlas.height,
            };
            self.renderer.encode_pass(
                self.recorder,
                target,
                (&segments, SegmentLayout::SideBySide),
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                "Layer Surface Atlas Pass",
            )?;
        }
        Ok(self.pixels_since(mark))
    }

    /// The member's surface in `atlas`, kept by the cache when it admits
    /// it: in place when the atlas holds kept members alone, else copied
    /// out into a texture of its own.
    fn retain_from_atlas(
        &mut self,
        child: &ChildLayer,
        member: &BatchMember,
        atlas: &Rc<OffscreenTarget>,
        (placement, in_place, pixels): (AtlasPlacement, bool, StandInPixels),
    ) -> SurfaceRender {
        let plan = &member.plan;
        let in_atlas = plan.surface(
            CompositeSource {
                texture: Rc::clone(atlas),
                content: SourceContent::Transient,
            },
            Some(DeviceRect {
                x: placement.x as f32,
                y: placement.y as f32,
                width: plan.width as f32,
                height: plan.height as f32,
            }),
            pixels,
        );
        let Some(key) = member.retain else {
            return in_atlas;
        };
        if !in_place {
            return self.copy_out_of_atlas(child, member, atlas, placement, key, in_atlas);
        }
        let region = DeviceRect {
            x: placement.x as f32,
            y: placement.y as f32,
            width: plan.width as f32,
            height: plan.height as f32,
        };
        let transient = self.transient_descriptor(atlas);
        if !self.retain_source(
            child.node_id,
            key,
            Retained::surface_in(Rc::clone(atlas), region, pixels),
            transient,
        ) {
            return in_atlas;
        }
        plan.surface(
            CompositeSource {
                texture: Rc::clone(atlas),
                content: SourceContent::retained(&key),
            },
            Some(region),
            pixels,
        )
    }

    fn copy_out_of_atlas(
        &mut self,
        child: &ChildLayer,
        member: &BatchMember,
        atlas: &Rc<OffscreenTarget>,
        placement: AtlasPlacement,
        key: LayerRasterCacheKey,
        in_atlas: SurfaceRender,
    ) -> SurfaceRender {
        let plan = &member.plan;
        let retained = Rc::new(
            self.renderer
                .acquire_retained_surface(plan.width, plan.height),
        );
        if !copy_compatible(atlas, &retained) {
            return in_atlas;
        }
        self.recorder.copy_texture_region(TextureRegionCopy {
            source: atlas,
            source_origin: [placement.x, placement.y],
            dest: &retained,
            dest_origin: [0, 0],
            size: [plan.width, plan.height],
        });
        let pixels = in_atlas.pixels;
        if !self.retain_source(
            child.node_id,
            key,
            Retained::surface(Rc::clone(&retained), pixels),
            None,
        ) {
            return in_atlas;
        }
        plan.surface(
            CompositeSource {
                texture: retained,
                content: SourceContent::retained(&key),
            },
            None,
            pixels,
        )
    }
}
