//! The producer→present frame boundary (pipeline step 4).
//!
//! A [`FramePacket`] is everything the present stage needs to render one
//! frame's direct root: the fully lowered, owned scene tree plus the frame
//! scalars. The producer builds it after lowering; the present side consumes
//! it and returns the scene buffers for recycling. Today both happen
//! synchronously on one thread; the packet type is the contract that lets a
//! later step move consumption to a present thread without changing what
//! crosses the boundary.
//!
//! Every payload member is proven `Send` at compile time below — a
//! regression that reintroduces a thread-bound member (an `Rc`, a raw
//! pointer, a borrowed graph node) fails the build here rather than at the
//! future channel.

use crate::normalized_scene::{ChildLayerComposite, CollectedLayer, LoweredChildSource};
use crate::scene::{
    BackdropLayer, CompositorScene, DrawOp, DrawShape, EffectLayer, ImageDraw, RetainedDraw,
    ShadowDraw, TextDraw,
};

/// One frame's producer output for the direct-root path.
pub(crate) struct FramePacket {
    /// Monotone frame sequence number, stamped by the producer. Consumed by
    /// present-side telemetry today; the lease/ack replay protocol keys off
    /// it when the stages split.
    pub(crate) frame_id: u64,
    /// Physical surface size the payload was lowered for.
    pub(crate) viewport: (u32, u32),
    /// Root scale the payload was lowered for.
    pub(crate) root_scale: f32,
    /// The lowered root scene plus owned child-layer composites.
    pub(crate) root: CollectedLayer,
}

/// Compile-time proof that the packet and every member chain can cross a
/// thread boundary. Listed individually so a regression names the exact
/// type that broke instead of one opaque `FramePacket: !Send` error.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<FramePacket>();
    assert_send::<CollectedLayer>();
    assert_send::<ChildLayerComposite>();
    assert_send::<LoweredChildSource>();
    assert_send::<CompositorScene>();
    assert_send::<DrawShape>();
    assert_send::<ImageDraw>();
    assert_send::<TextDraw>();
    assert_send::<ShadowDraw>();
    assert_send::<DrawOp>();
    assert_send::<EffectLayer>();
    assert_send::<BackdropLayer>();
    assert_send::<RetainedDraw>();
};
