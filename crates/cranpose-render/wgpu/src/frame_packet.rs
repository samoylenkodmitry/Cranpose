use crate::{
    collect::{ChildLayer, LayerScene},
    scene::{
        BackdropLayer, CompositorScene, DrawOp, EffectLayer, ImageDraw, RunDraw, ShadowDraw,
        TextDraw,
    },
};

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
    /// The packet was refused before any encoding; the scene returned whole.
    Cancelled(CancelReason),
}

pub(crate) struct FramePacket {
    pub(crate) frame_id: u64,
    pub(crate) viewport: (u32, u32),
    pub(crate) renderer_epoch: u64,
    pub(crate) surface_epoch: u64,
    pub(crate) root_scale: f32,
    pub(crate) root: LayerScene,
    pub(crate) overlay: Option<LayerScene>,
    pub(crate) text_cache_len: usize,
}

/// Present-stage timestamps for one consumed packet, in nanoseconds on the
/// clock the producer injected at runtime start (`0` = stage did not run or
/// no clock was injected). Carried back in `RenderReturns` so the
/// producer's frame telemetry keeps recording acquire/render/present phases
/// after those stages move to the present thread. Plain integers so the
/// packet types stay clock-library-free on every target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PresentTimings {
    pub after_acquire_ns: i64,
    pub after_render_ns: i64,
    pub after_present_ns: i64,
}

#[derive(Default)]
pub(crate) struct RenderReturns {
    pub(crate) scene: Option<CompositorScene>,
    pub(crate) frame_id: u64,
    pub(crate) outcome: PresentOutcome,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) timings: PresentTimings,
}

const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<FramePacket>();
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
