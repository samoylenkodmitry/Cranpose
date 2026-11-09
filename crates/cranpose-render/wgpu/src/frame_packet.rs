use cranpose_render_common::scene_store::SceneVersion;

use crate::{
    collect::{ChildLayer, LayerScene},
    frame::{DamageRects, PacketDamage},
    scene::{
        BackdropLayer, CompositorScene, DrawOp, EffectLayer, ImageDraw, RunDraw, ShadowDraw,
        TextDraw,
    },
    target_state::TargetDrawn,
};

pub(crate) const MAX_FRAMES_IN_FLIGHT: usize = 2;

/// Why the present stage refused a packet without drawing it. Each reason
/// names the expectation the packet no longer matches; the frame is not an
/// error, its scene travels back through `RenderReturns` for recycling.
/// `pub` (not `pub(crate)`) because the cancellation-protocol tests in
/// `tests/` observe outcomes through `#[doc(hidden)]` hooks; the module is
/// private, so the only public path is the hidden re-export in `lib.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelReason {
    /// The packet was built against a different `GpuRenderer` instance.
    RendererEpoch,
    /// The packet was built against a different surface configuration.
    SurfaceEpoch,
    /// The packet was lowered for a different surface size.
    Viewport,
    /// The present stage had no usable surface to draw the packet on: the
    /// surface was dropped, or acquire failed past its one reconfigure
    /// retry. Producer state (epochs, viewport) still matched, only the
    /// swapchain was missing.
    SurfaceUnavailable,
    /// The device reported an uncaptured error (validation/OOM/internal)
    /// since the previous frame: nothing of this packet is encoded on the
    /// suspect device. One cancel per poisoning; the gate clears as it
    /// fires, so the next packet renders (see `DeviceErrorSentry`).
    DeviceError,
    /// The packet holds the draws of a part of the window, and the image it
    /// went into needs more: the next packet holds every draw.
    NeedsWhole,
}

/// What the present stage did with a packet. `NotRun` is the `Default` so a
/// draw that never happened can never be reported `Presented`.
/// `pub` like [`CancelReason`], for the same hidden test re-export.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PresentOutcome {
    /// No packet was consumed (the default of an untouched returns value).
    #[default]
    NotRun,
    /// The packet was drawn and presented.
    Presented,
    /// The packet drew what the surface showed: no image, draw or present.
    Unchanged,
    /// The packet was refused before any encoding; the scene returned whole.
    Cancelled(CancelReason),
}

pub(crate) struct FramePacket {
    pub(crate) frame_id: u64,
    pub(crate) viewport: (u32, u32),
    pub(crate) renderer_epoch: u64,
    pub(crate) surface_epoch: u64,
    pub(crate) root_scale: f32,
    pub(crate) root: Box<LayerScene>,
    pub(crate) overlay: Option<Box<LayerScene>>,
    pub(crate) clear: wgpu::Color,
    // Whether a pointer was held on the app's controls for this frame.
    pub(crate) pointer_held: bool,
    // The scene update the packet was collected from: a frame drawn from the
    // update the frame before drew shows nothing new of the app's.
    pub(crate) scene_update: SceneVersion,
    // What the frame changes against the packet before it.
    pub(crate) damage: PacketDamage,
    // Whether the frame was one to skip working out what it changes, while
    // frames keep changing most of the window.
    pub(crate) skipped: bool,
    // The window rects the packet holds every draw within; `None` when it
    // holds every draw of the scene.
    pub(crate) culled: Option<DamageRects>,
}

impl FramePacket {
    // How the packet's scene moved since the frame that drew `drawn`, the
    // scene update before.
    pub(crate) fn scene_motion(&self, drawn: Option<SceneVersion>) -> SceneMotion {
        if self.pointer_held {
            SceneMotion::Held
        } else if drawn == Some(self.scene_update) {
            SceneMotion::Still
        } else {
            SceneMotion::Moving
        }
    }

    // Whether a backdrop of the packet reads the page beneath it.
    pub(crate) fn reads_page(&self) -> bool {
        self.root.contains_backdrop()
            || self
                .overlay
                .as_ref()
                .is_some_and(|overlay| overlay.contains_backdrop())
    }
}

// How a frame's scene moved since the frame before, which on WebGL decides
// whether a paced build runs at its end (see `PipelineCompiler::paced`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SceneMotion {
    // The frame draws the scene update the frame before drew, with no
    // pointer held.
    Still,
    // The scene changed, with no pointer held.
    Moving,
    // A pointer is held on the app's controls.
    Held,
}

pub(crate) struct FrameSceneStorage {
    pub(crate) root: Box<LayerScene>,
    pub(crate) overlay: Option<Box<LayerScene>>,
}

/// Present-stage timestamps for one consumed packet, in nanoseconds on the
/// clock the producer injected at runtime start (`0` = stage did not run or
/// no clock was injected). Carried back in `RenderReturns` so the
/// producer's frame telemetry keeps recording acquire/render/present phases
/// after those stages move to the present thread. Plain integers so the
/// packet types stay clock-library-free on every target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PresentTimings {
    /// When the present stage had acquired the surface texture.
    pub after_acquire_ns: i64,
    /// When the present stage had rendered the frame's draws.
    pub after_render_ns: i64,
    /// When the present stage had presented the frame to the surface.
    pub after_present_ns: i64,
}

impl RenderReturns {
    // What a target that keeps its frames returns for `packet` once it drew
    // the packet's damage over the frame before.
    pub(crate) fn drawn_in_place(packet: FramePacket) -> Self {
        Self {
            target: Some(TargetDrawn {
                damage: packet.damage.damage,
                skipped: packet.skipped,
                knew_before: packet.damage.since.is_some(),
                cleared: false,
            }),
            held: Some(packet.frame_id),
            scene: Some(FrameSceneStorage {
                root: packet.root,
                overlay: packet.overlay,
            }),
            frame_id: packet.frame_id,
            ..Self::default()
        }
    }
}

#[derive(Default)]
pub(crate) struct RenderReturns {
    pub(crate) scene: Option<FrameSceneStorage>,
    pub(crate) frame_id: u64,
    pub(crate) outcome: PresentOutcome,
    // How the frame went into a target that keeps its frames; `None` for a
    // frame drawn into a texture that keeps none.
    pub(crate) target: Option<TargetDrawn>,
    // The oldest packet the window's next frame finds in its target; `None`
    // when a target holds none known.
    pub(crate) held: Option<u64>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) timings: PresentTimings,
}

const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<FramePacket>();
    assert_send::<FrameSceneStorage>();
    assert_send::<LayerScene>();
    assert_send::<ChildLayer>();
    assert_send::<CompositorScene>();
    assert_send::<RunDraw>();
    assert_send::<ImageDraw>();
    assert_send::<TextDraw>();
    assert_send::<ShadowDraw>();
    assert_send::<DrawOp>();
    assert_send::<EffectLayer>();
    assert_send::<BackdropLayer>();
    assert_send::<RenderReturns>();
    assert_send::<CancelReason>();
    assert_send::<PresentOutcome>();
    assert_send::<PresentTimings>();
};
