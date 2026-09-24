use std::rc::Weak;

use cranpose_render_common::{graph::RenderGraph, software_text_raster::SoftwareTextFontSet};
use web_time::Instant;

use crate::{
    TextSystemState,
    collect::{collect_overlay, collect_root},
    frame_packet::{FramePacket, RenderReturns},
    render::{frame_clear_color, instant_ms, should_log_wgpu_render_stage},
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
    pub(crate) fps_overlay_graph: Option<RenderGraph>,
    pub(crate) inspector_overlay_graph: Option<RenderGraph>,
    pub(crate) root_scene_capacity: SceneCapacityHint,
    pub(crate) frame_sequence: u64,
    pub(crate) changed_nodes: Vec<cranpose_core::NodeId>,
    pub(crate) shader_warm_ups: Vec<cranpose_ui_graphics::ShaderWarmUp>,
    pub(crate) transparent_background: bool,
}

impl RendererFrontend {
    pub(crate) fn refresh_dev_overlay(&mut self) {
        self.dev_overlay_graph = self.fps_overlay_graph.clone();
        if let Some(inspector) = &self.inspector_overlay_graph {
            let graph = self.dev_overlay_graph.get_or_insert_with(|| {
                RenderGraph::new(cranpose_render_common::graph::LayerNode::default())
            });
            graph
                .root
                .children
                .push(cranpose_render_common::graph::RenderNode::Layer(Box::new(
                    inspector.root.clone(),
                )));
            graph.root.recompute_raster_cache_hashes();
        }
    }

    pub(crate) fn clear_fps_overlay(&mut self) {
        self.fps_overlay_graph = None;
        self.dev_overlay_cache = None;
        self.refresh_dev_overlay();
    }
    pub(crate) fn new(text_state: TextSystemState, text_fonts: SoftwareTextFontSet) -> Self {
        Self {
            scene: Scene::new(),
            text_state,
            text_fonts,
            app_context: None,
            root_scale: 1.0,
            dev_overlay_cache: None,
            dev_overlay_graph: None,
            fps_overlay_graph: None,
            inspector_overlay_graph: None,
            root_scene_capacity: SceneCapacityHint::default(),
            frame_sequence: 0,
            changed_nodes: Vec::new(),
            shader_warm_ups: Vec::new(),
            transparent_background: false,
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
            clear: frame_clear_color(self.transparent_background),
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
#[path = "tests/frontend_tests.rs"]
mod tests;
