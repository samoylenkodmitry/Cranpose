use std::rc::Weak;

#[cfg(feature = "layer-tree")]
use cranpose_render_common::graph::{LayerNode, RenderNode};
use cranpose_render_common::{
    dev_overlay::{DevOverlayKey, FpsOverlay},
    scene_store::{OverlayLayer, SceneStore},
    text_fonts::TextFonts,
};
use web_time::Instant;

use crate::{
    collect::{
        LayerMotion, LayerScene, LayerSceneRecycler, ShownRecord, collect_overlay, collect_root,
    },
    frame::{Damage, DamageRects, ImageContent, PacketDamage},
    frame_packet::{
        CancelReason, FramePacket, FrameSceneStorage, MAX_FRAMES_IN_FLIGHT, PresentOutcome,
        RenderReturns,
    },
    render::{frame_clear_color, instant_ms, should_log_wgpu_render_stage},
    scene::{Scene, SceneCapacityHint},
    target_state::{Prediction, TargetSetting, TargetState},
    window_packets::{PacketDraws, WindowPackets},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PacketFor {
    // The window: its frame changes the window's frame before, which the
    // window's target state follows.
    Window,
    // A capture at a scale of its own, drawn whole.
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
    pub(crate) fps_overlay: Option<FpsOverlay>,
    pub(crate) inspector_overlay: Option<OverlayLayer>,
    pub(crate) root_scene_capacity: SceneCapacityHint,
    scene_recyclers: [LayerSceneRecycler; MAX_FRAMES_IN_FLIGHT],
    returned_scenes: usize,
    // How a window walk of the root and one of the overlays drew each
    // record, which their targets read as the walk ends: the lists stay
    // here, as no packet in flight needs them.
    root_shown: Vec<ShownRecord>,
    overlay_shown: Vec<ShownRecord>,
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
    // The surface epoch of the window packet built last: the images of a
    // newer surface hold no frame the window drew.
    surface_epoch: u64,
    // Whether the last window walk that took every record found a backdrop,
    // whose reads a culled walk does not follow.
    backdrops: bool,
    // The renderer's count of pipeline landings as the next packet is built:
    // a frame draws the stand-ins of the frame its target holds again only
    // once the count moved. `None` while no renderer counts them.
    pub(crate) landings: Option<u64>,
}

impl RendererFrontend {
    pub(crate) fn refresh_dev_overlay(&mut self) {
        self.recycle_kept_packet();
        if self.fps_overlay.is_none() && self.inspector_overlay.is_none() {
            self.dev_overlay.clear();
            return;
        }
        let window = self
            .fps_overlay
            .as_ref()
            .map_or_else(Default::default, |fps| fps.window);
        // The inspector draws over the frame counter, as the last layer of
        // the overlay's root.
        let layers = self
            .fps_overlay
            .iter()
            .map(|fps| &fps.layer)
            .chain(&self.inspector_overlay);
        self.dev_overlay.describe_overlay(window, layers);
    }

    // Replaces the overlay with the records of `overlay`, a layer tree, in a
    // root of its own.
    #[cfg(feature = "layer-tree")]
    pub(crate) fn describe_overlay_tree(&mut self, overlay: Option<LayerNode>) {
        self.recycle_kept_packet();
        match overlay {
            Some(layer) => self.dev_overlay.describe(&LayerNode {
                children: vec![RenderNode::Layer(Box::new(layer))],
                ..Default::default()
            }),
            None => self.dev_overlay.clear(),
        }
    }

    fn recycle_kept_packet(&mut self) {
        if let Some(packet) = self.kept_packet.take() {
            self.recycle(packet);
        }
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
            root_shown: Vec::new(),
            overlay_shown: Vec::new(),
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
            surface_epoch: 0,
            backdrops: false,
            landings: None,
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
        let debug_toggles = cranpose_core::debug_toggles::debug_toggle_generation();
        if window {
            if surface_epoch != self.surface_epoch {
                self.surface_epoch = surface_epoch;
                self.window.hold(None);
            }
            self.begin_targets(TargetSetting {
                viewport: (width, height),
                root_scale,
                clear: frame_clear_color(self.transparent_background),
                debug_toggles,
            });
        }
        self.scene.root_layer()?;
        self.returned_scenes = self.returned_scenes.saturating_sub(1);
        // The region starts from the areas the targets drew the changed
        // records in, which their ends add the new areas to.
        let mut cull = None;
        if window {
            let region = cull.insert(DamageRects::default());
            if !self.cull_region(region) {
                cull = None;
            }
        }
        let mut overlay_damage = Damage::Full;
        let overlay = self.collect_overlays(root_scale, purpose, &mut overlay_damage);
        // Built field by field: a constant of the whole packet damage would
        // be copied in.
        let mut damage = PacketDamage {
            since: None,
            damage: Damage::Full,
        };
        let root = self.collect_scene(
            (width, height, root_scale),
            purpose,
            (overlay.as_deref(), &overlay_damage),
            (&mut damage, &mut cull),
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
        if window {
            self.window_packet_damage(&mut damage, draws);
        } else {
            damage = self.window.capture_damage(draws);
        }
        let packet = FramePacket {
            frame_id: self.frame_sequence,
            viewport: (width, height),
            renderer_epoch,
            surface_epoch,
            root_scale,
            root,
            overlay,
            clear: frame_clear_color(self.transparent_background),
            debug_toggles,
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

    // The overlays' scene for a frame at `root_scale` for `purpose`; writes
    // into `damage` what a window frame changes of them.
    fn collect_overlays(
        &mut self,
        root_scale: f32,
        purpose: PacketFor,
        damage: &mut Damage,
    ) -> Option<Box<LayerScene>> {
        let window = purpose == PacketFor::Window;
        let records = window && self.overlay_target.records();
        let recycler = &mut self.scene_recyclers[self.returned_scenes];
        let shown = &mut self.overlay_shown;
        let mut overlay = self.dev_overlay.root_layer().map(|overlay| {
            collect_overlay(overlay, (root_scale, records.then_some(shown)), recycler)
        });
        if window {
            self.overlay_target
                .end(&self.dev_overlay, overlay.as_deref(), damage);
        } else {
            *damage = Damage::Full;
        }
        if let Some(overlay) = overlay.as_mut().filter(|_| records) {
            self.overlay_shown = std::mem::take(&mut overlay.shown);
        }
        overlay
    }

    // The scene's root scene for a frame of `viewport` at `root_scale` for
    // `purpose`, over `overlay` that changes `overlay_damage`. Writes into
    // `damage` what a window frame changes against the packet before. A
    // window walk culls the root to `cull`, which the frame widens by what
    // the next image repaints, or drops. A culled walk an image repaints
    // past is taken again over what it repaints once, and then over every
    // record.
    fn collect_scene(
        &mut self,
        (width, height, root_scale): (u32, u32, f32),
        purpose: PacketFor,
        (overlay, overlay_damage): (Option<&LayerScene>, &Damage),
        (damage, cull): (&mut PacketDamage, &mut Option<DamageRects>),
    ) -> Box<LayerScene> {
        let window = purpose == PacketFor::Window;
        let viewport = (width, height);
        damage.since = self.since();
        let next = self.frame_sequence.wrapping_add(1);
        if let Some(region) = cull.as_mut()
            && !self.widen_by_predicted_repaint(region, next, overlay_damage, viewport, damage)
        {
            *cull = None;
        }
        let store = self.scene.store();
        let root_layer = store.root_layer().expect("the scene has a root");
        let recycler = &mut self.scene_recyclers[self.returned_scenes];
        let overlay_backdrops = overlay.is_some_and(LayerScene::contains_backdrop);
        // Whether the walk records how it draws each record: only the first
        // walk of a window frame that records does. A later walk takes in
        // every record the first took. The records it adds did not change
        // since the target kept their areas.
        let mut records = window && self.target.records();
        // Whether a walk of the frame ran before this one.
        let mut again = false;
        let mut repaint = DamageRects::default();
        let root = loop {
            let culled = cull.as_ref().map(|region| self.target.cull(store, region));
            let shown = records.then_some(&mut self.root_shown);
            let mut root = collect_root(
                root_layer,
                (store.update(), shown),
                culled.as_ref(),
                &mut self.layer_motion,
                self.root_scene_capacity,
                root_scale,
                recycler,
            );
            if !window {
                break root;
            }
            if !again {
                self.target.end(store, Some(&root), &mut damage.damage);
                damage.damage.add(overlay_damage, viewport);
                if records {
                    self.root_shown = std::mem::take(&mut root.shown);
                }
            } else if root.contains_backdrop() {
                // The end widened the damage by the captures of the first
                // walk's backdrops alone. The captures of the later walk's
                // backdrops widen it again.
                self.target.end_again(&root, &mut damage.damage);
                damage.damage.add(overlay_damage, viewport);
            }
            let Some(region) = cull.as_mut() else {
                break root;
            };
            let repainted = !(overlay_backdrops || root.contains_backdrop())
                && self
                    .window
                    .repaint(next, damage, viewport, self.landings, &mut repaint);
            if repainted && region.covers(&repaint) {
                break root;
            }
            recycler.recycle(root);
            if repainted && !again {
                region.add_all(&repaint);
            } else {
                *cull = None;
            }
            again = true;
            records = false;
        };
        if cull.is_none() {
            self.root_scene_capacity = root.scene.capacity_hint();
            self.backdrops = window && (overlay_backdrops || root.contains_backdrop());
        }
        root
    }

    // Widens `region`, which a window walk culls to, by what the next image
    // repaints when the frame changes what the target predicts, so the walk
    // takes in the repaint without walking again; `false`, a walk of every
    // record, when the frame changes most of the window or that image
    // repaints whole. The predicted damage goes into
    // `packet`, the damage of the packet `next`, which the frame's own
    // damage replaces after its walk.
    fn widen_by_predicted_repaint(
        &mut self,
        region: &mut DamageRects,
        next: u64,
        overlay_damage: &Damage,
        viewport: (u32, u32),
        packet: &mut PacketDamage,
    ) -> bool {
        match self
            .target
            .predict_damage(self.scene.store(), &mut packet.damage)
        {
            Prediction::Unknown => return true,
            Prediction::Whole => return false,
            Prediction::Part => {}
        }
        packet.damage.add(overlay_damage, viewport);
        let mut repaint = DamageRects::default();
        if !self
            .window
            .repaint(next, packet, viewport, self.landings, &mut repaint)
        {
            return false;
        }
        region.add_all(&repaint);
        true
    }

    // Gives back at rest the room the kept layer scenes took, and the
    // overlays' records past what they hold.
    pub(crate) fn trim(&mut self) {
        for recycler in &mut self.scene_recyclers {
            recycler.trim();
        }
        self.dev_overlay.trim();
    }

    // Starts a window frame of `setting` in the targets of the scene and of
    // the overlays.
    fn begin_targets(&mut self, setting: TargetSetting) {
        if std::mem::take(&mut self.held_still) {
            self.target.held_still();
        }
        self.target.begin(self.scene.store(), setting);
        self.scene.follow(self.target.followed());
        self.overlay_target.begin(&self.dev_overlay, setting);
        self.dev_overlay.follow(self.overlay_target.followed());
        if self.target.records() {
            self.scene.refresh_reach();
        }
        if self.overlay_target.records() {
            self.dev_overlay.refresh_reach();
        }
    }

    // Writes into `region` the region a window walk may cull to: the damages
    // since the oldest packet the window's targets hold, and the areas the
    // targets drew the changed records in; `false` when the walk takes every
    // record.
    fn cull_region(&mut self, region: &mut DamageRects) -> bool {
        if std::mem::take(&mut self.uncull_next) || self.backdrops {
            return false;
        }
        let (Some(target), Some(overlay)) = (self.target.begun(), self.overlay_target.begun())
        else {
            return false;
        };
        self.window
            .cull_region([target, overlay], self.landings, region)
    }

    // The last window packet, when the targets knew what the images held
    // before this frame: the packet the next one changes against.
    fn since(&self) -> Option<u64> {
        self.window
            .last()
            .filter(|_| self.target.knew_before() && self.overlay_target.knew_before())
    }

    // Makes `damage`, what the frame changes, the damage of the window
    // packet that draws `draws`, and notes the packet built.
    fn window_packet_damage(&mut self, damage: &mut PacketDamage, draws: PacketDraws) {
        damage.since = self.since();
        self.window.carry(damage, draws.viewport);
        self.window.built(self.frame_sequence, damage, draws);
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
        if let Some(drawn) = &returns.target {
            self.target.drawn(drawn);
            self.window.hold(returns.held.as_ref());
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
            self.returned_scenes += 1;
        }
    }
}

#[cfg(test)]
#[path = "tests/frontend_tests.rs"]
mod tests;
