use std::rc::Weak;

use cranpose_render_common::{graph::RenderGraph, software_text_raster::SoftwareTextFontSet};
use web_time::Instant;

use crate::{
    TextSystemState,
    collect::{collect_overlay, collect_root},
    frame_packet::{FramePacket, RenderReturns},
    render::{instant_ms, should_log_wgpu_render_stage},
    scene::{Scene, SceneCapacityHint},
};

#[derive(Clone, Debug)]
pub(crate) struct DevOverlayCache {
    pub(crate) text: String,
    pub(crate) viewport_width_bits: u32,
    pub(crate) viewport_height_bits: u32,
}

/// The producer side of a frame: owns the scene graph and the text layout
/// state, and collects each frame into the packet the present stage draws.
pub(crate) struct RendererFrontend {
    pub(crate) scene: Scene,
    pub(crate) text_state: TextSystemState,
    pub(crate) text_fonts: SoftwareTextFontSet,
    pub(crate) app_context: Option<Weak<cranpose_ui::AppContext>>,
    pub(crate) root_scale: f32,
    pub(crate) dev_overlay_cache: Option<DevOverlayCache>,
    pub(crate) dev_overlay_graph: Option<RenderGraph>,
    pub(crate) root_scene_capacity: SceneCapacityHint,
    pub(crate) frame_sequence: u64,
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
            root_scene_capacity: SceneCapacityHint::default(),
            frame_sequence: 0,
            changed_nodes: Vec::new(),
        }
    }

    pub(crate) fn build_frame_packet(
        &mut self,
        width: u32,
        height: u32,
        renderer_epoch: u64,
        surface_epoch: u64,
    ) -> Option<FramePacket> {
        self.build_frame_packet_with_scale(
            width,
            height,
            self.root_scale,
            renderer_epoch,
            surface_epoch,
        )
    }

    pub(crate) fn build_frame_packet_with_scale(
        &mut self,
        width: u32,
        height: u32,
        root_scale: f32,
        renderer_epoch: u64,
        surface_epoch: u64,
    ) -> Option<FramePacket> {
        self.scene.graph.as_ref()?;
        let app_context = self.app_context.as_ref().and_then(Weak::upgrade);
        match app_context {
            Some(app_context) => app_context.enter(|| {
                self.build_frame_packet_inner(
                    width,
                    height,
                    root_scale,
                    renderer_epoch,
                    surface_epoch,
                )
            }),
            None => self.build_frame_packet_inner(
                width,
                height,
                root_scale,
                renderer_epoch,
                surface_epoch,
            ),
        }
    }

    fn build_frame_packet_inner(
        &mut self,
        width: u32,
        height: u32,
        root_scale: f32,
        renderer_epoch: u64,
        surface_epoch: u64,
    ) -> Option<FramePacket> {
        let build_start = Instant::now();
        let graph = self.scene.graph.as_ref()?;
        let root = collect_root(&graph.root, &mut self.text_state, self.root_scene_capacity);
        self.root_scene_capacity = root.scene.capacity_hint();
        let after_root_collect = Instant::now();
        let overlay = self
            .dev_overlay_graph
            .as_ref()
            .map(|overlay| collect_overlay(&overlay.root, &mut self.text_state));
        self.frame_sequence = self.frame_sequence.wrapping_add(1);
        let packet = FramePacket {
            frame_id: self.frame_sequence,
            viewport: (width, height),
            renderer_epoch,
            surface_epoch,
            root_scale,
            root,
            overlay,
            text_cache_len: self.text_state.text_cache_len(),
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

    /// Folds the present stage's returns back into the producer: the
    /// rendered root scene's capacities seed the next collect.
    pub(crate) fn apply_returns(&mut self, returns: RenderReturns) {
        if let Some(scene) = returns.scene {
            self.root_scene_capacity = scene.capacity_hint();
        }
    }
}

#[cfg(test)]
mod tests {
    use cranpose_render_common::graph::ProjectiveTransform;
    use cranpose_ui_graphics::{GraphicsLayer, Rect};

    use super::*;
    use crate::{WgpuTextSystem, test_support::layer_node};

    fn frontend() -> RendererFrontend {
        let text_system = WgpuTextSystem::from_fonts(&[]);
        RendererFrontend::new(text_system.render_state(), text_system.software_fonts())
    }

    fn graph() -> RenderGraph {
        RenderGraph::new(layer_node(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 240.0,
                height: 160.0,
            },
            ProjectiveTransform::default(),
            GraphicsLayer::default(),
            vec![],
        ))
    }

    #[test]
    fn build_without_graph_produces_no_packet() {
        let mut frontend = frontend();
        assert!(frontend.build_frame_packet(320, 240, 0, 0).is_none());
        assert_eq!(frontend.frame_sequence, 0);
    }

    #[test]
    fn root_builds_packet_with_frame_scalars() {
        let mut frontend = frontend();
        frontend.scene.graph = Some(graph());
        frontend.root_scale = 2.0;

        let packet = frontend
            .build_frame_packet(320, 240, 0, 0)
            .expect("a graph collects into a packet");
        assert_eq!(packet.frame_id, 1);
        assert_eq!(packet.viewport, (320, 240));
        assert_eq!(packet.root_scale, 2.0);
        assert!(packet.root.children.is_empty());
        assert!(packet.overlay.is_none());

        let next = frontend
            .build_frame_packet_with_scale(320, 240, 1.0, 0, 0)
            .expect("second frame collects too");
        assert_eq!(next.frame_id, 2, "frame sequence must be monotone");
        assert_eq!(next.root_scale, 1.0, "explicit scale must win");
    }

    #[test]
    fn dev_overlay_collects_into_packet() {
        let mut frontend = frontend();
        frontend.scene.graph = Some(graph());
        frontend.dev_overlay_graph = Some(graph());

        let packet = frontend
            .build_frame_packet(320, 240, 0, 0)
            .expect("a graph collects into a packet");
        let overlay = packet
            .overlay
            .as_ref()
            .expect("the dev overlay is collected producer-side");
        assert!(overlay.children.is_empty());
    }

    #[test]
    fn apply_returns_seeds_the_next_collect_capacity() {
        let mut frontend = frontend();
        frontend.scene.graph = Some(graph());
        let packet = frontend
            .build_frame_packet(320, 240, 0, 0)
            .expect("a graph collects into a packet");
        let mut scene = packet.root.scene;
        scene.shapes.reserve(64);
        let hint = scene.capacity_hint();
        frontend.apply_returns(RenderReturns {
            scene: Some(scene),
            ..RenderReturns::default()
        });
        assert_eq!(frontend.root_scene_capacity, hint);
    }
}
