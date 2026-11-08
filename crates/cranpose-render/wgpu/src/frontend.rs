use std::rc::Weak;

use cranpose_render_common::{
    dev_overlay::DevOverlayKey,
    graph::{LayerNode, RenderNode},
    scene_store::SceneStore,
    text_fonts::TextFonts,
};
use web_time::Instant;

use crate::{
    collect::{LayerMotion, LayerSceneRecycler, collect_overlay, collect_root},
    frame_packet::{FramePacket, FrameSceneStorage, MAX_FRAMES_IN_FLIGHT, RenderReturns},
    render::{frame_clear_color, instant_ms, should_log_wgpu_render_stage},
    scene::{Scene, SceneCapacityHint},
};

/// The producer side of a frame: owns the scene graph and the fonts the app
/// lays its text out with, and collects each frame into the packet the
/// present stage draws.
pub(crate) struct RendererFrontend {
    pub(crate) scene: Scene,
    pub(crate) text_fonts: TextFonts,
    pub(crate) app_context: Option<Weak<cranpose_ui::AppContext>>,
    pub(crate) root_scale: f32,
    pub(crate) dev_overlay_cache: Option<DevOverlayKey>,
    // The overlays' records; empty while no overlay shows.
    pub(crate) dev_overlay: SceneStore,
    pub(crate) fps_overlay: Option<LayerNode>,
    pub(crate) inspector_overlay: Option<LayerNode>,
    pub(crate) root_scene_capacity: SceneCapacityHint,
    scene_recyclers: [LayerSceneRecycler; MAX_FRAMES_IN_FLIGHT],
    returned_scenes: usize,
    pub(crate) layer_motion: LayerMotion,
    pub(crate) frame_sequence: u64,
    pub(crate) transparent_background: bool,
    // Whether the app held still since the last packet; the next one says so.
    pub(crate) held_still: bool,
    // Whether a pointer is held for the next packet's frame.
    pub(crate) pointer_held: bool,
    // The packet a surface did not show yet, kept by `keep_packet` for the
    // frame drawn next.
    kept_packet: Option<FramePacket>,
}

impl RendererFrontend {
    pub(crate) fn refresh_dev_overlay(&mut self) {
        if let Some(packet) = self.kept_packet.take() {
            self.recycle(packet);
        }
        let Some(inspector) = self.inspector_overlay.take() else {
            match &self.fps_overlay {
                Some(fps) => self.dev_overlay.describe(fps),
                None => self.dev_overlay.clear(),
            }
            return;
        };
        // The inspector draws over the frame counter, as the last child of
        // its root: it moves in for the write and back out after.
        let had_fps = self.fps_overlay.is_some();
        let mut root = self.fps_overlay.take().unwrap_or_default();
        root.children.push(RenderNode::Layer(Box::new(inspector)));
        self.dev_overlay.describe(&root);
        if let Some(RenderNode::Layer(inspector)) = root.children.pop() {
            self.inspector_overlay = Some(*inspector);
        }
        self.fps_overlay = had_fps.then_some(root);
    }

    pub(crate) fn clear_fps_overlay(&mut self) {
        self.fps_overlay = None;
        self.dev_overlay_cache = None;
        self.refresh_dev_overlay();
    }
    pub(crate) fn new(text_fonts: TextFonts) -> Self {
        Self {
            scene: Scene::new(),
            text_fonts,
            app_context: None,
            root_scale: 1.0,
            dev_overlay_cache: None,
            dev_overlay: SceneStore::new(),
            fps_overlay: None,
            inspector_overlay: None,
            root_scene_capacity: SceneCapacityHint::default(),
            scene_recyclers: std::array::from_fn(|_| LayerSceneRecycler::default()),
            returned_scenes: 0,
            layer_motion: LayerMotion::default(),
            frame_sequence: 0,
            transparent_background: false,
            held_still: false,
            pointer_held: false,
            kept_packet: None,
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
        self.scene.root_layer()?;
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
        let store = self.scene.store();
        let root_layer = store.root_layer()?;
        self.returned_scenes = self.returned_scenes.saturating_sub(1);
        let recycler = &mut self.scene_recyclers[self.returned_scenes];
        let root = collect_root(
            root_layer,
            store.update(),
            &mut self.layer_motion,
            self.root_scene_capacity,
            root_scale,
            recycler,
        );
        self.root_scene_capacity = root.scene.capacity_hint();
        let after_root_collect = Instant::now();
        let overlay = self
            .dev_overlay
            .root_layer()
            .map(|overlay| collect_overlay(overlay, root_scale, recycler));
        recycler.clear();
        self.frame_sequence = self.frame_sequence.wrapping_add(1);
        let packet = FramePacket {
            frame_id: self.frame_sequence,
            viewport: (width, height),
            renderer_epoch,
            surface_epoch,
            root_scale,
            root,
            overlay,
            clear: frame_clear_color(self.transparent_background),
            held_still: std::mem::take(&mut self.held_still),
            pointer_held: std::mem::take(&mut self.pointer_held),
            scene_update: store.version(),
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

    // The packet of the current scene at `width` by `height`: the one kept
    // by `keep_packet` while the scene and the frame's setting stay as they
    // were, else a new one.
    pub(crate) fn take_packet(
        &mut self,
        width: u32,
        height: u32,
        renderer_epoch: u64,
        surface_epoch: u64,
    ) -> Option<FramePacket> {
        self.scene.root_layer()?;
        let version = self.scene.store().version();
        if let Some(packet) = self.kept_packet.take() {
            if packet.scene_update == version
                && packet.viewport == (width, height)
                && (packet.renderer_epoch, packet.surface_epoch) == (renderer_epoch, surface_epoch)
                && packet.root_scale.to_bits() == self.root_scale.to_bits()
                && packet.clear == frame_clear_color(self.transparent_background)
            {
                return Some(packet);
            }
            self.recycle(packet);
        }
        self.build_frame_packet(width, height, renderer_epoch, surface_epoch)
    }

    // Keeps `packet`, which no surface showed yet, for the next `take_packet`.
    pub(crate) fn keep_packet(&mut self, packet: FramePacket) {
        self.kept_packet = Some(packet);
    }

    // Gives the scene of a packet nothing drew back to the recyclers.
    pub(crate) fn recycle(&mut self, packet: FramePacket) {
        self.apply_returns(RenderReturns {
            scene: Some(FrameSceneStorage {
                root: packet.root,
                overlay: packet.overlay,
            }),
            ..RenderReturns::default()
        });
    }

    pub(crate) fn apply_returns(&mut self, returns: RenderReturns) {
        if let Some(frame) = returns.scene {
            self.root_scene_capacity = frame.root.scene.capacity_hint();
            if self.returned_scenes == MAX_FRAMES_IN_FLIGHT {
                self.scene_recyclers.rotate_left(1);
                self.returned_scenes -= 1;
            }
            let recycler = &mut self.scene_recyclers[self.returned_scenes];
            recycler.clear();
            if let Some(overlay) = frame.overlay {
                recycler.recycle(overlay);
            }
            recycler.recycle(frame.root);
            recycler.returned_packet();
            self.returned_scenes += 1;
        }
    }
}

#[cfg(test)]
#[path = "tests/frontend_tests.rs"]
mod tests;
