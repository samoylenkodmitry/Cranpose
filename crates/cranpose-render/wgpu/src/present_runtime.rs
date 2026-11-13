use std::{
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering},
        mpsc::{
            Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError, TrySendError, channel,
            sync_channel,
        },
    },
    time::Duration,
};

use crate::{
    frame_packet::{
        CancelReason, CaptureReply, FramePacket, FrameSceneStorage, PresentOutcome, PresentTimings,
        RenderReturns,
    },
    render::{FrameOutput, FrameSubmit, GpuRenderer, GpuRendererInit},
};

pub(crate) type PresentWaker = Arc<dyn Fn() + Send + Sync>;

pub(crate) type PresentClock = Arc<dyn Fn() -> i64 + Send + Sync>;

pub(crate) type BoxedPresentObserver = Box<dyn crate::PresentObserver>;

pub(crate) const CONTROL_ACK_TIMEOUT: Duration = Duration::from_secs(5);

// A capture waits behind the frames before it, the pipelines its effects
// build included.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) struct PresentRuntimeInit {
    pub(crate) gpu: GpuRendererInit,
    pub(crate) clock: Option<PresentClock>,
    pub(crate) observer: Option<BoxedPresentObserver>,
}

pub(crate) enum PresentControl {
    // Nothing waits for it: packets sent after it run on the new surface,
    // which first shows `background`.
    ReplaceSurface {
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
        surface_epoch: u64,
        background: wgpu::Color,
    },
    // `ack`, when given, hears once the surface has the new configuration.
    Reconfigure {
        config: wgpu::SurfaceConfiguration,
        surface_epoch: u64,
        ack: Option<SyncSender<()>>,
    },
    // Whether the window shows nothing of the surface. While it does, the
    // thread takes no image: a Metal drawable can stall a second then.
    Occluded(bool),
    // The renderer moves to a new device, the last one lost; the surface
    // stays and is configured on the new device.
    ReplaceDevice(Box<GpuRendererInit>),
    // Draws `packet` into an image of `viewport` and hands its pixels back.
    Capture {
        packet: FramePacket,
        viewport: (u32, u32),
        reply: SyncSender<CaptureReply>,
    },
    // Hands the surface back through `reply`; `None` when there was none.
    TakeSurface {
        reply: SyncSender<Option<wgpu::Surface<'static>>>,
    },
    #[doc(hidden)]
    AttachOffscreenTargetForTests {
        width: u32,
        height: u32,
        ack: SyncSender<()>,
    },
    // Destroys the device between two frames and waits for its loss to be
    // reported, as a robot's loss drill asks.
    LoseDevice {
        ack: SyncSender<()>,
    },
    Shutdown,
}

pub(crate) enum PresentMsg {
    Control(PresentControl),
    Packet(FramePacket),
}

#[derive(Default)]
pub(crate) struct PresentStatus {
    pub(crate) last_frame_stats: Mutex<Option<crate::gpu_stats::FrameStatsSnapshot>>,
    pub(crate) needs_frame_warmup: AtomicBool,
    /// Whether the last frame drew a placeholder for an effect whose
    /// pipelines compile.
    pub(crate) drew_placeholder: AtomicBool,
    /// The current renderer's compiler account, to tell whether pipelines a
    /// placeholder waits for still compile.
    pub(crate) landing: Mutex<Option<Arc<crate::pipeline_compiler::Landing>>>,
    // Whether the pipeline cache file held an earlier launch's first screen.
    pub(crate) first_screen_recorded: AtomicBool,
    pub(crate) presented_frames: AtomicU64,
    /// Frames handed back to the producer, whatever their outcome.
    pub(crate) returned_frames: AtomicU64,
    pub(crate) placeholder_frames: AtomicU64,
    pub(crate) last_present_outcome: AtomicU64,
    pub(crate) last_error_frame: AtomicU64,
    /// The present thread's OS id once it has started, `0` before.
    pub(crate) thread_id: AtomicI32,
}

impl PresentStatus {
    // Whether the producer must draw another frame for the renderer: the
    // present thread's account of its last frame, or a pipeline that landed
    // for a placeholder since. The present thread reads the landing before
    // it writes its account, so a pipeline that lands in between is only in
    // the landing, and the app it woke reads it there.
    pub(crate) fn warmup_due(&self) -> bool {
        self.needs_frame_warmup.load(Ordering::Relaxed)
            || self.landing_is(crate::pipeline_compiler::Landing::landed)
    }

    // Whether the current renderer's compiler account says `ask`.
    pub(crate) fn landing_is(&self, ask: fn(&crate::pipeline_compiler::Landing) -> bool) -> bool {
        self.landing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_deref()
            .is_some_and(ask)
    }
}

pub(crate) fn encode_present_outcome(outcome: PresentOutcome, frame_id: u64) -> u64 {
    let code: u64 = match outcome {
        PresentOutcome::NotRun => 0,
        PresentOutcome::Presented => 1,
        PresentOutcome::Cancelled(CancelReason::RendererEpoch) => 2,
        PresentOutcome::Cancelled(CancelReason::SurfaceEpoch) => 3,
        PresentOutcome::Cancelled(CancelReason::Viewport) => 4,
        PresentOutcome::Cancelled(CancelReason::SurfaceUnavailable) => 5,
        PresentOutcome::Cancelled(CancelReason::DeviceError) => 6,
        PresentOutcome::Unchanged => 7,
        PresentOutcome::Cancelled(CancelReason::NeedsWhole) => 8,
        PresentOutcome::Cancelled(CancelReason::DeviceLost) => 9,
    };
    (frame_id << 8) | code
}

pub(crate) struct PresentState {
    gpu_renderer: GpuRenderer,
    submit: FrameSubmit,
    device: Arc<wgpu::Device>,
    surface: Option<wgpu::Surface<'static>>,
    config: Option<wgpu::SurfaceConfiguration>,
    offscreen_target: Option<(u32, u32)>,
    renderer_epoch: u64,
    surface_epoch: u64,
    waiting_packet: Option<FramePacket>,
    occluded: bool,
    background: wgpu::Color,
    returns_tx: SyncSender<RenderReturns>,
    status: Arc<PresentStatus>,
    waker: PresentWaker,
    clock: Option<PresentClock>,
    observer: Option<BoxedPresentObserver>,
}

impl PresentState {
    pub(crate) fn new(
        init: PresentRuntimeInit,
        returns_tx: SyncSender<RenderReturns>,
        status: Arc<PresentStatus>,
        waker: PresentWaker,
    ) -> Self {
        let PresentRuntimeInit {
            gpu,
            clock,
            observer,
        } = init;
        let renderer_epoch = gpu.renderer_epoch;
        let (gpu_renderer, submit) = Self::renderer(gpu, &status, &waker);
        Self {
            device: Arc::clone(&gpu_renderer.device),
            renderer_epoch,
            gpu_renderer,
            submit,
            surface: None,
            config: None,
            offscreen_target: None,
            surface_epoch: 0,
            waiting_packet: None,
            occluded: false,
            background: crate::render::CLEAR_COLOR,
            returns_tx,
            status,
            waker,
            clock,
            observer,
        }
    }

    // The renderer `gpu` describes, its compiler account in `status`.
    fn renderer(
        gpu: GpuRendererInit,
        status: &PresentStatus,
        waker: &PresentWaker,
    ) -> (GpuRenderer, FrameSubmit) {
        let submit = FrameSubmit::new(&gpu);
        let gpu_renderer = GpuRenderer::new(gpu);
        *status
            .landing
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = gpu_renderer.pipelines.landing();
        status
            .first_screen_recorded
            .store(gpu_renderer.first_screen_recorded(), Ordering::Relaxed);
        // The landing tells the app the frame is due: the present thread
        // alone writes its account.
        gpu_renderer.pipelines.wake_on_landing(Box::new({
            let waker = Arc::clone(waker);
            move || waker()
        }));
        (gpu_renderer, submit)
    }

    pub(crate) fn run(mut self, rx: Receiver<PresentMsg>) {
        loop {
            let msg = if self.waiting_packet.is_some() && !self.occluded {
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
    }

    pub(crate) fn run_once(&mut self, msg: PresentMsg) -> bool {
        match msg {
            PresentMsg::Control(control) => self.handle_control(control),
            PresentMsg::Packet(packet) => {
                // While occluded, a newer packet stands in for the one that
                // waits, which goes back undrawn.
                match self.waiting_packet.take() {
                    Some(older) if self.occluded => {
                        self.cancel_packet(older, CancelReason::SurfaceUnavailable);
                    }
                    Some(older) => self.consume_packet(older),
                    None => {}
                }
                self.waiting_packet = Some(packet);
                true
            }
        }
    }

    pub(crate) fn has_waiting_packet(&self) -> bool {
        self.waiting_packet.is_some()
    }

    pub(crate) fn consume_waiting(&mut self) {
        if self.occluded {
            return;
        }
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
                background,
            } => {
                self.surface_epoch = surface_epoch;
                self.background = background;
                self.cancel_waiting(CancelReason::SurfaceEpoch);
                surface.configure(&self.device, &config);
                self.surface = Some(surface);
                self.config = Some(config);
                self.offscreen_target = None;
                self.present_placeholder_frame();
                true
            }
            PresentControl::Reconfigure {
                config,
                surface_epoch,
                ack,
            } => {
                self.surface_epoch = surface_epoch;
                self.cancel_waiting(CancelReason::SurfaceEpoch);
                self.reconfigure(&config);
                if self.offscreen_target.is_some() {
                    self.offscreen_target = Some((config.width, config.height));
                }
                self.config = Some(config);
                if let Some(ack) = ack {
                    let _ = ack.send(());
                }
                true
            }
            PresentControl::Occluded(occluded) => {
                self.occluded = occluded;
                true
            }
            PresentControl::ReplaceDevice(gpu) => {
                self.renderer_epoch = gpu.renderer_epoch;
                self.cancel_waiting(CancelReason::RendererEpoch);
                (self.gpu_renderer, self.submit) = Self::renderer(*gpu, &self.status, &self.waker);
                self.device = Arc::clone(&self.gpu_renderer.device);
                if let (Some(surface), Some(config)) = (&self.surface, &self.config) {
                    surface.configure(&self.device, config);
                }
                self.present_placeholder_frame();
                true
            }
            PresentControl::Capture {
                packet,
                viewport,
                reply,
            } => {
                let mut returns = RenderReturns::default();
                let pixels = self.gpu_renderer.render_to_rgba_pixels(
                    &mut self.submit,
                    viewport,
                    packet,
                    self.surface_epoch,
                    &mut returns,
                );
                self.record_status();
                let _ = reply.send(CaptureReply { returns, pixels });
                true
            }
            PresentControl::TakeSurface { reply } => {
                self.cancel_waiting(CancelReason::SurfaceUnavailable);
                self.config = None;
                self.offscreen_target = None;
                let _ = reply.send(self.surface.take());
                true
            }
            PresentControl::LoseDevice { ack } => {
                self.device.destroy();
                let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
                let _ = ack.send(());
                true
            }
            PresentControl::AttachOffscreenTargetForTests { width, height, ack } => {
                self.cancel_waiting(CancelReason::SurfaceEpoch);
                self.surface = None;
                self.config = None;
                self.offscreen_target = Some((width, height));
                self.present_placeholder_frame();
                let _ = ack.send(());
                true
            }
            PresentControl::Shutdown => {
                self.cancel_waiting(CancelReason::SurfaceUnavailable);
                false
            }
        }
    }

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

    fn cancel_waiting(&mut self, fallback: CancelReason) {
        if let Some(packet) = self.waiting_packet.take() {
            let reason = self.validate(&packet).unwrap_or(fallback);
            self.cancel_packet(packet, reason);
        }
    }

    fn cancel_packet(&mut self, packet: FramePacket, reason: CancelReason) {
        let mut returns = RenderReturns::default();
        let _ = GpuRenderer::cancel_packet(packet, reason, &mut returns);
        self.finish_returns(returns);
    }

    fn consume_packet(&mut self, packet: FramePacket) {
        if let Some(reason) = self.validate(&packet) {
            self.cancel_packet(packet, reason);
            return;
        }
        let (width, height) = packet.viewport;
        if self.surface.is_some() {
            self.render_to_surface(packet, width, height);
        } else if self.offscreen_target.is_some() {
            self.render_offscreen(packet, width, height);
        } else {
            self.cancel_packet(packet, CancelReason::SurfaceUnavailable);
        }
    }

    fn render_to_surface(&mut self, packet: FramePacket, width: u32, height: u32) {
        if self.gpu_renderer.shows(&packet, (width, height)) {
            self.finish_returns(RenderReturns {
                scene: Some(FrameSceneStorage {
                    root: packet.root,
                    overlay: packet.overlay,
                }),
                frame_id: packet.frame_id,
                outcome: PresentOutcome::Unchanged,
                ..RenderReturns::default()
            });
            return;
        }
        let Some(frame) = self.acquire_with_one_retry() else {
            self.cancel_packet(packet, CancelReason::SurfaceUnavailable);
            return;
        };
        let after_acquire_ns = self.now();
        if let Some(delay) = cranpose_core::DIAGNOSTICS.encode_delay_ms.parse::<u64>() {
            std::thread::sleep(Duration::from_millis(delay));
        }
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.gpu_renderer.surface_format().remove_srgb_suffix()),
            ..Default::default()
        });
        let image = crate::frame::ImageContent::of(&frame);
        let mut returns = RenderReturns::default();
        let result = self.gpu_renderer.render(
            &mut self.submit,
            (width, height),
            packet,
            self.surface_epoch,
            &mut returns,
            FrameOutput::display(&frame.texture, &view, image),
        );
        let after_render_ns = self.now();
        self.present(frame);
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

    fn present_placeholder_frame(&mut self) {
        if self.surface.is_some() && !self.occluded {
            if let Some(frame) = self.acquire_with_one_retry() {
                let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
                    format: Some(self.gpu_renderer.surface_format().remove_srgb_suffix()),
                    ..Default::default()
                });
                crate::initial_present::clear_to_background(
                    &self.device,
                    &self.gpu_renderer.queue,
                    &view,
                    self.background,
                );
                self.gpu_renderer.queue.present(frame);
                self.status
                    .placeholder_frames
                    .fetch_add(1, Ordering::Relaxed);
            }
        } else if let Some((width, height)) = self.offscreen_target {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Present Runtime Placeholder Offscreen Target"),
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
            crate::initial_present::clear_to_default_background(
                &self.device,
                &self.gpu_renderer.queue,
                &view,
            );
            self.status
                .placeholder_frames
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    fn acquire_with_one_retry(&mut self) -> Option<wgpu::SurfaceTexture> {
        let surface = self.surface.as_ref()?;
        match Self::acquire(surface) {
            AcquireOutcome::Ready(frame) => Some(frame),
            AcquireOutcome::Skip => None,
            AcquireOutcome::Reconfigure => {
                let config = self.config.clone()?;
                self.reconfigure(&config);
                let surface = self.surface.as_ref()?;
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
            &mut self.submit,
            (width, height),
            packet,
            self.surface_epoch,
            &mut returns,
            FrameOutput::display(&texture, &view, crate::frame::ImageContent::Untracked),
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

    // Writes the renderer's account of its last frame where the producer
    // reads it.
    fn record_status(&self) {
        *self
            .status
            .last_frame_stats
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = self.gpu_renderer.last_frame_stats();
        self.status
            .needs_frame_warmup
            .store(self.gpu_renderer.needs_frame_warmup(), Ordering::Relaxed);
        self.status.drew_placeholder.store(
            self.gpu_renderer.pipelines.drew_placeholder.get(),
            Ordering::Relaxed,
        );
    }

    fn finish_returns(&mut self, returns: RenderReturns) {
        self.record_status();
        if returns.outcome == PresentOutcome::Presented {
            self.status.presented_frames.fetch_add(1, Ordering::Relaxed);
        }
        self.status.last_present_outcome.store(
            encode_present_outcome(returns.outcome, returns.frame_id),
            Ordering::Relaxed,
        );
        match self.returns_tx.try_send(returns) {
            Ok(()) => {
                self.status.returned_frames.fetch_add(1, Ordering::Release);
            }
            Err(TrySendError::Full(_)) => {
                log::error!("[present-runtime] returns channel full; depth-one credit violated");
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
        (self.waker)();
    }

    /// Gives the current surface a new configuration, letting the platform's
    /// observer act on the swapchain it replaces first.
    fn reconfigure(&mut self, config: &wgpu::SurfaceConfiguration) {
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        if let Some(observer) = self.observer.as_mut() {
            observer.before_reconfigure(surface);
        }
        surface.configure(&self.device, config);
    }

    /// Presents `frame` on the current surface, with the platform's
    /// observer on either side of it.
    fn present(&mut self, frame: wgpu::SurfaceTexture) {
        let observed = self.observer.as_mut().zip(self.surface.as_ref());
        if let Some((observer, surface)) = observed {
            observer.before_present(surface);
            self.gpu_renderer.queue.present(frame);
            observer.after_present(surface);
        } else {
            self.gpu_renderer.queue.present(frame);
        }
    }

    fn now(&self) -> i64 {
        self.clock.as_ref().map_or(0, |clock| clock())
    }
}

enum AcquireOutcome {
    Ready(wgpu::SurfaceTexture),
    Reconfigure,
    Skip,
}

pub(crate) struct PresentHandle {
    msg_tx: Sender<PresentMsg>,
    returns_rx: Receiver<RenderReturns>,
    status: Arc<PresentStatus>,
    thread: Option<std::thread::JoinHandle<()>>,
    outstanding: usize,
    drained: u64,
}

const PRESENT_THREAD_STACK_BYTES: usize = 8 * 1024 * 1024;

impl PresentHandle {
    pub(crate) fn spawn(init: PresentRuntimeInit, waker: PresentWaker) -> Result<Self, String> {
        let (msg_tx, msg_rx) = channel::<PresentMsg>();
        let (returns_tx, returns_rx) =
            sync_channel::<RenderReturns>(crate::frame_packet::MAX_FRAMES_IN_FLIGHT);
        let status = Arc::new(PresentStatus::default());
        let thread_status = Arc::clone(&status);
        let thread = std::thread::Builder::new()
            .name("cranpose-present".to_string())
            .stack_size(PRESENT_THREAD_STACK_BYTES)
            .spawn(move || {
                crate::fast_cores::pin_current_thread_to_fast_cores("present");
                if let Some(id) = crate::fast_cores::current_thread_id() {
                    thread_status.thread_id.store(id, Ordering::Relaxed);
                }
                PresentState::new(init, returns_tx, thread_status, waker).run(msg_rx);
            })
            .map_err(|error| format!("failed to spawn present thread: {error}"))?;
        Ok(Self {
            msg_tx,
            returns_rx,
            status,
            thread: Some(thread),
            outstanding: 0,
            drained: 0,
        })
    }

    pub(crate) fn new_inline(
        init: PresentRuntimeInit,
        waker: PresentWaker,
    ) -> (Self, PresentState, Receiver<PresentMsg>) {
        let (msg_tx, msg_rx) = channel::<PresentMsg>();
        let (returns_tx, returns_rx) =
            sync_channel::<RenderReturns>(crate::frame_packet::MAX_FRAMES_IN_FLIGHT);
        let status = Arc::new(PresentStatus::default());
        let state = PresentState::new(init, returns_tx, Arc::clone(&status), waker);
        (
            Self {
                msg_tx,
                returns_rx,
                status,
                thread: None,
                outstanding: 0,
                drained: 0,
            },
            state,
            msg_rx,
        )
    }

    pub(crate) fn status(&self) -> &PresentStatus {
        &self.status
    }

    pub(crate) fn has_credit(&self) -> bool {
        self.outstanding < crate::frame_packet::MAX_FRAMES_IN_FLIGHT
    }

    /// Whether the present thread has handed back a frame that
    /// [`Self::try_drain`] has not taken yet, which frees a credit.
    pub(crate) fn has_undrained_return(&self) -> bool {
        self.status.returned_frames.load(Ordering::Acquire) > self.drained
    }

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

    pub(crate) fn try_drain(&mut self) -> Option<RenderReturns> {
        match self.returns_rx.try_recv() {
            Ok(returns) => {
                self.outstanding = self.outstanding.saturating_sub(1);
                self.drained += 1;
                Some(returns)
            }
            Err(_) => None,
        }
    }

    // Sends the control `build` makes and waits for the thread's answer to
    // it; `None` when no answer came.
    pub(crate) fn send_control_and_wait<T>(
        &self,
        build: impl FnOnce(SyncSender<T>) -> PresentControl,
        what: &str,
    ) -> Option<T> {
        let Some(ack_rx) = self.send_control_unacked(build) else {
            log::error!("[present-runtime] {what}: runtime is gone");
            return None;
        };
        match ack_rx.recv_timeout(CONTROL_ACK_TIMEOUT) {
            Ok(answer) => Some(answer),
            Err(RecvTimeoutError::Timeout) => {
                log::error!(
                    "[present-runtime] {what}: no ack within {CONTROL_ACK_TIMEOUT:?}; \
                     present thread wedged?"
                );
                None
            }
            Err(RecvTimeoutError::Disconnected) => {
                log::error!("[present-runtime] {what}: runtime exited before ack");
                None
            }
        }
    }

    /// Sends `control` behind the packets already sent, waiting for
    /// nothing. False when the runtime is gone.
    pub(crate) fn send_control(&self, control: PresentControl) -> bool {
        self.msg_tx.send(PresentMsg::Control(control)).is_ok()
    }

    // Draws `packet` into an image of `viewport` on the present thread,
    // behind the packets already sent, and waits for its pixels: a robot's
    // capture, which the frame loop never asks for.
    pub(crate) fn capture(&self, packet: FramePacket, viewport: (u32, u32)) -> CaptureReply {
        let (reply, reply_rx) = sync_channel(1);
        let capture = PresentControl::Capture {
            packet,
            viewport,
            reply,
        };
        if let Err(error) = self.msg_tx.send(PresentMsg::Control(capture)) {
            let PresentMsg::Control(PresentControl::Capture { packet, .. }) = error.0 else {
                unreachable!("capture only sends a capture");
            };
            let mut returns = RenderReturns::default();
            let _ =
                GpuRenderer::cancel_packet(packet, CancelReason::SurfaceUnavailable, &mut returns);
            return CaptureReply {
                returns,
                pixels: Err("the present thread is gone".to_string()),
            };
        }
        reply_rx
            .recv_timeout(CAPTURE_TIMEOUT)
            .unwrap_or_else(|error| CaptureReply {
                returns: RenderReturns::default(),
                pixels: Err(format!("the present thread sent no capture: {error}")),
            })
    }

    pub(crate) fn send_control_unacked<T>(
        &self,
        build: impl FnOnce(SyncSender<T>) -> PresentControl,
    ) -> Option<Receiver<T>> {
        let (ack_tx, ack_rx) = sync_channel::<T>(1);
        self.msg_tx
            .send(PresentMsg::Control(build(ack_tx)))
            .ok()
            .map(|()| ack_rx)
    }

    pub(crate) fn shutdown(&mut self) {
        let _ = self
            .msg_tx
            .send(PresentMsg::Control(PresentControl::Shutdown));
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            log::error!("[present-runtime] present thread panicked");
        }
    }
}

impl Drop for PresentHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<PresentRuntimeInit>();
    assert_send::<PresentControl>();
    assert_send::<PresentMsg>();
    assert_send::<PresentWaker>();
    assert_send::<Option<PresentClock>>();
    assert_send::<Arc<PresentStatus>>();
    assert_send::<SyncSender<RenderReturns>>();
};
