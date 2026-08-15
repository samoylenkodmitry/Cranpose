//! The present runtime (pipeline step 7b): the depth-one packet slot and
//! the thread that owns the present stage on Android.
//!
//! The producer ([`WgpuRenderer`](crate::WgpuRenderer)) publishes at most
//! one [`FramePacket`] into the runtime and may not publish another until
//! the packet's [`RenderReturns`] have been drained — backpressure lands
//! BEFORE the expensive update/lowering work, not after. The runtime
//! exclusively owns the `GpuRenderer` (constructed ON the present thread —
//! its `Rc` caches are thread-confined), the surface, surface
//! configuration, acquisition, encoding, submission and `present()`.
//!
//! Surface lifecycle changes arrive as typed [`PresentControl`] messages.
//! An invalidating control message (replace/reconfigure/drop) is
//! acknowledged ONLY AFTER any packet waiting in the slot has been
//! cancelled and its returns sent, so the producer can trust that nothing
//! stale renders once it publishes under the new epoch.
//!
//! The same state machine runs without a thread: tests construct
//! [`PresentState`] inline (via `WgpuRenderer::init_gpu_inline_for_tests`)
//! and pump the message queue by hand, so every protocol path is
//! observable deterministically.

use crate::display_clip::DisplayVisibleRegion;
use crate::frame_packet::{
    CancelReason, FramePacket, PresentOutcome, PresentTimings, RenderReturns, ReplayAck,
    ReplayFrameOps,
};
use crate::render::GpuRenderer;
use cranpose_render_common::software_text_raster::SoftwareTextFontSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{
    channel, sync_channel, Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError,
    TrySendError,
};
use std::sync::Arc;
use std::time::Duration;

/// Wakes the producer's event loop after the runtime sends returns (the
/// Android frame waker: `Send + Sync`, installed at runtime init).
pub(crate) type PresentWaker = Arc<dyn Fn() + Send + Sync>;

/// Producer-injected monotonic clock, nanoseconds. Injected (rather than
/// read locally) so present-thread timestamps live in the SAME clock
/// domain as the producer's frame telemetry; `None` leaves every
/// [`PresentTimings`] stage at `0`.
pub(crate) type PresentClock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// How long the producer waits for a control acknowledgement. The ack is
/// near-immediate in a healthy runtime (the present thread never blocks on
/// anything but `get_current_texture`), so this is generous on purpose:
/// hitting it means the runtime is wedged, which is logged, not fatal.
pub(crate) const CONTROL_ACK_TIMEOUT: Duration = Duration::from_secs(5);

/// One frame's [`ReplayAck`] plus the batch's emptied op buffers, sent to
/// the producer on its own channel as soon as the store consumed the ops —
/// BEFORE surface acquire and everything after it — so the confirmations
/// reach the very next frame's planning (see `consume_packet`).
pub(crate) type ReplayAckBatch = (ReplayAck, ReplayFrameOps);

/// Everything the present thread needs to construct its own
/// [`GpuRenderer`]; every member is `Send` (proven below).
pub(crate) struct PresentRuntimeInit {
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) queue: Arc<wgpu::Queue>,
    pub(crate) surface_format: wgpu::TextureFormat,
    pub(crate) adapter_backend: wgpu::Backend,
    pub(crate) adapter_downlevel: wgpu::DownlevelFlags,
    pub(crate) text_fonts: SoftwareTextFontSet,
    pub(crate) renderer_epoch: u64,
    pub(crate) store_feed_generation: u64,
    pub(crate) clock: Option<PresentClock>,
    /// The producer's current display visible region, inherited at
    /// construction exactly as `init_gpu` hands it to a fresh sync
    /// renderer; later changes arrive as
    /// [`PresentControl::SetDisplayVisibleRegion`].
    pub(crate) display_visible_region: DisplayVisibleRegion,
}

/// Surface-lifecycle control messages. Each invalidation carries the NEW
/// surface epoch stamped by the producer (`note_surface_reconfigured` is
/// the counter authority) and an ack the runtime fires only after the
/// waiting packet — if any — has been cancelled and its returns sent.
pub(crate) enum PresentControl {
    /// Install a (re)created surface; also delivers the initial surface.
    ReplaceSurface {
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
        surface_epoch: u64,
        ack: SyncSender<()>,
    },
    /// Reconfigure the current surface (resize) without replacing it.
    Reconfigure {
        config: wgpu::SurfaceConfiguration,
        surface_epoch: u64,
        ack: SyncSender<()>,
    },
    /// The window died (`TerminateWindow`): the surface drops, the
    /// renderer and its caches live on for the next window.
    DropSurface { ack: SyncSender<()> },
    /// The platform's display visible region changed (round-display
    /// providers etc.). Not invalidating and ack-less: the channel is FIFO,
    /// so it applies before any packet published after the producer's call
    /// — the same ordering the sync path's direct renderer call has.
    SetDisplayVisibleRegion { region: DisplayVisibleRegion },
    /// Test-only surrogate swapchain: packets render into an offscreen
    /// texture of this size instead of a real surface, so the protocol is
    /// testable headlessly. Invalidating, like `ReplaceSurface`.
    #[doc(hidden)]
    AttachOffscreenTargetForTests {
        width: u32,
        height: u32,
        ack: SyncSender<()>,
    },
    /// Drop the surface, drop the renderer (both on the present thread),
    /// exit the loop.
    Shutdown,
}

/// One message from the producer. Control and packets ride one FIFO so
/// the runtime can block on a single receiver; priority is enforced by the
/// waiting-packet slot: a packet is held, every already-queued control
/// message is processed against it (cancelling it if invalidating), and
/// only then does it render.
// A packet moves through here once per frame; boxing it to appease
// `large_enum_variant` would buy the smaller enum with a per-frame
// allocation, which the packet protocol exists to avoid.
#[allow(clippy::large_enum_variant)]
pub(crate) enum PresentMsg {
    Control(PresentControl),
    Packet(FramePacket),
}

/// Producer-readable snapshot of present-thread state. Every producer read
/// is `Relaxed`: each field is an independent monotone-ish signal, not a
/// synchronization point.
#[derive(Default)]
pub(crate) struct PresentStatus {
    /// Mirror of `GpuRenderer::needs_frame_warmup`, updated after every
    /// consumed packet — the producer reads it every loop iteration via
    /// `AppShell::needs_redraw` and must never touch the renderer.
    pub(crate) needs_frame_warmup: AtomicBool,
    /// Immutable capability, published once when the runtime constructs
    /// its renderer.
    pub(crate) replay_supported: AtomicBool,
    /// Lifetime count of packets that reached `PresentOutcome::Presented`.
    pub(crate) presented_frames: AtomicU64,
    /// The last consumed packet's outcome, encoded with its frame id — see
    /// [`encode_present_outcome`].
    pub(crate) last_present_outcome: AtomicU64,
    /// `frame_id` of the last packet whose draw errored (`0` = never).
    pub(crate) last_error_frame: AtomicU64,
}

/// Encode an outcome + frame id into one atomic word: outcome code in the
/// low 8 bits, frame id (wrapping) above.
pub(crate) fn encode_present_outcome(outcome: PresentOutcome, frame_id: u64) -> u64 {
    let code: u64 = match outcome {
        PresentOutcome::NotRun => 0,
        PresentOutcome::Presented => 1,
        PresentOutcome::Cancelled(CancelReason::RendererEpoch) => 2,
        PresentOutcome::Cancelled(CancelReason::SurfaceEpoch) => 3,
        PresentOutcome::Cancelled(CancelReason::Viewport) => 4,
        PresentOutcome::Cancelled(CancelReason::SurfaceUnavailable) => 5,
        PresentOutcome::Cancelled(CancelReason::DeviceError) => 6,
    };
    (frame_id << 8) | code
}

/// The present stage's state machine: one `GpuRenderer` (constructed here,
/// on whichever thread runs this), the current surface + configuration,
/// the runtime's own surface-epoch copy, and the depth-one waiting slot.
pub(crate) struct PresentState {
    gpu_renderer: GpuRenderer,
    device: Arc<wgpu::Device>,
    surface: Option<wgpu::Surface<'static>>,
    config: Option<wgpu::SurfaceConfiguration>,
    /// Test-only surrogate swapchain (see
    /// [`PresentControl::AttachOffscreenTargetForTests`]).
    offscreen_target: Option<(u32, u32)>,
    /// The renderer epoch this runtime was built under; packets from
    /// another instance cancel before any GPU work.
    renderer_epoch: u64,
    /// The runtime's copy of the surface epoch, updated by control
    /// messages; a waiting packet stamped with an older value cancels.
    surface_epoch: u64,
    /// THE depth-one slot: the packet received but not yet consumed.
    waiting_packet: Option<FramePacket>,
    returns_tx: SyncSender<RenderReturns>,
    /// The early replay-ack channel: one [`ReplayAckBatch`] per consumed
    /// Direct packet, sent BEFORE surface acquire so the producer's next
    /// planning sees this frame's confirmations (the confirmation-latency
    /// fix that made depth-one overlap pay — see `consume_packet`).
    acks_tx: SyncSender<ReplayAckBatch>,
    status: Arc<PresentStatus>,
    waker: PresentWaker,
    clock: Option<PresentClock>,
}

impl PresentState {
    pub(crate) fn new(
        init: PresentRuntimeInit,
        returns_tx: SyncSender<RenderReturns>,
        acks_tx: SyncSender<ReplayAckBatch>,
        status: Arc<PresentStatus>,
        waker: PresentWaker,
    ) -> Self {
        let PresentRuntimeInit {
            device,
            queue,
            surface_format,
            adapter_backend,
            adapter_downlevel,
            text_fonts,
            renderer_epoch,
            store_feed_generation,
            clock,
            display_visible_region,
        } = init;
        // The renderer is constructed HERE — on the present thread when
        // spawned, inline in tests — because its Rc caches are
        // thread-confined and must never move.
        let mut gpu_renderer = GpuRenderer::new(
            device.clone(),
            queue,
            surface_format,
            adapter_backend,
            adapter_downlevel,
            text_fonts,
            renderer_epoch,
            store_feed_generation,
        );
        // Inherited exactly as `init_gpu` hands the producer's stored
        // region to a fresh sync renderer.
        gpu_renderer.set_display_visible_region(display_visible_region);
        // `replay_supported` is immutable after construction: published
        // once, read by every producer-side packet build.
        status
            .replay_supported
            .store(gpu_renderer.replay_supported(), Ordering::Relaxed);
        Self {
            gpu_renderer,
            device,
            surface: None,
            config: None,
            offscreen_target: None,
            renderer_epoch,
            surface_epoch: 0,
            waiting_packet: None,
            returns_tx,
            acks_tx,
            status,
            waker,
            clock,
        }
    }

    /// The blocking runtime loop: park on the message queue while the slot
    /// is empty; while a packet waits, drain every already-queued message
    /// (control first, by construction) and only then consume the packet.
    pub(crate) fn run(mut self, rx: Receiver<PresentMsg>) {
        loop {
            let msg = if self.waiting_packet.is_some() {
                match rx.try_recv() {
                    Ok(msg) => Some(msg),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => break,
                }
            } else {
                match rx.recv() {
                    Ok(msg) => Some(msg),
                    Err(_) => break,
                }
            };
            match msg {
                Some(msg) => {
                    if !self.run_once(msg) {
                        break;
                    }
                }
                None => self.consume_waiting(),
            }
        }
        // Falling out of the loop drops the surface and the GpuRenderer on
        // this thread — the shutdown contract.
    }

    /// Process one message. Returns `false` when the runtime must exit.
    pub(crate) fn run_once(&mut self, msg: PresentMsg) -> bool {
        match msg {
            PresentMsg::Control(control) => self.handle_control(control),
            PresentMsg::Packet(packet) => {
                if self.waiting_packet.is_some() {
                    // Legal at depth two: the producer published N+1 while
                    // N still waited. Consume the older one first — FIFO
                    // order is the protocol, and neither packet may be
                    // overwritten or dropped.
                    self.consume_waiting();
                }
                self.waiting_packet = Some(packet);
                true
            }
        }
    }

    pub(crate) fn has_waiting_packet(&self) -> bool {
        self.waiting_packet.is_some()
    }

    /// Consume the waiting packet, if any: validate, acquire late, render,
    /// present, send returns.
    pub(crate) fn consume_waiting(&mut self) {
        if let Some(packet) = self.waiting_packet.take() {
            self.consume_packet(packet);
        }
    }

    fn handle_control(&mut self, control: PresentControl) -> bool {
        match control {
            PresentControl::ReplaceSurface {
                surface,
                config,
                surface_epoch,
                ack,
            } => {
                self.surface_epoch = surface_epoch;
                // Invalidation-before-ack: the waiting packet's returns
                // are sent BEFORE the producer's ack fires, so a producer
                // that publishes under the new epoch after the ack can
                // never race a stale render.
                self.cancel_waiting(CancelReason::SurfaceEpoch);
                surface.configure(&self.device, &config);
                self.surface = Some(surface);
                self.config = Some(config);
                self.offscreen_target = None;
                let _ = ack.send(());
                true
            }
            PresentControl::Reconfigure {
                config,
                surface_epoch,
                ack,
            } => {
                self.surface_epoch = surface_epoch;
                self.cancel_waiting(CancelReason::SurfaceEpoch);
                if let Some(surface) = self.surface.as_ref() {
                    surface.configure(&self.device, &config);
                }
                if self.offscreen_target.is_some() {
                    self.offscreen_target = Some((config.width, config.height));
                }
                self.config = Some(config);
                let _ = ack.send(());
                true
            }
            PresentControl::DropSurface { ack } => {
                // The surface dies; the renderer and its caches live on
                // for the next window. A waiting packet cannot render at
                // all — cancel it directly, no GPU touched.
                self.cancel_waiting(CancelReason::SurfaceUnavailable);
                self.surface = None;
                self.config = None;
                self.offscreen_target = None;
                let _ = ack.send(());
                true
            }
            PresentControl::SetDisplayVisibleRegion { region } => {
                // A packet published before this change must draw under
                // the region it was built for, exactly as on the sync path
                // (where the region call sits between two render calls) —
                // so the waiting packet consumes first, then the region
                // applies to everything after it.
                self.consume_waiting();
                self.gpu_renderer.set_display_visible_region(region);
                true
            }
            PresentControl::AttachOffscreenTargetForTests { width, height, ack } => {
                self.cancel_waiting(CancelReason::SurfaceEpoch);
                self.surface = None;
                self.config = None;
                self.offscreen_target = Some((width, height));
                let _ = ack.send(());
                true
            }
            PresentControl::Shutdown => {
                // A packet still waiting at shutdown is cancelled so its
                // buffers travel back if the producer is still listening.
                self.cancel_waiting(CancelReason::SurfaceUnavailable);
                false
            }
        }
    }

    /// The CPU-only validity gate, mirroring `GpuRenderer::render`'s head
    /// so a doomed packet never reaches `get_current_texture`.
    fn validate(&self, packet: &FramePacket) -> Option<CancelReason> {
        if packet.renderer_epoch != self.renderer_epoch {
            Some(CancelReason::RendererEpoch)
        } else if packet.surface_epoch != self.surface_epoch {
            Some(CancelReason::SurfaceEpoch)
        } else if let Some((width, height)) = self.target_size() {
            (packet.viewport != (width, height)).then_some(CancelReason::Viewport)
        } else {
            Some(CancelReason::SurfaceUnavailable)
        }
    }

    fn target_size(&self) -> Option<(u32, u32)> {
        if self.surface.is_some() {
            self.config
                .as_ref()
                .map(|config| (config.width, config.height))
        } else {
            self.offscreen_target
        }
    }

    /// Cancel the waiting packet (if any) with the validation head's
    /// reason, falling back to `fallback` when the packet would otherwise
    /// still validate — an invalidating control message must cancel it
    /// regardless.
    fn cancel_waiting(&mut self, fallback: CancelReason) {
        if let Some(packet) = self.waiting_packet.take() {
            let reason = self.validate(&packet).unwrap_or(fallback);
            self.cancel_packet(packet, reason);
        }
    }

    /// Refuse a packet without touching the GPU. The recycled
    /// confirmations buffer is adopted into the store first (capacity must
    /// not leak with the packet), then the 7a cancel path returns every
    /// buffer through `returns`.
    fn cancel_packet(&mut self, mut packet: FramePacket, reason: CancelReason) {
        if let Some(confirmations) = packet.recycled_confirmations.take() {
            self.gpu_renderer
                .restore_replay_ack_confirmations(confirmations);
        }
        let mut returns = RenderReturns::default();
        let _ = GpuRenderer::cancel_packet(packet, reason, &mut returns);
        self.finish_returns(returns);
    }

    fn consume_packet(&mut self, mut packet: FramePacket) {
        if let Some(reason) = self.validate(&packet) {
            self.cancel_packet(packet, reason);
            return;
        }
        // EARLY REPLAY ACK — the fix for the confirmation latency that
        // sank the first 7b attempt. The store consumes the (validated)
        // packet's replay plan NOW, before `get_current_texture` can block
        // on the swapchain, and the ack leaves immediately on its own
        // channel: the producer, which starts building frame N+1 the
        // moment it publishes N, drains this channel right before N+1's
        // planning and sees N's confirmations — the same one-frame
        // latency the synchronous path has. Left to ride the returns (as
        // parked), every capture took TWO frames to confirm and each
        // recapture re-materialized ~17k primitives instead of replaying
        // ~35 retained spans. The recycled confirmations buffer is
        // adopted first so the store's ack reuses its capacity.
        if let Some(confirmations) = packet.recycled_confirmations.take() {
            self.gpu_renderer
                .restore_replay_ack_confirmations(confirmations);
        }
        if let Some(batch) = self.gpu_renderer.take_replay_ack_early(&mut packet) {
            self.send_ack(batch);
        }
        let (width, height) = packet.viewport;
        if self.surface.is_some() {
            self.render_to_surface(packet, width, height);
        } else if self.offscreen_target.is_some() {
            self.render_offscreen(packet, width, height);
        } else {
            // Unreachable: `validate` refused surfaceless packets above.
            self.cancel_packet(packet, CancelReason::SurfaceUnavailable);
        }
    }

    /// Ship one early [`ReplayAckBatch`] to the producer. No waker: an ack
    /// gates nothing — the producer that is awake folds it in before its
    /// next planning, and one that is asleep has nothing to plan.
    fn send_ack(&mut self, batch: ReplayAckBatch) {
        match self.acks_tx.try_send(batch) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                // Impossible under the depth-one credit protocol (at most
                // one ack per in-flight packet). Dropping loses recycled
                // buffer capacity and one frame's confirmations — the
                // planner re-requests, correctness holds; scream so the
                // protocol break is seen.
                log::error!("[present-runtime] ack channel full; depth-one credit violated");
            }
            Err(TrySendError::Disconnected(_)) => {
                // Producer gone (shutdown race); nothing to confirm into.
            }
        }
    }

    /// LATE ACQUIRE: the swapchain texture is acquired only after the
    /// epoch validation, immediately before the render call — the move of
    /// the acquire out of the producer loop that step 7b exists for.
    fn render_to_surface(&mut self, packet: FramePacket, width: u32, height: u32) {
        let frame = match self.acquire_with_one_retry() {
            Some(frame) => frame,
            None => {
                // Acquire failed past its one reconfigure retry: the
                // packet's buffers must return — never drop.
                self.cancel_packet(packet, CancelReason::SurfaceUnavailable);
                return;
            }
        };
        let after_acquire_ns = self.now();
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut returns = RenderReturns::default();
        let result = self.gpu_renderer.render(
            &view,
            width,
            height,
            packet,
            self.surface_epoch,
            &mut returns,
        );
        let after_render_ns = self.now();
        // Present even on a draw error, mirroring the old sync loop: an
        // acquired frame that is never presented can stall the swapchain.
        frame.present();
        returns.timings = PresentTimings {
            after_acquire_ns,
            after_render_ns,
            after_present_ns: self.now(),
        };
        if let Err(error) = result {
            log::error!("[present-runtime] render error: {error}");
            self.status
                .last_error_frame
                .store(returns.frame_id.max(1), Ordering::Relaxed);
        }
        self.finish_returns(returns);
    }

    /// The `current_surface_texture` semantics, runtime-owned: Ready →
    /// render+present; Lost/Outdated → reconfigure with the CURRENT config
    /// and retry ONCE; Timeout/Occluded/Validation → give up this frame.
    fn acquire_with_one_retry(&mut self) -> Option<wgpu::SurfaceTexture> {
        let surface = self.surface.as_ref()?;
        match Self::acquire(surface) {
            AcquireOutcome::Ready(frame) => Some(frame),
            AcquireOutcome::Skip => None,
            AcquireOutcome::Reconfigure => {
                let config = self.config.as_ref()?;
                surface.configure(&self.device, config);
                match Self::acquire(surface) {
                    AcquireOutcome::Ready(frame) => Some(frame),
                    _ => None,
                }
            }
        }
    }

    fn acquire(surface: &wgpu::Surface<'static>) -> AcquireOutcome {
        match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => AcquireOutcome::Ready(frame),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                log::debug!("[present-runtime] surface suboptimal, rendering current frame");
                AcquireOutcome::Ready(frame)
            }
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                AcquireOutcome::Reconfigure
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                log::debug!("[present-runtime] surface timeout, skipping frame");
                AcquireOutcome::Skip
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                log::debug!("[present-runtime] surface occluded, skipping frame");
                AcquireOutcome::Skip
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                log::error!("[present-runtime] surface validation error, skipping frame");
                AcquireOutcome::Skip
            }
        }
    }

    /// Test-only: consume a packet against the offscreen surrogate target
    /// so the full render path runs headlessly.
    fn render_offscreen(&mut self, packet: FramePacket, width: u32, height: u32) {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Present Runtime Offscreen Test Target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.gpu_renderer.surface_format(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let after_acquire_ns = self.now();
        let mut returns = RenderReturns::default();
        let result = self.gpu_renderer.render(
            &view,
            width,
            height,
            packet,
            self.surface_epoch,
            &mut returns,
        );
        let after_render_ns = self.now();
        returns.timings = PresentTimings {
            after_acquire_ns,
            after_render_ns,
            after_present_ns: self.now(),
        };
        if let Err(error) = result {
            log::error!("[present-runtime] offscreen render error: {error}");
            self.status
                .last_error_frame
                .store(returns.frame_id.max(1), Ordering::Relaxed);
        }
        self.finish_returns(returns);
    }

    /// The tail every consumed packet shares: refresh the warmup snapshot,
    /// record the outcome, send the returns, wake the producer.
    fn finish_returns(&mut self, returns: RenderReturns) {
        self.status
            .needs_frame_warmup
            .store(self.gpu_renderer.needs_frame_warmup(), Ordering::Relaxed);
        if returns.outcome == PresentOutcome::Presented {
            self.status.presented_frames.fetch_add(1, Ordering::Relaxed);
        }
        self.status.last_present_outcome.store(
            encode_present_outcome(returns.outcome, returns.frame_id),
            Ordering::Relaxed,
        );
        match self.returns_tx.try_send(returns) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                // Impossible under the depth-one credit protocol (at most
                // one in-flight packet plus one invalidation cancel).
                // Dropping the returns loses recycled buffers, not
                // correctness; scream so the protocol break is seen.
                log::error!("[present-runtime] returns channel full; depth-one credit violated");
            }
            Err(TrySendError::Disconnected(_)) => {
                // Producer gone (shutdown race); nothing to recycle into.
            }
        }
        (self.waker)();
    }

    fn now(&self) -> i64 {
        self.clock.as_ref().map(|clock| clock()).unwrap_or(0)
    }

    /// Test inspector: the store-side ack confirmations buffer's capacity,
    /// proving the threaded recycle path hands capacity back to the store.
    pub(crate) fn store_ack_confirmations_capacity(&self) -> usize {
        self.gpu_renderer.replay_ack_confirmations_capacity()
    }
}

enum AcquireOutcome {
    Ready(wgpu::SurfaceTexture),
    Reconfigure,
    Skip,
}

/// The producer's handle to a spawned (or inline) present runtime: the
/// message sender, the returns receiver, the shared status snapshot and
/// the local outstanding-packet count that implements depth-one credit.
pub(crate) struct PresentHandle {
    msg_tx: Sender<PresentMsg>,
    returns_rx: Receiver<RenderReturns>,
    /// Early replay acks, one per consumed Direct packet, arriving ahead
    /// of the packet's returns (see `PresentState::consume_packet`).
    acks_rx: Receiver<ReplayAckBatch>,
    status: Arc<PresentStatus>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// Packets published and not yet drained. Credit = `outstanding < 1`:
    /// the producer must drain a frame's returns before building the next.
    outstanding: u32,
}

impl PresentHandle {
    /// Spawn the runtime thread; the `GpuRenderer` is constructed on it.
    pub(crate) fn spawn(init: PresentRuntimeInit, waker: PresentWaker) -> Result<Self, String> {
        let (msg_tx, msg_rx) = channel::<PresentMsg>();
        // Capacity 2: one rendering and one invalidation-cancelled packet
        // can both complete before the producer drains.
        let (returns_tx, returns_rx) = sync_channel::<RenderReturns>(2);
        // Acks: at most one per in-flight packet; 4 leaves headroom for a
        // drain that raced a publish.
        let (acks_tx, acks_rx) = sync_channel::<ReplayAckBatch>(4);
        let status = Arc::new(PresentStatus::default());
        let thread_status = Arc::clone(&status);
        let thread = std::thread::Builder::new()
            .name("cranpose-present".to_string())
            .spawn(move || {
                PresentState::new(init, returns_tx, acks_tx, thread_status, waker).run(msg_rx);
            })
            .map_err(|error| format!("failed to spawn present thread: {error}"))?;
        Ok(Self {
            msg_tx,
            returns_rx,
            acks_rx,
            status,
            thread: Some(thread),
            outstanding: 0,
        })
    }

    /// Build the runtime WITHOUT a thread: the state machine and its
    /// message receiver are handed back for the test to pump by hand.
    pub(crate) fn new_inline(
        init: PresentRuntimeInit,
        waker: PresentWaker,
    ) -> (Self, PresentState, Receiver<PresentMsg>) {
        let (msg_tx, msg_rx) = channel::<PresentMsg>();
        let (returns_tx, returns_rx) = sync_channel::<RenderReturns>(2);
        let (acks_tx, acks_rx) = sync_channel::<ReplayAckBatch>(4);
        let status = Arc::new(PresentStatus::default());
        let state = PresentState::new(init, returns_tx, acks_tx, Arc::clone(&status), waker);
        (
            Self {
                msg_tx,
                returns_rx,
                acks_rx,
                status,
                thread: None,
                outstanding: 0,
            },
            state,
            msg_rx,
        )
    }

    pub(crate) fn status(&self) -> &PresentStatus {
        &self.status
    }

    /// One packet rendering AND one waiting (tofable §4) — the credit that
    /// makes the stages overlap. Allowing only one packet in flight would
    /// idle the producer for the whole of present (acquire, encode, submit,
    /// vsync) and idle the present thread for the whole of the next
    /// producer frame: the serial chain the pipeline exists to break, with
    /// a thread hop added. Two lets the producer lower frame N+1 while the
    /// present thread draws N, so throughput becomes max(producer, present)
    /// instead of their sum. It is still bounded: the producer stalls at
    /// two, so it can never run away from the screen.
    pub(crate) fn has_credit(&self) -> bool {
        self.outstanding < 2
    }

    /// Publish a packet into the depth-one slot. On failure (runtime gone)
    /// the packet is handed back — boxed, cold path only — so the caller
    /// can recover its buffers.
    pub(crate) fn publish(&mut self, packet: FramePacket) -> Result<(), Box<FramePacket>> {
        debug_assert!(self.has_credit(), "publish without credit");
        match self.msg_tx.send(PresentMsg::Packet(packet)) {
            Ok(()) => {
                self.outstanding += 1;
                Ok(())
            }
            Err(error) => {
                let PresentMsg::Packet(packet) = error.0 else {
                    unreachable!("publish only sends packets");
                };
                Err(Box::new(packet))
            }
        }
    }

    /// Non-blocking drain of one returns value.
    pub(crate) fn try_drain(&mut self) -> Option<RenderReturns> {
        match self.returns_rx.try_recv() {
            Ok(returns) => {
                self.outstanding = self.outstanding.saturating_sub(1);
                Some(returns)
            }
            Err(_) => None,
        }
    }

    /// Non-blocking drain of one early replay ack. Does NOT release
    /// publish credit — the ack precedes its frame's returns, which still
    /// carry the scene buffers and the outcome.
    pub(crate) fn try_drain_ack(&mut self) -> Option<ReplayAckBatch> {
        self.acks_rx.try_recv().ok()
    }

    /// Route a display-visible-region change to the present thread's
    /// renderer. Ack-less: FIFO ordering against later publishes is the
    /// contract (see [`PresentControl::SetDisplayVisibleRegion`]).
    pub(crate) fn send_display_visible_region(&self, region: DisplayVisibleRegion) {
        if self
            .msg_tx
            .send(PresentMsg::Control(
                PresentControl::SetDisplayVisibleRegion { region },
            ))
            .is_err()
        {
            log::error!("[present-runtime] set display visible region: runtime is gone");
        }
    }

    /// Send a control message and wait (bounded) for its acknowledgement.
    /// Returns whether the ack arrived; a timeout is logged and survivable
    /// (the producer's epoch bump already protects correctness).
    pub(crate) fn send_control_and_wait(
        &self,
        build: impl FnOnce(SyncSender<()>) -> PresentControl,
        what: &str,
    ) -> bool {
        let ack_rx = match self.send_control_unacked(build) {
            Some(ack_rx) => ack_rx,
            None => {
                log::error!("[present-runtime] {what}: runtime is gone");
                return false;
            }
        };
        match ack_rx.recv_timeout(CONTROL_ACK_TIMEOUT) {
            Ok(()) => true,
            Err(RecvTimeoutError::Timeout) => {
                log::error!(
                    "[present-runtime] {what}: no ack within {CONTROL_ACK_TIMEOUT:?}; \
                     present thread wedged?"
                );
                false
            }
            Err(RecvTimeoutError::Disconnected) => {
                log::error!("[present-runtime] {what}: runtime exited before ack");
                false
            }
        }
    }

    /// Send a control message and hand back the ack receiver without
    /// waiting — the inline test mode's building block (there is no thread
    /// to ack until the test pumps).
    pub(crate) fn send_control_unacked(
        &self,
        build: impl FnOnce(SyncSender<()>) -> PresentControl,
    ) -> Option<Receiver<()>> {
        let (ack_tx, ack_rx) = sync_channel::<()>(1);
        self.msg_tx
            .send(PresentMsg::Control(build(ack_tx)))
            .ok()
            .map(|_| ack_rx)
    }

    /// Send `Shutdown` and join the thread (spawned mode). Pending returns
    /// are left to the caller to drain BEFORE calling this.
    pub(crate) fn shutdown(&mut self) {
        let _ = self
            .msg_tx
            .send(PresentMsg::Control(PresentControl::Shutdown));
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                log::error!("[present-runtime] present thread panicked");
            }
        }
    }
}

impl Drop for PresentHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Compile-time proof that everything crossing into the present thread is
/// `Send`, member by member (mirrors `frame_packet.rs`).
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<PresentRuntimeInit>();
    assert_send::<PresentControl>();
    assert_send::<PresentMsg>();
    assert_send::<PresentWaker>();
    assert_send::<Option<PresentClock>>();
    assert_send::<Arc<PresentStatus>>();
    assert_send::<SyncSender<RenderReturns>>();
    assert_send::<SyncSender<ReplayAckBatch>>();
    assert_send::<DisplayVisibleRegion>();
};
