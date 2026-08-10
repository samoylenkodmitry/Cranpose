//! The producer→present frame boundary (pipeline step 4).
//!
//! A [`FramePacket`] is everything the present stage needs to render one
//! frame: the fully lowered, owned scene tree plus the frame scalars. The
//! producer builds it after lowering; the present side consumes it and
//! returns the scene buffers for recycling. Today both happen synchronously
//! on one thread; the packet type is the contract that lets a later step
//! move consumption to a present thread without changing what crosses the
//! boundary.
//!
//! Every payload member is proven `Send` at compile time below — a
//! regression that reintroduces a thread-bound member (an `Rc`, a raw
//! pointer, a borrowed graph node) fails the build here rather than at the
//! future channel.

use crate::normalized_scene::{ChildLayerComposite, CollectedLayer, LoweredChildSource};
use crate::scene::{
    BackdropLayer, ColorPatch, CompositorScene, DrawOp, DrawShape, EffectLayer, ImageDraw,
    PendingFeedCapture, RetainedDraw, ShadowDraw, TextDraw,
};
use cranpose_core::NodeId;
use cranpose_render_common::graph::{DrawCommandId, ProjectiveTransform};
use cranpose_ui_graphics::{GraphicsLayer, Rect, RenderEffect};

/// One frame's replay plan, emitted by the producer-side planner
/// ([`ShapeReplayState::take_frame_ops`](crate::shape_replay::ShapeReplayState))
/// when the packet is built and consumed by the present-side store
/// (`GpuRenderer::consume_replay_ops`) just before the packet renders. This
/// is the ONLY producer→store replay channel; the store answers with a
/// [`ReplayAck`].
#[derive(Default)]
pub(crate) struct ReplayFrameOps {
    /// The retained-feed generation the plan was made under. The store
    /// drops the batch whole on a mismatch: every capture/patch/release in
    /// it names slots of a universe the store no longer holds.
    pub(crate) generation: u64,
    /// The planner's frame ordinal at plan time; the store's defensive
    /// staleness reference for `captures` (each capture is stamped with the
    /// ordinal it was queued on).
    pub(crate) frame: u64,
    pub(crate) captures: Vec<PendingFeedCapture>,
    pub(crate) color_patches: Vec<ColorPatch>,
    pub(crate) releases: Vec<u32>,
}

/// One confirmed capture: the span's identity key `(command, span slot)`
/// mapped to the physical GPU slot the store retained it in.
pub(crate) type ReplayConfirmation = ((DrawCommandId, u32), u32);

/// The store's answer to one [`ReplayFrameOps`] batch, applied by the
/// planner ([`ShapeReplayState::apply_ack`](crate::shape_replay::ShapeReplayState))
/// before the next frame's planning. Travels with the batch's emptied
/// buffers (capacity intact) so neither side allocates per frame.
pub(crate) struct ReplayAck {
    /// The generation the confirmations are stamped with — the slot
    /// universe they verifiably exist in.
    pub(crate) generation: u64,
    pub(crate) confirmations: Vec<ReplayConfirmation>,
}

/// The lowered root a [`FramePacket`] carries: either today's direct path
/// (the root renders straight to the surface) or a root layer surface (the
/// old non-direct fallback, now lowered producer-side too).
pub(crate) enum PacketRoot {
    /// The lowered root scene plus owned child-layer composites, rendered
    /// by `render_root_direct`. Boxed (like `Surface`) so moving the packet
    /// moves one pointer instead of the payload struct.
    Direct(Box<CollectedLayer>),
    /// A root that needs its own layer surface (effects, backdrops or
    /// shadows on the root), rendered by the snapshot-consuming
    /// `render_layer_surface` body plus the root composite tail.
    Surface(Box<RootSurfacePacket>),
}

/// Producer lowering of a non-direct root plus a snapshot of every value
/// the present-side root composite tail used to read off `graph.root` —
/// the present backend must never touch the retained graph.
pub(crate) struct RootSurfacePacket {
    /// The root's collection-time snapshot, from `lower_layer_node` (the
    /// same lowering the render-side root path used to run).
    pub(crate) lowered: ChildLayerComposite,
    /// The root's collected content; dropped unused on a raster-cache hit.
    pub(crate) source: LoweredChildSource,
    /// `graph.root.transform_to_parent` — maps the root surface and the
    /// backdrop/shadow rects into surface space.
    pub(crate) transform_to_parent: ProjectiveTransform,
    /// `graph.root.node_id`, for the root backdrop layer.
    pub(crate) node_id: Option<NodeId>,
    /// `graph.root.backdrop().cloned()` — drives the composite-target
    /// decision and the root backdrop apply.
    pub(crate) backdrop: Option<RenderEffect>,
    /// `graph.root.graphics_layer` — read for `shadow_elevation` and by
    /// `push_layer_shadow`.
    pub(crate) graphics_layer: GraphicsLayer,
    /// `graph.root.local_bounds` — the backdrop/shadow source rect.
    pub(crate) local_bounds: Rect,
    /// `graph.root.clip_rect()` — the root backdrop layer's clip.
    pub(crate) clip_rect: Option<Rect>,
    /// `graph.root.shadow_clip` — the root shadow's clip.
    pub(crate) shadow_clip: Option<Rect>,
}

/// One frame's producer output.
pub(crate) struct FramePacket {
    /// Monotone frame sequence number, stamped by the producer. Consumed by
    /// present-side telemetry today; the lease/ack replay protocol keys off
    /// it when the stages split.
    pub(crate) frame_id: u64,
    /// Physical surface size the payload was lowered for.
    pub(crate) viewport: (u32, u32),
    /// Root scale the payload was lowered for.
    pub(crate) root_scale: f32,
    /// The lowered root (direct or root-surface).
    pub(crate) root: PacketRoot,
    /// The dev overlay, lowered producer-side when a dev overlay graph is
    /// set; the present backend only renders it.
    pub(crate) overlay: Option<CollectedLayer>,
    /// The frame's replay plan. Unconditional so the packet has one
    /// architecture; wasm has no retained replay path, and Surface frames
    /// never touch the planner — both carry the empty default, which the
    /// present store must NOT consume (its generation 0 would count a
    /// false generation drop).
    pub(crate) replay: ReplayFrameOps,
    /// Producer-side text layout cache size at packet build time, carried
    /// for the present backend's frame stats — the present call tree holds
    /// no text layout state to read it from.
    pub(crate) text_cache_len: usize,
}

/// What the present stage hands back to the producer after consuming a
/// frame: the rendered packet's scene buffers for recycling and the store's
/// [`ReplayAck`] (with the batch's emptied op buffers) for the planner.
/// The producer folds it in via `RendererFrontend::apply_returns`; the
/// present backend fills it instead of writing producer state itself.
#[derive(Default)]
pub(crate) struct RenderReturns {
    /// The rendered direct-root scene, returned so its draw vectors are
    /// reused instead of reallocated every frame. `None` when the frame
    /// rendered a Surface root (its scene is not pooled, as before) or the
    /// direct draw failed.
    pub(crate) scene: Option<CompositorScene>,
    /// The store's answer to the packet's replay plan plus the recycled
    /// op buffers. `None` when no packet was consumed; always `None` on
    /// wasm, which has no retained replay path.
    pub(crate) ack: Option<(ReplayAck, ReplayFrameOps)>,
}

/// Compile-time proof that the packet and every member chain can cross a
/// thread boundary. Listed individually so a regression names the exact
/// type that broke instead of one opaque `FramePacket: !Send` error.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<FramePacket>();
    assert_send::<PacketRoot>();
    assert_send::<RootSurfacePacket>();
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
    assert_send::<ReplayFrameOps>();
    assert_send::<ReplayAck>();
    assert_send::<RenderReturns>();
    assert_send::<ColorPatch>();
    assert_send::<PendingFeedCapture>();
};
