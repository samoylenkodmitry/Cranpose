//! Producer frontend of the renderer (pipeline step 6a).
//!
//! [`RendererFrontend`] owns everything the producer stage needs to lower a
//! frame into a [`FramePacket`]: the retained scene graph, the text layout
//! state, the producer-side lowering memos, and the direct-scene recycling
//! pool. [`WgpuRenderer`](crate::WgpuRenderer) drives it as
//! `build_frame_packet -> GpuRenderer::render(packet) -> apply_returns`, so
//! the direct-root path lowers entirely before the present backend runs; the
//! non-direct graph fallback and the dev overlay still lower backend-side
//! (packetized in a later step).

use crate::frame_packet::{FramePacket, RenderReturns, ReplayConfirmation};
use crate::normalized_scene::collect_layer_contents_reusing;
use crate::render::{
    direct_root_child_underlays_are_supported, instant_ms, should_log_wgpu_render_stage,
    RETAINED_LAYER_REQUIREMENTS_CAPACITY,
};
use crate::scene::{CompositorScene, Scene, SceneCapacityHint};
use crate::surface_executor::root_direct_scene_events_are_supported;
use crate::surface_plan::{
    root_can_render_directly_cached, LayerSurfaceRequirements, TranslationRenderContext,
};
use crate::TextSystemState;
use cranpose_core::collections::map::HashMap;
use cranpose_render_common::graph::RenderGraph;
use cranpose_render_common::software_text_raster::SoftwareTextFontSet;
use cranpose_ui_graphics::Rect;
use std::rc::Weak;
use web_time::Instant;

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
    pub(crate) retained_direct_scene: Option<CompositorScene>,
    pub(crate) direct_scene_capacity: SceneCapacityHint,
    /// Monotone id stamped on each [`FramePacket`] this producer publishes.
    pub(crate) frame_sequence: u64,
    /// Producer-side lowering memos, cleared per frame. The present backend
    /// keeps its own pair for the graph fallback and overlay paths; both
    /// memoize the same deterministic functions, so the two instances cannot
    /// diverge in value.
    pub(crate) layer_surface_rect_cache: HashMap<usize, Rect>,
    pub(crate) layer_surface_requirements_cache: HashMap<usize, LayerSurfaceRequirements>,
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
            retained_direct_scene: None,
            direct_scene_capacity: SceneCapacityHint::default(),
            frame_sequence: 0,
            layer_surface_rect_cache: HashMap::new(),
            layer_surface_requirements_cache: HashMap::new(),
        }
    }

    /// Lower the current scene graph's direct root into a [`FramePacket`]
    /// using the frontend's configured root scale.
    ///
    /// `None` means the present backend must take the non-direct graph
    /// fallback path (or there is no graph at all).
    pub(crate) fn build_frame_packet(
        &mut self,
        width: u32,
        height: u32,
        replay_supported: bool,
    ) -> Option<FramePacket> {
        self.build_frame_packet_with_scale(width, height, self.root_scale, replay_supported)
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
    ) -> Option<FramePacket> {
        self.scene.graph.as_ref()?;
        // Lowering must run under the app context: text layout branches on
        // the `has_current_app_context` thread-local (see
        // `TextLayoutResolver for TextSystemState`).
        let app_context = self.app_context.as_ref().and_then(Weak::upgrade);
        let packet = if let Some(app_context) = app_context {
            app_context.enter(|| {
                self.build_frame_packet_inner(width, height, root_scale, replay_supported)
            })
        } else {
            self.build_frame_packet_inner(width, height, root_scale, replay_supported)
        };
        // Per-frame memo hygiene, mirroring the present backend's
        // end-of-render clears: the memos key on node addresses, so they
        // must never survive into a frame with a different graph, and the
        // requirements map must not keep a worst-case frame's capacity.
        self.layer_surface_rect_cache.clear();
        self.layer_surface_requirements_cache.clear();
        if self.layer_surface_requirements_cache.capacity() > RETAINED_LAYER_REQUIREMENTS_CAPACITY {
            self.layer_surface_requirements_cache
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
    ) -> Option<FramePacket> {
        #[cfg(target_arch = "wasm32")]
        let _ = replay_supported;
        let build_start = Instant::now();
        self.layer_surface_rect_cache.clear();
        self.layer_surface_requirements_cache.clear();
        let graph = self.scene.graph.as_ref()?;
        let direct_root = if root_can_render_directly_cached(
            &graph.root,
            &mut self.layer_surface_requirements_cache,
        ) {
            let recycled_scene = self
                .retained_direct_scene
                .take()
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
            if root_direct_scene_events_are_supported(&collected.scene)
                && direct_root_child_underlays_are_supported(&collected)
            {
                Some(collected)
            } else {
                // The collected scene will not render this frame, so any
                // capture requests recorded against its shape indices are
                // void; the replay state re-bootstraps on the next direct
                // frame.
                #[cfg(not(target_arch = "wasm32"))]
                crate::shape_replay::SHAPE_REPLAY.with(|state| state.borrow_mut().retire_all());
                self.retained_direct_scene = Some(collected.scene);
                None
            }
        } else {
            None
        };
        let after_root_collect = Instant::now();
        // The producer's complete output for this frame, owned and Send.
        // Built before the present backend runs so a later step can publish
        // it to a present thread; consumed synchronously today. Building
        // the packet closes the frame's replay window: the planner emits
        // its plan into `replay` and stops accepting retained ops.
        let direct_packet = direct_root.map(|root| {
            self.frame_sequence = self.frame_sequence.wrapping_add(1);
            #[cfg(not(target_arch = "wasm32"))]
            let replay = crate::shape_replay::SHAPE_REPLAY.with(|state| {
                state
                    .borrow_mut()
                    .take_frame_ops(crate::pipeline::retained_feed_generation())
            });
            #[cfg(target_arch = "wasm32")]
            let replay = crate::frame_packet::ReplayFrameOps::default();
            FramePacket {
                frame_id: self.frame_sequence,
                viewport: (width, height),
                root_scale,
                root,
                replay,
            }
        });
        let after_build = Instant::now();
        if let Some(total_ms) = should_log_wgpu_render_stage(build_start, after_build) {
            log::warn!(
                "[wgpu-render-stage:frontend-collect] total_ms={total_ms:.2} collect_ms={:.2}",
                instant_ms(build_start, after_root_collect),
            );
        }
        direct_packet
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
        let RenderReturns { scene, ack } = returns;
        if let Some(scene) = scene {
            // The packet's scene buffers return to the producer pool — for
            // a heavy animated frame they are megabytes of Vec.
            self.retained_direct_scene = Some(scene);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            ack.map(|(ack, recycled)| {
                crate::shape_replay::SHAPE_REPLAY
                    .with(|state| state.borrow_mut().apply_ack(ack, recycled))
            })
        }
        #[cfg(target_arch = "wasm32")]
        {
            // wasm has no retained replay path; a present stage there never
            // produces an ack.
            let _ = ack;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::layer_node;
    use crate::WgpuTextSystem;
    use cranpose_render_common::graph::{ProjectiveTransform, RenderNode};
    use cranpose_ui_graphics::GraphicsLayer;

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

    #[test]
    fn build_without_graph_produces_no_packet() {
        let mut frontend = frontend();
        assert!(frontend.build_frame_packet(320, 240, false).is_none());
        assert_eq!(frontend.frame_sequence, 0);
    }

    #[test]
    fn direct_root_builds_packet_with_frame_scalars() {
        let mut frontend = frontend();
        frontend.scene.graph = Some(direct_graph());
        frontend.root_scale = 2.0;

        let packet = frontend
            .build_frame_packet(320, 240, false)
            .expect("direct root must lower into a packet");

        assert_eq!(packet.frame_id, 1);
        assert_eq!(packet.viewport, (320, 240));
        assert_eq!(packet.root_scale, 2.0);
        assert!(packet.root.child_layers.is_empty());

        let next = frontend
            .build_frame_packet_with_scale(320, 240, 1.0, false)
            .expect("second frame should lower too");
        assert_eq!(next.frame_id, 2, "frame sequence must be monotone");
        assert_eq!(next.root_scale, 1.0, "explicit scale must win");
    }

    #[test]
    fn non_direct_root_produces_no_packet() {
        let mut frontend = frontend();
        let mut root = layer_node(
            bounds(240.0, 160.0),
            ProjectiveTransform::default(),
            GraphicsLayer::default(),
            vec![],
        );
        root.graphics_layer.shadow_elevation = 4.0;
        frontend.scene.graph = Some(RenderGraph::new(root));

        assert!(frontend.build_frame_packet(320, 240, false).is_none());
        assert_eq!(
            frontend.frame_sequence, 0,
            "a fallback frame must not consume a packet id"
        );
        assert!(
            frontend.retained_direct_scene.is_none(),
            "the non-direct gate rejects before any scene is collected"
        );
    }

    #[test]
    fn rejected_collect_stashes_scene_and_produces_no_packet() {
        // A descendant backdrop passes the direct-eligibility gate but the
        // collected scene carries a root-local backdrop event, which the
        // direct path cannot render — the reject branch must recycle the
        // collected scene.
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

        assert!(frontend.build_frame_packet(320, 240, false).is_none());
        assert!(
            frontend.retained_direct_scene.is_some(),
            "rejected collect must stash the scene for recycling"
        );
        assert_eq!(frontend.frame_sequence, 0);
    }

    #[test]
    fn apply_returns_recycles_scene_into_next_build() {
        let mut frontend = frontend();
        frontend.scene.graph = Some(direct_graph());

        let packet = frontend
            .build_frame_packet(320, 240, false)
            .expect("direct root must lower into a packet");
        assert!(
            frontend.retained_direct_scene.is_none(),
            "the packet owns the scene while the present stage renders it"
        );

        let confirmations = frontend.apply_returns(RenderReturns {
            scene: Some(packet.root.scene),
            ack: None,
        });
        assert!(confirmations.is_none(), "no ack means nothing to recycle");
        assert!(
            frontend.retained_direct_scene.is_some(),
            "apply_returns must stash the scene for the next build"
        );

        let next = frontend
            .build_frame_packet(320, 240, false)
            .expect("recycled scene must feed the next frame");
        assert_eq!(next.frame_id, 2);
        assert!(
            frontend.retained_direct_scene.is_none(),
            "the next build must take the recycled scene"
        );
    }
}
