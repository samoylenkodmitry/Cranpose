use std::rc::Weak;

use cranpose_render_common::{
    dev_overlay::DevOverlayKey,
    graph::{LayerNode, RenderNode},
    scene_store::SceneStore,
    text_fonts::TextFonts,
};
use web_time::Instant;

use crate::{
    collect::{LayerMotion, LayerScene, LayerSceneRecycler, collect_overlay, collect_root},
    frame::{Damage, DamageRects, ImageContent, PacketDamage},
    frame_packet::{
        CancelReason, FramePacket, FrameSceneStorage, MAX_FRAMES_IN_FLIGHT, PresentOutcome,
        RenderReturns,
    },
    render::{frame_clear_color, instant_ms, should_log_wgpu_render_stage},
    scene::{Scene, SceneCapacityHint},
    target_state::{Journaled, TargetSetting, TargetState},
    window_packets::{PacketDraws, WindowPackets},
};

/// What a packet is built for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PacketFor {
    /// The window: its frame changes the window's frame before, which the
    /// window's target state follows.
    Window,
    /// A capture at a scale of its own, drawn whole.
    Capture,
}

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
    // What the window drew of each record of the scene and of the overlays,
    // which each window frame's damage is worked out against.
    target: TargetState,
    overlay_target: TargetState,
    // The window packets built last, which a window walk culls against.
    window: WindowPackets,
    // Whether the next window walk takes every record: an image refused a
    // culled packet.
    uncull_next: bool,
    // Whether the last window walk that took every record found a backdrop,
    // whose reads a culled walk does not follow.
    backdrops: bool,
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
            target: TargetState::default(),
            overlay_target: TargetState::default(),
            window: WindowPackets::default(),
            uncull_next: false,
            backdrops: false,
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
            (width, height, self.root_scale),
            renderer_epoch,
            surface_epoch,
            PacketFor::Window,
        )
    }

    pub(crate) fn build_frame_packet_with_scale(
        &mut self,
        (width, height, root_scale): (u32, u32, f32),
        renderer_epoch: u64,
        surface_epoch: u64,
        purpose: PacketFor,
    ) -> Option<FramePacket> {
        self.scene.root_layer()?;
        let app_context = self.app_context.as_ref().and_then(Weak::upgrade);
        match app_context {
            Some(app_context) => app_context.enter(|| {
                self.build_frame_packet_inner(
                    (width, height, root_scale),
                    renderer_epoch,
                    surface_epoch,
                    purpose,
                )
            }),
            None => self.build_frame_packet_inner(
                (width, height, root_scale),
                renderer_epoch,
                surface_epoch,
                purpose,
            ),
        }
    }

    fn build_frame_packet_inner(
        &mut self,
        (width, height, root_scale): (u32, u32, f32),
        renderer_epoch: u64,
        surface_epoch: u64,
        purpose: PacketFor,
    ) -> Option<FramePacket> {
        let build_start = Instant::now();
        let window = purpose == PacketFor::Window;
        if window {
            self.begin_targets(TargetSetting {
                viewport: (width, height),
                root_scale,
                clear: frame_clear_color(self.transparent_background),
            });
        }
        self.scene.root_layer()?;
        self.returned_scenes = self.returned_scenes.saturating_sub(1);
        let (overlay, overlay_damage) = self.collect_overlays(root_scale, purpose);
        let (root, damage, cull) = self.collect_scene(
            (width, height, root_scale),
            purpose,
            (overlay.as_deref(), overlay_damage),
        );
        let after_root_collect = Instant::now();
        self.scene_recyclers[self.returned_scenes].clear();
        self.frame_sequence = self.frame_sequence.wrapping_add(1);
        let scene_update = self.scene.store().version();
        let draws = PacketDraws {
            scene: scene_update,
            overlay: self.dev_overlay.version(),
            viewport: (width, height),
            root_scale: root_scale.to_bits(),
            transparent: self.transparent_background,
        };
        let damage = if window {
            self.window_packet_damage(damage, draws)
        } else {
            self.window.capture_damage(draws)
        };
        let packet = FramePacket {
            frame_id: self.frame_sequence,
            viewport: (width, height),
            renderer_epoch,
            surface_epoch,
            root_scale,
            root,
            overlay,
            clear: frame_clear_color(self.transparent_background),
            pointer_held: std::mem::take(&mut self.pointer_held),
            scene_update,
            damage,
            skipped: window && !self.target.records(),
            culled: cull,
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

    // The overlays' scene for a frame at `root_scale` for `purpose`, and
    // what a window frame changes of them.
    fn collect_overlays(
        &mut self,
        root_scale: f32,
        purpose: PacketFor,
    ) -> (Option<Box<LayerScene>>, Damage) {
        let window = purpose == PacketFor::Window;
        let recycler = &mut self.scene_recyclers[self.returned_scenes];
        let overlay = self.dev_overlay.root_layer().map(|overlay| {
            collect_overlay(
                overlay,
                (root_scale, window && self.overlay_target.records()),
                recycler,
            )
        });
        let damage = if window {
            self.overlay_target
                .end(&self.dev_overlay, overlay.as_deref())
        } else {
            Damage::Full
        };
        (overlay, damage)
    }

    // The scene's root scene for a frame of `viewport` at `root_scale` for
    // `purpose`, over
    // `overlay` that changes `overlay_damage`: what a window frame changes,
    // and the region a window walk culled the root to. A culled walk an
    // image repaints past is taken again over what it repaints once, and
    // then over every record.
    fn collect_scene(
        &mut self,
        (width, height, root_scale): (u32, u32, f32),
        purpose: PacketFor,
        (overlay, overlay_damage): (Option<&LayerScene>, Damage),
    ) -> (Box<LayerScene>, Damage, Option<DamageRects>) {
        let window = purpose == PacketFor::Window;
        let mut cull = if window { self.cull_region() } else { None };
        let store = self.scene.store();
        let root_layer = store.root_layer().expect("the scene has a root");
        let recycler = &mut self.scene_recyclers[self.returned_scenes];
        let overlay_backdrops = overlay.is_some_and(LayerScene::contains_backdrop);
        let records = (store.update(), window && self.target.records());
        let mut widened = false;
        let (root, damage) = loop {
            let culled = cull.map(|region| self.target.cull(store, region));
            let root = collect_root(
                root_layer,
                records,
                culled.as_ref(),
                &mut self.layer_motion,
                self.root_scene_capacity,
                root_scale,
                recycler,
            );
            if !window {
                break (root, Damage::Full);
            }
            let damage = self
                .target
                .end(store, Some(&root))
                .union(overlay_damage, (width, height));
            let Some(region) = cull else {
                break (root, damage);
            };
            let since = self
                .window
                .last()
                .filter(|_| self.target.knew_before() && self.overlay_target.knew_before());
            let next = self.window.next_damage(damage, since, (width, height));
            let repaint = if overlay_backdrops || root.contains_backdrop() {
                None
            } else {
                self.window
                    .repaint(self.frame_sequence.wrapping_add(1), next)
            };
            if repaint.is_some_and(|repaint| region.covers(&repaint)) {
                break (root, damage);
            }
            recycler.recycle(root);
            cull = repaint.filter(|_| !widened).map(|repaint| {
                let mut wider = region;
                for rect in repaint.as_slice() {
                    wider.add(*rect);
                }
                wider
            });
            widened = true;
        };
        if cull.is_none() {
            self.root_scene_capacity = root.scene.capacity_hint();
            self.backdrops = window && (overlay_backdrops || root.contains_backdrop());
        }
        (root, damage, cull)
    }

    // Starts a window frame of `setting` in the targets of the scene and of
    // the overlays.
    fn begin_targets(&mut self, setting: TargetSetting) {
        if std::mem::take(&mut self.held_still) {
            self.target.held_still();
        }
        self.target
            .begin(Journaled::Scene(&mut self.scene), setting);
        self.overlay_target
            .begin(Journaled::Store(&mut self.dev_overlay), setting);
        if self.target.records() {
            self.scene.refresh_reach();
        }
        if self.overlay_target.records() {
            self.dev_overlay.refresh_reach();
        }
    }

    // The region a window walk may cull to: the damages since the oldest
    // packet the window's targets hold, and the areas the targets drew the
    // changed records in; `None` when the walk takes every record.
    fn cull_region(&mut self) -> Option<DamageRects> {
        if std::mem::take(&mut self.uncull_next) || self.backdrops {
            return None;
        }
        let begun = [self.target.begun()?, self.overlay_target.begun()?];
        self.window.cull_region(begun)
    }

    // The damage of the window packet that draws `draws` with `damage`.
    fn window_packet_damage(&mut self, damage: Damage, draws: PacketDraws) -> PacketDamage {
        let since = self
            .window
            .last()
            .filter(|_| self.target.knew_before() && self.overlay_target.knew_before());
        let damage = self.window.next_damage(damage, since, draws.viewport);
        self.window.built(self.frame_sequence, damage, draws);
        damage
    }

    // Takes the window packet at `width` by `height` and draws it with
    // `draw` into `image`; a packet an image refused as culled past what it
    // needs is built again with every draw and drawn again, as is one for a
    // texture that keeps no frame. `None` when the scene has no root.
    pub(crate) fn draw_window(
        &mut self,
        (width, height): (u32, u32),
        (renderer_epoch, surface_epoch): (u64, u64),
        image: ImageContent,
        mut draw: impl FnMut(FramePacket, &mut RenderReturns) -> Result<(), String>,
    ) -> Option<Result<(), String>> {
        self.uncull_next |= image == ImageContent::Untracked;
        loop {
            let packet = self.take_packet(width, height, renderer_epoch, surface_epoch)?;
            let mut returns = RenderReturns::default();
            let result = draw(packet, &mut returns);
            let refused = returns.outcome == PresentOutcome::Cancelled(CancelReason::NeedsWhole);
            self.apply_returns(returns);
            if !refused {
                return Some(result);
            }
        }
    }

    // The window packet of the current scene at `width` by `height`: the one
    // kept by `keep_packet` while the scene and the frame's setting stay as
    // they were, else a new one.
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

    // Gives the scene of a packet nothing drew back to the recyclers. The
    // next window packet changes against what the packet changed against.
    pub(crate) fn recycle(&mut self, packet: FramePacket) {
        self.window.went_back(packet.frame_id, packet.damage);
        self.apply_returns(RenderReturns {
            scene: Some(FrameSceneStorage {
                root: packet.root,
                overlay: packet.overlay,
            }),
            ..RenderReturns::default()
        });
    }

    // Forgets the frames the window's targets hold: the renderer let a
    // target's frame go.
    pub(crate) fn forget_held(&mut self) {
        self.window.hold(None);
    }

    pub(crate) fn apply_returns(&mut self, returns: RenderReturns) {
        if let Some(drawn) = returns.target {
            self.target.drawn(drawn);
            self.window.hold(returns.held);
        }
        if returns.outcome == PresentOutcome::Cancelled(CancelReason::NeedsWhole) {
            self.uncull_next = true;
        }
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
