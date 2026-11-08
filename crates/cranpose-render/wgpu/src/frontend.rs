use std::rc::Weak;

use cranpose_render_common::{
    dev_overlay::DevOverlayKey,
    graph::{LayerNode, RenderNode},
    scene_store::{SceneStore, SceneVersion},
    text_fonts::TextFonts,
};
use web_time::Instant;

use crate::{
    collect::{LayerMotion, LayerScene, LayerSceneRecycler, collect_overlay, collect_root},
    frame::{Damage, DamageRects, PacketDamage, settle},
    frame_packet::{FramePacket, FrameSceneStorage, MAX_FRAMES_IN_FLIGHT, RenderReturns},
    render::{frame_clear_color, instant_ms, should_log_wgpu_render_stage},
    scene::{Scene, SceneCapacityHint},
    target_state::{Journaled, TargetSetting, TargetState},
};

// What a packet draws: two packets of the same draw the same frame.
#[derive(Clone, Copy, PartialEq, Eq)]
struct PacketDraws {
    scene: SceneVersion,
    overlay: SceneVersion,
    viewport: (u32, u32),
    root_scale: u32,
    transparent: bool,
}

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
    // The window packet built last, which the next one changes against, and
    // what it drew: a capture of the same draws the same frame.
    last_window_packet: Option<u64>,
    last_window_draws: Option<PacketDraws>,
    // The damage of the window packet built last when it went back unshown:
    // the next window packet changes against what it changed against.
    carried: Option<PacketDamage>,
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
            last_window_packet: None,
            last_window_draws: None,
            carried: None,
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
        let store = self.scene.store();
        let root_layer = store.root_layer()?;
        self.returned_scenes = self.returned_scenes.saturating_sub(1);
        let recycler = &mut self.scene_recyclers[self.returned_scenes];
        let mut root = collect_root(
            root_layer,
            (store.update(), window && self.target.records()),
            &mut self.layer_motion,
            self.root_scene_capacity,
            root_scale,
            recycler,
        );
        self.root_scene_capacity = root.scene.capacity_hint();
        let after_root_collect = Instant::now();
        let mut overlay = self.dev_overlay.root_layer().map(|overlay| {
            collect_overlay(
                overlay,
                (root_scale, window && self.overlay_target.records()),
                recycler,
            )
        });
        recycler.clear();
        self.frame_sequence = self.frame_sequence.wrapping_add(1);
        let scene_update = store.version();
        let draws = PacketDraws {
            scene: scene_update,
            overlay: self.dev_overlay.version(),
            viewport: (width, height),
            root_scale: root_scale.to_bits(),
            transparent: self.transparent_background,
        };
        let damage = self.packet_damage(purpose, draws, (&mut root, overlay.as_deref_mut()));
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

    // The damage of the packet for `purpose` that draws `draws` from `root`
    // and `overlay`: a window packet's from its targets' changes since the
    // window packet before, a capture's none when the window packet built
    // last draws the same.
    fn packet_damage(
        &mut self,
        purpose: PacketFor,
        draws: PacketDraws,
        (root, overlay): (&mut LayerScene, Option<&mut LayerScene>),
    ) -> PacketDamage {
        if purpose == PacketFor::Capture {
            return if self.last_window_draws == Some(draws) {
                PacketDamage {
                    since: self.last_window_packet,
                    damage: Damage::Part(DamageRects::default()),
                }
            } else {
                PacketDamage::WHOLE
            };
        }
        let viewport = draws.viewport;
        let damage = union(
            self.target.end(self.scene.store(), Some(root)),
            self.overlay_target.end(&self.dev_overlay, overlay),
            viewport,
        );
        let damage = match self.carried.take() {
            Some(carried) => PacketDamage {
                since: carried.since,
                damage: union(carried.damage, damage, viewport),
            },
            None => PacketDamage {
                since: self
                    .last_window_packet
                    .filter(|_| self.target.knew_before() && self.overlay_target.knew_before()),
                damage,
            },
        };
        self.last_window_packet = Some(self.frame_sequence);
        self.last_window_draws = Some(draws);
        damage
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
        if self.last_window_packet == Some(packet.frame_id) {
            self.carried = Some(packet.damage);
        }
        self.apply_returns(RenderReturns {
            scene: Some(FrameSceneStorage {
                root: packet.root,
                overlay: packet.overlay,
            }),
            ..RenderReturns::default()
        });
    }

    pub(crate) fn apply_returns(&mut self, returns: RenderReturns) {
        if let Some(drawn) = returns.target {
            self.target.drawn(drawn);
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

// The pixels either damage changes, in a window of `viewport`.
fn union(a: Damage, b: Damage, viewport: (u32, u32)) -> Damage {
    match (a, b) {
        (Damage::Part(mut a), Damage::Part(b)) => {
            for rect in b.as_slice() {
                a.add(*rect);
            }
            settle(a, viewport)
        }
        (Damage::Full, _) | (_, Damage::Full) => Damage::Full,
        (Damage::Skipped, _) | (_, Damage::Skipped) => Damage::Skipped,
    }
}
