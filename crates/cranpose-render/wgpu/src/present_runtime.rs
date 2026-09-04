use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{
            Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError, TrySendError, channel,
            sync_channel,
        },
    },
    time::Duration,
};

use cranpose_render_common::software_text_raster::SoftwareTextFontSet;

use crate::{
    frame_packet::{CancelReason, FramePacket, PresentOutcome, PresentTimings, RenderReturns},
    render::GpuRenderer,
};

pub(crate) type PresentWaker = Arc<dyn Fn() + Send + Sync>;

pub(crate) type PresentClock = Arc<dyn Fn() -> i64 + Send + Sync>;

pub(crate) const CONTROL_ACK_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct PresentRuntimeInit {
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) queue: Arc<wgpu::Queue>,
    pub(crate) surface_format: wgpu::TextureFormat,
    pub(crate) adapter_backend: wgpu::Backend,
    pub(crate) adapter_downlevel: wgpu::DownlevelFlags,
    pub(crate) text_fonts: SoftwareTextFontSet,
    pub(crate) renderer_epoch: u64,
    pub(crate) clock: Option<PresentClock>,
}

pub(crate) enum PresentControl {
    ReplaceSurface {
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
        surface_epoch: u64,
        ack: SyncSender<()>,
    },
    Reconfigure {
        config: wgpu::SurfaceConfiguration,
        surface_epoch: u64,
        ack: SyncSender<()>,
    },
    DropSurface {
        ack: SyncSender<()>,
    },
    #[doc(hidden)]
    AttachOffscreenTargetForTests {
        width: u32,
        height: u32,
        ack: SyncSender<()>,
    },
    Shutdown,
}

#[allow(clippy::large_enum_variant)]
pub(crate) enum PresentMsg {
    Control(PresentControl),
    Packet(FramePacket),
}

#[derive(Default)]
pub(crate) struct PresentStatus {
    pub(crate) last_frame_stats: Mutex<Option<crate::gpu_stats::FrameStatsSnapshot>>,
    pub(crate) needs_frame_warmup: AtomicBool,
    pub(crate) presented_frames: AtomicU64,
    pub(crate) placeholder_frames: AtomicU64,
    pub(crate) last_present_outcome: AtomicU64,
    pub(crate) last_error_frame: AtomicU64,
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
    };
    (frame_id << 8) | code
}

pub(crate) struct PresentState {
    gpu_renderer: GpuRenderer,
    device: Arc<wgpu::Device>,
    surface: Option<wgpu::Surface<'static>>,
    config: Option<wgpu::SurfaceConfiguration>,
    offscreen_target: Option<(u32, u32)>,
    renderer_epoch: u64,
    surface_epoch: u64,
    waiting_packet: Option<FramePacket>,
    returns_tx: SyncSender<RenderReturns>,
    status: Arc<PresentStatus>,
    waker: PresentWaker,
    clock: Option<PresentClock>,
}

impl PresentState {
    pub(crate) fn new(
        init: PresentRuntimeInit,
        returns_tx: SyncSender<RenderReturns>,
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
            clock,
        } = init;
        let gpu_renderer = GpuRenderer::new(
            device.clone(),
            queue,
            surface_format,
            adapter_backend,
            adapter_downlevel,
            text_fonts,
            renderer_epoch,
        );
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
            status,
            waker,
            clock,
        }
    }

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
    }

    pub(crate) fn run_once(&mut self, msg: PresentMsg) -> bool {
        match msg {
            PresentMsg::Control(control) => self.handle_control(control),
            PresentMsg::Packet(packet) => {
                if self.waiting_packet.is_some() {
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
                self.cancel_waiting(CancelReason::SurfaceEpoch);
                surface.configure(&self.device, &config);
                self.surface = Some(surface);
                self.config = Some(config);
                self.offscreen_target = None;
                self.present_placeholder_frame();
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
                self.cancel_waiting(CancelReason::SurfaceUnavailable);
                self.surface = None;
                self.config = None;
                self.offscreen_target = None;
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
        let frame = match self.acquire_with_one_retry() {
            Some(frame) => frame,
            None => {
                self.cancel_packet(packet, CancelReason::SurfaceUnavailable);
                return;
            }
        };
        let after_acquire_ns = self.now();
        if let Some(delay) = crate::debug_toggles::debug_toggle("CRANPOSE_ENCODE_DELAY_MS")
            .and_then(|value| value.parse::<u64>().ok())
        {
            std::thread::sleep(std::time::Duration::from_millis(delay));
        }
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.gpu_renderer.surface_format().remove_srgb_suffix()),
            ..Default::default()
        });
        let mut returns = RenderReturns::default();
        let result = self.gpu_renderer.render(
            &frame.texture,
            &view,
            width,
            height,
            packet,
            self.surface_epoch,
            &mut returns,
        );
        let after_render_ns = self.now();
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

    fn present_placeholder_frame(&mut self) {
        if self.surface.is_some() {
            if let Some(frame) = self.acquire_with_one_retry() {
                let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
                    format: Some(self.gpu_renderer.surface_format().remove_srgb_suffix()),
                    ..Default::default()
                });
                crate::initial_present::clear_to_default_background(
                    &self.device,
                    &self.gpu_renderer.queue,
                    &view,
                );
                frame.present();
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
            &texture,
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

    fn finish_returns(&mut self, returns: RenderReturns) {
        *self
            .status
            .last_frame_stats
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            self.gpu_renderer.last_frame_stats();
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
                log::error!("[present-runtime] returns channel full; depth-one credit violated");
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
        (self.waker)();
    }

    fn now(&self) -> i64 {
        self.clock.as_ref().map(|clock| clock()).unwrap_or(0)
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
    outstanding: u32,
}

impl PresentHandle {
    pub(crate) fn spawn(init: PresentRuntimeInit, waker: PresentWaker) -> Result<Self, String> {
        let (msg_tx, msg_rx) = channel::<PresentMsg>();
        let (returns_tx, returns_rx) = sync_channel::<RenderReturns>(2);
        let status = Arc::new(PresentStatus::default());
        let thread_status = Arc::clone(&status);
        let thread = std::thread::Builder::new()
            .name("cranpose-present".to_string())
            .spawn(move || {
                crate::fast_cores::pin_current_thread_to_fast_cores("present");
                PresentState::new(init, returns_tx, thread_status, waker).run(msg_rx);
            })
            .map_err(|error| format!("failed to spawn present thread: {error}"))?;
        Ok(Self {
            msg_tx,
            returns_rx,
            status,
            thread: Some(thread),
            outstanding: 0,
        })
    }

    pub(crate) fn new_inline(
        init: PresentRuntimeInit,
        waker: PresentWaker,
    ) -> (Self, PresentState, Receiver<PresentMsg>) {
        let (msg_tx, msg_rx) = channel::<PresentMsg>();
        let (returns_tx, returns_rx) = sync_channel::<RenderReturns>(2);
        let status = Arc::new(PresentStatus::default());
        let state = PresentState::new(init, returns_tx, Arc::clone(&status), waker);
        (
            Self {
                msg_tx,
                returns_rx,
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

    pub(crate) fn has_credit(&self) -> bool {
        self.outstanding < 2
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
                Some(returns)
            }
            Err(_) => None,
        }
    }

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
