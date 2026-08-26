//! Producer frontend of the renderer (pipeline steps 6a/6b).
//!
//! [`RendererFrontend`] owns everything the producer stage needs to lower a
//! frame into a [`FramePacket`]: the retained scene graph, the text layout
//! state, the producer-side lowering memos, and the direct-scene recycling
//! pool. [`WgpuRenderer`](crate::WgpuRenderer) drives it as
//! `build_frame_packet -> GpuRenderer::render(packet) -> apply_returns`.
//! Every frame lowers entirely before the present backend runs: direct
//! roots, non-direct root surfaces, and the dev overlay all become packet
//! payloads here — the present backend never touches the retained graph.

use std::rc::Weak;

use cranpose_core::collections::map::HashMap;
use cranpose_render_common::{
    graph::{LayerNode, RenderGraph},
    software_text_raster::SoftwareTextFontSet,
};
use cranpose_ui_graphics::Rect;
use web_time::Instant;

use crate::{
    TextSystemState,
    frame_packet::{
        FramePacket, PacketRoot, RenderReturns, ReplayConfirmation, ReplayFrameOps,
        RootSurfacePacket,
    },
    normalized_scene::{
        collect_layer_contents_reusing,
        collect_layer_contents_with_translation_context_and_text_layout, lower_layer_node,
    },
    render::{
        RETAINED_LAYER_REQUIREMENTS_CAPACITY, direct_root_child_underlays_are_supported,
        instant_ms, should_log_wgpu_render_stage,
    },
    scene::{CompositorScene, Scene, SceneCapacityHint},
    surface_executor::{layer_surface_translation_context, root_direct_scene_events_are_supported},
    surface_plan::{
        LayerSurfaceRequirements, TranslationRenderContext, effective_surface_requirements,
        layer_surface_requirements_cached, root_can_render_directly_cached,
    },
    surface_requirements::SurfaceRequirement,
};

/// Direct scenes kept for recycling: one per packet that can be in flight
/// (depth-one publishing allows one rendering and one waiting) plus the one
/// being filled. Beyond that the buffers are dropped rather than hoarded.
const MAX_RETAINED_DIRECT_SCENES: usize = 3;

#[derive(Clone, Debug)]
pub(crate) struct DevOverlayCache {
    pub(crate) text: String,
    pub(crate) viewport_width_bits: u32,
    pub(crate) viewport_height_bits: u32,
}

/// The producer half of [`WgpuRenderer`](crate::WgpuRenderer): scene inputs,
/// text layout, and the direct-root lowering stage that publishes
/// [`FramePacket`]s for the present backend to consume.
pub(crate) struct RendererFrontend {
    pub(crate) scene: Scene,
    pub(crate) text_state: TextSystemState,
    pub(crate) text_fonts: SoftwareTextFontSet,
    pub(crate) app_context: Option<Weak<cranpose_ui::AppContext>>,
    /// Root scale factor for text rendering (use for density scaling)
    pub(crate) root_scale: f32,
    pub(crate) dev_overlay_cache: Option<DevOverlayCache>,
    pub(crate) dev_overlay_graph: Option<RenderGraph>,
    /// Last frame's direct-root scene, kept so its (potentially multi-MB)
    /// draw vectors are reused instead of reallocated every frame. Returned
    /// by the present stage through [`RenderReturns`].
    /// Recycled direct-root scenes. Depth-one publishing keeps up to two
    /// packets in flight (one rendering, one waiting), so ONE spare scene
    /// would leave the producer allocating a fresh multi-megabyte
    /// `CompositorScene` on every other frame — the mmap churn the
    /// recycling exists to prevent. The pool holds one scene per in-flight
    /// packet plus the one being filled.
    pub(crate) retained_direct_scenes: Vec<CompositorScene>,
    pub(crate) direct_scene_capacity: SceneCapacityHint,
    /// Monotone id stamped on each [`FramePacket`] this producer publishes.
    pub(crate) frame_sequence: u64,
    /// The renderer's only lowering-memo pair (the present backend lowers
    /// nothing since step 6b), cleared per frame — the memos key on node
    /// addresses and must never survive into a frame with another graph.
    pub(crate) layer_surface_rect_cache: HashMap<usize, Rect>,
    pub(crate) layer_surface_requirements_cache: HashMap<usize, LayerSurfaceRequirements>,
    pub(crate) overlay_surface_requirements_cache: HashMap<usize, LayerSurfaceRequirements>,
    pub(crate) changed_nodes: Vec<cranpose_core::NodeId>,
}

impl RendererFrontend {
    pub(crate) fn new(text_state: TextSystemState, text_fonts: SoftwareTextFontSet) -> Self {
        Self {
            scene: Scene::new(),
            text_state,
            text_fonts,
            app_context: None,
            root_scale: 1.0,
            dev_overlay_cache: None,
            dev_overlay_graph: None,
            retained_direct_scenes: Vec::new(),
            direct_scene_capacity: SceneCapacityHint::default(),
            frame_sequence: 0,
            layer_surface_rect_cache: HashMap::new(),
            layer_surface_requirements_cache: HashMap::new(),
            overlay_surface_requirements_cache: HashMap::new(),
            changed_nodes: Vec::new(),
        }
    }

    /// Lower the current scene graph into a [`FramePacket`] using the
    /// frontend's configured root scale.
    ///
    /// `None` means there is no graph at all; every frame with a graph
    /// becomes a packet (direct or root-surface).
    pub(crate) fn build_frame_packet(
        &mut self,
        width: u32,
        height: u32,
        replay_supported: bool,
        renderer_epoch: u64,
        surface_epoch: u64,
    ) -> Option<FramePacket> {
        self.build_frame_packet_with_scale(
            width,
            height,
            self.root_scale,
            replay_supported,
            renderer_epoch,
            surface_epoch,
        )
    }

    /// [`build_frame_packet`](Self::build_frame_packet) with an explicit
    /// root scale — the screenshot capture path lowers at a caller-chosen
    /// scale instead of the configured one.
    pub(crate) fn build_frame_packet_with_scale(
        &mut self,
        width: u32,
        height: u32,
        root_scale: f32,
        replay_supported: bool,
        renderer_epoch: u64,
        surface_epoch: u64,
    ) -> Option<FramePacket> {
        self.scene.graph.as_ref()?;
        // Lowering must run under the app context: text layout branches on
        // the `has_current_app_context` thread-local (see
        // `TextLayoutResolver for TextSystemState`).
        let app_context = self.app_context.as_ref().and_then(Weak::upgrade);
        let packet = if let Some(app_context) = app_context {
            app_context.enter(|| {
                self.build_frame_packet_inner(
                    width,
                    height,
                    root_scale,
                    replay_supported,
                    renderer_epoch,
                    surface_epoch,
                )
            })
        } else {
            self.build_frame_packet_inner(
                width,
                height,
                root_scale,
                replay_supported,
                renderer_epoch,
                surface_epoch,
            )
        };
        // Per-frame memo hygiene, mirroring the present backend's
        // end-of-render clears: the rect memo keys on node addresses, so it
        // must never survive into a frame with a different graph. The
        // requirements memo keys on layout node ids and is dropped per node
        // by the scene patch instead.
        self.layer_surface_rect_cache.clear();
        self.overlay_surface_requirements_cache.clear();
        if self.overlay_surface_requirements_cache.capacity() > RETAINED_LAYER_REQUIREMENTS_CAPACITY
        {
            self.overlay_surface_requirements_cache
                .shrink_to(RETAINED_LAYER_REQUIREMENTS_CAPACITY);
        }
        packet
    }

    fn build_frame_packet_inner(
        &mut self,
        width: u32,
        height: u32,
        root_scale: f32,
        replay_supported: bool,
        renderer_epoch: u64,
        surface_epoch: u64,
    ) -> Option<FramePacket> {
        #[cfg(target_arch = "wasm32")]
        let _ = replay_supported;
        let build_start = Instant::now();
        self.layer_surface_rect_cache.clear();
        let graph = self.scene.graph.as_ref()?;
        let direct_root = if root_can_render_directly_cached(
            &graph.root,
            &mut self.layer_surface_requirements_cache,
        ) {
            let recycled_scene = self
                .retained_direct_scenes
                .pop()
                .unwrap_or_else(|| CompositorScene::with_capacity(self.direct_scene_capacity));
            #[cfg(not(target_arch = "wasm32"))]
            crate::shape_replay::SHAPE_REPLAY
                .with(|state| state.borrow_mut().begin_frame(replay_supported, root_scale));
            let collected = collect_layer_contents_reusing(
                &graph.root,
                &mut self.text_state,
                None,
                None,
                TranslationRenderContext::default(),
                // The direct-root path renders each collected child with the
                // default request context (see `render_root_direct`), so the
                // child sources must be collected under it as-is rather than
                // the surface-derived context.
                TranslationRenderContext::default(),
                &mut self.layer_surface_rect_cache,
                &mut self.layer_surface_requirements_cache,
                recycled_scene,
            );
            self.direct_scene_capacity = collected.scene.capacity_hint();
            if root_direct_scene_events_are_supported(&collected.scene, true)
                && direct_root_child_underlays_are_supported(&collected, true)
            {
                Some(collected)
            } else {
                // The collected scene will not render this frame, so any
                // capture requests recorded against its shape indices are
                // void; the replay state re-bootstraps on the next direct
                // frame.
                #[cfg(not(target_arch = "wasm32"))]
                crate::shape_replay::SHAPE_REPLAY.with(|state| state.borrow_mut().retire_all());
                if self.retained_direct_scenes.len() < MAX_RETAINED_DIRECT_SCENES {
                    self.retained_direct_scenes.push(collected.scene);
                }
                None
            }
        } else {
            None
        };
        // The producer's complete output for this frame, owned and Send.
        // Built before the present backend runs so a later step can publish
        // it to a present thread; consumed synchronously today. Building a
        // direct packet closes the frame's replay window: the planner emits
        // its plan into `replay` and stops accepting retained ops.
        let (root, replay) = match direct_root {
            Some(root) => {
                #[cfg(not(target_arch = "wasm32"))]
                let replay = crate::shape_replay::SHAPE_REPLAY.with(|state| {
                    state
                        .borrow_mut()
                        .take_frame_ops(crate::pipeline::retained_feed_generation())
                });
                #[cfg(target_arch = "wasm32")]
                let replay = ReplayFrameOps;
                (PacketRoot::Direct(Box::new(root)), replay)
            }
            None => {
                // A non-direct-eligible root and a rejected direct collect
                // both used to render through the present backend's graph
                // fallback; both now lower into a Surface packet here. A
                // Surface frame never touches the planner: its pending
                // queues simply wait for the next direct frame, as they
                // always did, and the packet carries the empty default
                // replay plan (generation 0), which the present store must
                // not consume.
                let surface = lower_root_surface_packet(
                    &graph.root,
                    &mut self.text_state,
                    &mut self.layer_surface_rect_cache,
                    &mut self.layer_surface_requirements_cache,
                    width,
                    height,
                    root_scale,
                );
                #[cfg(not(target_arch = "wasm32"))]
                let replay = ReplayFrameOps::default();
                #[cfg(target_arch = "wasm32")]
                let replay = ReplayFrameOps;
                (PacketRoot::Surface(Box::new(surface)), replay)
            }
        };
        let after_root_collect = Instant::now();
        let overlay = if let Some(overlay_graph) = self.dev_overlay_graph.as_ref() {
            // The dev overlay is a different graph: the rect memo keys on node
            // addresses, so the root graph's entries must not leak into the
            // overlay collect (mirrors the present backend's old per-path
            // clears), and the overlay plans into a map of its own.
            self.layer_surface_rect_cache.clear();
            self.overlay_surface_requirements_cache.clear();
            Some(
                collect_layer_contents_with_translation_context_and_text_layout(
                    &overlay_graph.root,
                    &mut self.text_state,
                    None,
                    None,
                    TranslationRenderContext::default(),
                    &mut self.layer_surface_rect_cache,
                    &mut self.overlay_surface_requirements_cache,
                ),
            )
        } else {
            None
        };
        self.frame_sequence = self.frame_sequence.wrapping_add(1);
        let packet = FramePacket {
            frame_id: self.frame_sequence,
            viewport: (width, height),
            renderer_epoch,
            surface_epoch,
            root_scale,
            root,
            overlay,
            replay,
            text_cache_len: self.text_state.text_cache_len(),
            recycled_confirmations: None,
            replay_preconsumed: false,
        };
        let after_build = Instant::now();
        if let Some(total_ms) = should_log_wgpu_render_stage(build_start, after_build) {
            log::warn!(
                "[wgpu-render-stage:frontend-collect] total_ms={total_ms:.2} collect_ms={:.2}",
                instant_ms(build_start, after_root_collect),
            );
        }
        Some(packet)
    }

    /// Fold the present stage's [`RenderReturns`] back into producer state:
    /// stash the rendered packet's scene buffers for recycling and apply the
    /// replay ack to the planner. Returns the planner-drained confirmations
    /// buffer (capacity intact) for the caller to hand back to the store.
    ///
    /// Applying the ack after render is equivalent to the store-side
    /// application it replaces: both points sit after this frame's graph
    /// build and before the next collect, which is where the bypass gate
    /// and `feed_slots` are read.
    pub(crate) fn apply_returns(
        &mut self,
        returns: RenderReturns,
    ) -> Option<Vec<ReplayConfirmation>> {
        let RenderReturns {
            scene,
            ack,
            frame_id: _,
            outcome: _,
            cancelled_replay,
            #[cfg(not(target_arch = "wasm32"))]
                timings: _,
        } = returns;
        if let Some(scene) = scene {
            // The packet's scene buffers return to the producer pool — for
            // a heavy animated frame they are megabytes of Vec. A cancelled
            // packet returns its scene the same way as a presented one.
            if self.retained_direct_scenes.len() < MAX_RETAINED_DIRECT_SCENES {
                self.retained_direct_scenes.push(scene);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            // A cancelled packet's replay plan never reached the store:
            // re-queue its releases (slot ids must not leak — the pool is
            // finite), purge its unconfirmable awaiting entries, and
            // recycle its buffers.
            if let Some(ops) = cancelled_replay {
                crate::shape_replay::SHAPE_REPLAY
                    .with(|state| state.borrow_mut().reclaim_cancelled_ops(ops));
            }
            ack.map(|(ack, recycled)| {
                crate::shape_replay::SHAPE_REPLAY
                    .with(|state| state.borrow_mut().apply_ack(ack, recycled))
            })
        }
        #[cfg(target_arch = "wasm32")]
        {
            // wasm has no retained replay path; a present stage there never
            // produces an ack or a cancelled replay plan.
            let _ = ack;
            let _ = cancelled_replay;
            None
        }
    }
}

/// Lowers a non-direct root into a [`RootSurfacePacket`]: the producer-side
/// equivalent of the present backend's old `render_root_layer_surface`
/// prologue. The request context matches the old graph-fallback call exactly
/// (`allow_runtime_cache=false`, `logical_rect_override=Some(viewport_rect)`,
/// `activates_nested_capture=false`, default translation context), and the
/// derivation mirrors its shadowing of the translation context BEFORE the
/// collect; `target_scale` and the composite sample mode are recomputed
/// present-side by `render_layer_surface` from the same snapshot.
fn lower_root_surface_packet(
    root: &LayerNode,
    text_state: &mut TextSystemState,
    layer_surface_rect_cache: &mut HashMap<usize, Rect>,
    layer_surface_requirements_cache: &mut HashMap<usize, LayerSurfaceRequirements>,
    width: u32,
    height: u32,
    root_scale: f32,
) -> RootSurfacePacket {
    // The root layer's visible area is always the viewport — content
    // outside the screen is invisible regardless of scroll offsets or
    // inflated scene bounds.  Pass the viewport rect as an explicit
    // surface rect to prevent offscreen inflation on constrained GPUs.
    let viewport_rect = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32 / root_scale,
        height: height as f32 / root_scale,
    };
    let translation_context = TranslationRenderContext::default();
    let activates_nested_capture = false;
    let surface_requirements =
        layer_surface_requirements_cached(root, layer_surface_requirements_cache);
    let direct_translated_content_context =
        translation_context.inherited_content_translation || root.translated_content_context;
    let effective_translated_content_context =
        direct_translated_content_context || surface_requirements.contains_translated_content;
    let effective_requirements = effective_surface_requirements(
        effective_translated_content_context,
        translation_context.surface_capture_active,
        surface_requirements,
    );
    let translation_context = layer_surface_translation_context(
        translation_context,
        activates_nested_capture
            && effective_requirements.contains(SurfaceRequirement::MotionStableCapture),
    );
    let (lowered, source) = lower_layer_node(
        root,
        text_state,
        surface_requirements,
        viewport_rect,
        translation_context,
        layer_surface_rect_cache,
        layer_surface_requirements_cache,
    );
    RootSurfacePacket {
        lowered,
        source,
        transform_to_parent: root.transform_to_parent,
        node_id: root.node_id,
        backdrop: root.backdrop().cloned(),
        graphics_layer: root.graphics_layer.clone(),
        local_bounds: root.local_bounds,
        clip_rect: root.clip_rect(),
        shadow_clip: root.shadow_clip,
    }
}

#[cfg(test)]
mod tests {
    use cranpose_render_common::graph::{ProjectiveTransform, RenderNode};
    use cranpose_ui_graphics::GraphicsLayer;

    use super::*;
    use crate::{WgpuTextSystem, test_support::layer_node};

    fn frontend() -> RendererFrontend {
        let text_system = WgpuTextSystem::from_fonts(&[]);
        RendererFrontend::new(text_system.render_state(), text_system.software_fonts())
    }

    fn bounds(width: f32, height: f32) -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            width,
            height,
        }
    }

    fn direct_graph() -> RenderGraph {
        RenderGraph::new(layer_node(
            bounds(240.0, 160.0),
            ProjectiveTransform::default(),
            GraphicsLayer::default(),
            vec![],
        ))
    }

    fn direct_root(packet: &FramePacket) -> &crate::normalized_scene::CollectedLayer {
        match &packet.root {
            PacketRoot::Direct(root) => root,
            PacketRoot::Surface(_) => panic!("expected a direct packet root"),
        }
    }

    fn surface_root(packet: &FramePacket) -> &RootSurfacePacket {
        match &packet.root {
            PacketRoot::Surface(surface) => surface,
            PacketRoot::Direct(_) => panic!("expected a surface packet root"),
        }
    }

    #[test]
    fn build_without_graph_produces_no_packet() {
        let mut frontend = frontend();
        assert!(frontend.build_frame_packet(320, 240, false, 0, 0).is_none());
        assert_eq!(frontend.frame_sequence, 0);
    }

    #[test]
    fn direct_root_builds_packet_with_frame_scalars() {
        let mut frontend = frontend();
        frontend.scene.graph = Some(direct_graph());
        frontend.root_scale = 2.0;

        let packet = frontend
            .build_frame_packet(320, 240, false, 0, 0)
            .expect("direct root must lower into a packet");

        assert_eq!(packet.frame_id, 1);
        assert_eq!(packet.viewport, (320, 240));
        assert_eq!(packet.root_scale, 2.0);
        assert!(direct_root(&packet).child_layers.is_empty());
        assert!(packet.overlay.is_none());

        let next = frontend
            .build_frame_packet_with_scale(320, 240, 1.0, false, 0, 0)
            .expect("second frame should lower too");
        assert_eq!(next.frame_id, 2, "frame sequence must be monotone");
        assert_eq!(next.root_scale, 1.0, "explicit scale must win");
    }

    #[test]
    fn non_direct_root_builds_surface_packet() {
        let mut frontend = frontend();
        let mut root = layer_node(
            bounds(240.0, 160.0),
            ProjectiveTransform::default(),
            GraphicsLayer::default(),
            vec![],
        );
        root.graphics_layer.shadow_elevation = 4.0;
        frontend.scene.graph = Some(RenderGraph::new(root));
        frontend.root_scale = 2.0;

        let packet = frontend
            .build_frame_packet(320, 240, false, 0, 0)
            .expect("non-direct root must lower into a surface packet");
        let surface = surface_root(&packet);
        assert_eq!(
            surface.lowered.logical_rect,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 160.0,
                height: 120.0,
            },
            "the root surface must be lowered against the viewport rect"
        );
        assert_eq!(surface.graphics_layer.shadow_elevation, 4.0);
        assert!(surface.backdrop.is_none());
        assert_eq!(surface.local_bounds, bounds(240.0, 160.0));
        assert_eq!(
            packet.replay.generation, 0,
            "a surface frame never touches the planner and carries the default plan"
        );
        assert_eq!(packet.frame_id, 1);
        assert!(
            frontend.retained_direct_scenes.is_empty(),
            "the non-direct gate rejects before any direct scene is collected"
        );
    }

    #[test]
    fn direct_root_with_descendant_backdrop_keeps_the_direct_packet() {
        let mut frontend = frontend();
        let mut backdrop = layer_node(
            bounds(40.0, 40.0),
            ProjectiveTransform::default(),
            GraphicsLayer::default(),
            vec![],
        );
        backdrop.graphics_layer.backdrop_effect =
            Some(cranpose_ui_graphics::RenderEffect::blur(4.0));
        let child = layer_node(
            bounds(120.0, 96.0),
            ProjectiveTransform::default(),
            GraphicsLayer::default(),
            vec![RenderNode::Layer(Box::new(backdrop))],
        );
        let root = layer_node(
            bounds(240.0, 160.0),
            ProjectiveTransform::default(),
            GraphicsLayer::default(),
            vec![RenderNode::Layer(Box::new(child))],
        );
        frontend.scene.graph = Some(RenderGraph::new(root));

        let packet = frontend
            .build_frame_packet(320, 240, false, 0, 0)
            .expect("a direct root with readable composition must produce a packet");
        let root = direct_root(&packet);
        assert!(
            root.child_layers
                .iter()
                .any(|child| child.backdrop.is_some()),
            "the direct root must retain the descendant backdrop effect"
        );
        assert!(
            frontend.retained_direct_scenes.is_empty(),
            "a readable composition root must not recycle a supported direct scene"
        );
        assert_eq!(packet.frame_id, 1);
    }

    #[test]
    fn dev_overlay_lowers_into_packet_for_direct_and_surface_roots() {
        let mut frontend = frontend();
        frontend.scene.graph = Some(direct_graph());
        frontend.dev_overlay_graph = Some(direct_graph());

        let packet = frontend
            .build_frame_packet(320, 240, false, 0, 0)
            .expect("direct root must lower into a packet");
        let overlay = packet
            .overlay
            .as_ref()
            .expect("the dev overlay must be lowered producer-side");
        assert!(
            overlay.child_layers.is_empty(),
            "the dev overlay stays directly renderable"
        );

        let mut shadowed_root = layer_node(
            bounds(240.0, 160.0),
            ProjectiveTransform::default(),
            GraphicsLayer::default(),
            vec![],
        );
        shadowed_root.graphics_layer.shadow_elevation = 4.0;
        frontend.scene.graph = Some(RenderGraph::new(shadowed_root));
        let packet = frontend
            .build_frame_packet(320, 240, false, 0, 0)
            .expect("non-direct root must lower into a surface packet");
        assert!(matches!(packet.root, PacketRoot::Surface(_)));
        assert!(
            packet.overlay.is_some(),
            "surface frames must carry the lowered dev overlay too"
        );
    }

    #[test]
    fn apply_returns_recycles_scene_into_next_build() {
        let mut frontend = frontend();
        frontend.scene.graph = Some(direct_graph());

        let packet = frontend
            .build_frame_packet(320, 240, false, 0, 0)
            .expect("direct root must lower into a packet");
        assert!(
            frontend.retained_direct_scenes.is_empty(),
            "the packet owns the scene while the present stage renders it"
        );

        let root = match packet.root {
            PacketRoot::Direct(root) => root,
            PacketRoot::Surface(_) => panic!("expected a direct packet root"),
        };
        let confirmations = frontend.apply_returns(RenderReturns {
            scene: Some(root.scene),
            ..RenderReturns::default()
        });
        assert!(confirmations.is_none(), "no ack means nothing to recycle");
        assert!(
            !frontend.retained_direct_scenes.is_empty(),
            "apply_returns must stash the scene for the next build"
        );

        let next = frontend
            .build_frame_packet(320, 240, false, 0, 0)
            .expect("recycled scene must feed the next frame");
        assert_eq!(next.frame_id, 2);
        assert!(
            frontend.retained_direct_scenes.is_empty(),
            "the next build must take the recycled scene"
        );
    }
}
