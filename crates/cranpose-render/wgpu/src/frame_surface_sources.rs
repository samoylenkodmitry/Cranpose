// A child's surface: from the layer cache, or rendered and kept there.

use std::rc::Rc;

use cranpose_core::NodeId;
use cranpose_render_common::raster_cache::LayerRasterCacheKey;

use super::{
    Beneath, CompositeSource, FrameExecutor, Page, SourceDecision, SurfacePlan, SurfaceRender,
    renders_flat,
};
use crate::{
    collect::ChildLayer,
    draw_pass::SourceContent,
    frame_graph::{FrameCommandRecorder, FrameTextureDescriptor},
    layer_cache::Retained,
    render::admission_gate::{AdmissionGate, NodeGate},
};

#[expect(
    clippy::multiple_inherent_impl,
    reason = "surface sources in their file"
)]
impl<C: FrameCommandRecorder> FrameExecutor<'_, '_, C> {
    /// Whether the child's surface comes from the cache or is rendered, and
    /// whether a rendered one is kept; `gate` makes a new node's source gate.
    pub(super) fn source_decision(
        &mut self,
        child: &ChildLayer,
        plan: &SurfacePlan,
        gate: fn(LayerRasterCacheKey) -> AdmissionGate,
    ) -> SourceDecision {
        let key = plan.cache_key(child);
        if let Some(key) = key
            && let Some(retained) = self.cached_source(child, key, plan.width, plan.height)
        {
            return SourceDecision::Cached(plan.surface(
                CompositeSource {
                    texture: retained.texture,
                    content: SourceContent::retained(&key),
                },
                retained.region,
                retained.pixels,
            ));
        }
        SourceDecision::Render(
            key.filter(|key| {
                self.admits_source(child.node_id, *key, (plan.width, plan.height), gate)
            }),
        )
    }

    pub(super) fn render_planned_surface(
        &mut self,
        child: &ChildLayer,
        plan: &SurfacePlan,
        retain: Option<LayerRasterCacheKey>,
        beneath: &Beneath<'_>,
    ) -> Result<SurfaceRender, String> {
        let texture = if retain.is_some() {
            Rc::new(
                self.renderer
                    .acquire_retained_surface(plan.width, plan.height),
            )
        } else {
            self.acquire_transient("Layer Surface", plan.width, plan.height)
        };
        let child_page = Page {
            texture: Rc::clone(&texture),
            offset: [plan.surface_rect.x, plan.surface_rect.y],
            window: None,
        };
        self.renderer.frame_stats.record_isolated_layer_render(
            plan.width,
            plan.height,
            child.node_id,
            plan.surface_logical,
        );
        let mark = self.renderer.stand_in_mark();
        self.render_layer(
            &child.content,
            child_page,
            plan.surface_scale,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            beneath,
        )?;
        let pixels = self.pixels_since(mark);
        let retained = retain.filter(|key| {
            self.retain_source(
                child.node_id,
                *key,
                Retained::surface(Rc::clone(&texture), pixels),
                None,
            )
        });
        Ok(plan.surface(
            CompositeSource {
                texture,
                content: retained.map_or(SourceContent::Transient, |key| {
                    SourceContent::retained(&key)
                }),
            },
            None,
            pixels,
        ))
    }

    /// The child's retained surface under `key`. A surface kept in place in
    /// an atlas serves only a child that composites it flat: an effect
    /// reads its source texture whole.
    pub(super) fn cached_source(
        &mut self,
        child: &ChildLayer,
        key: LayerRasterCacheKey,
        width: u32,
        height: u32,
    ) -> Option<Retained> {
        let node_id = child.node_id;
        let retained = self
            .renderer
            .layer_cache
            .get(&key)
            .filter(|retained| retained.region.is_none() || renders_flat(child))?;
        self.renderer
            .frame_stats
            .record_layer_cache_hit(&key, width, height);
        self.supersede_source(node_id, key);
        if let Some(gate) = self.source_gate(node_id) {
            gate.hit(key);
        }
        Some(retained)
    }

    /// Keeps `retained` under `key`; `transient` names the pool texture a
    /// surface kept in place in a frame's atlas returns to.
    pub(super) fn retain_source(
        &mut self,
        node_id: Option<NodeId>,
        key: LayerRasterCacheKey,
        retained: Retained,
        transient: Option<FrameTextureDescriptor>,
    ) -> bool {
        let (width, height) = match retained.region {
            Some(region) => (region.width as u32, region.height as u32),
            None => (retained.texture.width, retained.texture.height),
        };
        self.renderer
            .frame_stats
            .record_layer_cache_miss(&key, width, height);
        let inserted = self.renderer.layer_cache.insert(key, retained, transient);
        if inserted && let Some(gate) = self.source_gate(node_id) {
            gate.admitted();
        }
        inserted
    }

    pub(super) fn source_gate(&mut self, node_id: Option<NodeId>) -> Option<&mut AdmissionGate> {
        node_id.and_then(|node_id| self.renderer.gates.node(node_id, NodeGate::Source))
    }

    pub(super) fn supersede_source(&mut self, node_id: Option<NodeId>, key: LayerRasterCacheKey) {
        let Some(previous) = self.source_gate(node_id).map(|gate| gate.key()) else {
            return;
        };
        if previous.draws_other_content(key) {
            self.renderer.layer_cache.remove(&previous);
        }
    }

    pub(super) fn admits_source(
        &mut self,
        node_id: Option<NodeId>,
        key: LayerRasterCacheKey,
        (width, height): (u32, u32),
        gate: fn(LayerRasterCacheKey) -> AdmissionGate,
    ) -> bool {
        self.supersede_source(node_id, key);
        let admits = match node_id {
            None => true,
            Some(node_id) => self.renderer.gates.observe_node(
                (node_id, NodeGate::Source),
                key,
                gate,
                &mut self.renderer.layer_cache,
            ),
        };
        admits && self.renderer.layer_cache.fits(width, height)
    }
}
