//! Running an application inside another program's window.
//!
//! A host program — an IDE plugin, an editor extension — listens on a loopback
//! address and starts the application as a child process with
//! [`EmbedEndpoint::ADDRESS_VARIABLE`] and [`EmbedEndpoint::TOKEN_VARIABLE`]
//! set. [`AppLauncher::run_embedded`](crate::AppLauncher::run_embedded)
//! connects there, draws every frame off screen on the GPU and streams the
//! rectangle of pixels that changed; the host paints it and sends back size,
//! pointer, keyboard and theme events. [`send_to_host`](crate::send_to_host)
//! and [`rememberHostMessages`](crate::rememberHostMessages) carry the
//! application's own messages in both directions.
//!
//! The application keeps its own process, so a crash never takes the host
//! down, and the same binary still runs as an ordinary desktop window when it
//! is started without an endpoint. The
//! [IntelliJ plugin template](https://github.com/samoylenkodmitry/cranpose-intellij-plugin-template)
//! is a complete host and documents the wire format.

use std::{
    io::{self, BufReader, BufWriter, Write},
    net::TcpStream,
    rc::Rc,
    sync::{
        Arc,
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    time::{Duration, Instant},
};

use cranpose_app_shell::{AppShell, FrameSchedule, default_root_key};
use cranpose_render_wgpu::{WgpuRenderer, WgpuTextSystem};
use cranpose_services::{
    HostMessage, LifecycleState, SystemTheme, advance_lifecycle, clear_host_outbox,
    install_host_outbox, publish_host_message,
};
use cranpose_ui::PointerIcon;

use crate::{
    AppSettings,
    embed_frame::{FRAME_FORMAT, FrameTarget, changed_rect},
    embed_input::EmbedInput,
    embed_protocol::{AppEvent, FrameUpdate, HostEvent, read_host_event, write_app_event},
    platform_env::PlatformEnvironment,
};

const MAX_FRAMES_IN_FLIGHT: u32 = 2;
const DEFAULT_REFRESH_HZ: f32 = 60.0;
const WRITE_BUFFER_BYTES: usize = 1 << 20;

/// Where an embedded application finds its host, and the secret it proves
/// itself with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbedEndpoint {
    address: String,
    token: String,
}

impl EmbedEndpoint {
    /// The environment variable holding the host's loopback `address:port`.
    pub const ADDRESS_VARIABLE: &'static str = "CRANPOSE_EMBED_ADDRESS";
    /// The environment variable holding the token the host expects back.
    pub const TOKEN_VARIABLE: &'static str = "CRANPOSE_EMBED_TOKEN";

    /// An endpoint at `address` (`host:port`) answered with `token`.
    pub fn new(address: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            address: address.into(),
            token: token.into(),
        }
    }

    /// The endpoint a host passed through the environment, or `None` when the
    /// process was not started by a host.
    pub fn from_env() -> Option<Self> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// The endpoint named by `lookup`, which resolves the two variable names
    /// the way the environment would; `None` unless both are present.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Option<Self> {
        Some(Self::new(
            lookup(Self::ADDRESS_VARIABLE)?,
            lookup(Self::TOKEN_VARIABLE)?,
        ))
    }

    /// The host's `address:port`.
    pub fn address(&self) -> &str {
        &self.address
    }
}

/// Why an embedded application stopped before its host closed it.
#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    /// The host was not listening at the endpoint's address.
    #[error("could not connect to the embedding host at {address}: {source}")]
    Connect {
        /// The address that refused the connection.
        address: String,
        /// The underlying socket error.
        #[source]
        source: io::Error,
    },
    /// Reading from or writing to the host failed.
    #[error("the connection to the embedding host failed: {0}")]
    Io(#[from] io::Error),
    /// No GPU adapter could draw off screen.
    #[error("no compatible GPU adapter was available: {0}")]
    NoAdapter(#[source] wgpu::RequestAdapterError),
    /// The GPU adapter refused to create a device.
    #[error("failed to create GPU device: {0}")]
    DeviceCreate(#[source] wgpu::RequestDeviceError),
    /// Drawing a frame failed.
    #[error("rendering a frame failed: {0}")]
    Render(String),
    /// Copying a drawn frame back from the GPU failed.
    #[error("reading a frame back from the GPU failed: {0}")]
    Readback(String),
}

pub(crate) struct HeadlessGpu {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    backend: wgpu::Backend,
    downlevel: wgpu::DownlevelFlags,
}

impl HeadlessGpu {
    pub(crate) fn request() -> Result<Self, EmbedError> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::all();
        let instance = wgpu::Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            ..wgpu::RequestAdapterOptions::default()
        }))
        .map_err(EmbedError::NoAdapter)?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Embed Device"),
            required_features: cranpose_render_wgpu::optional_device_features(&adapter),
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(EmbedError::DeviceCreate)?;
        log::info!("embed: drawing on {:?}", adapter.get_info());
        Ok(Self {
            device: Arc::new(device),
            queue: Arc::new(queue),
            backend: adapter.get_info().backend,
            downlevel: adapter.get_downlevel_capabilities().flags,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SurfaceSize {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) scale: f32,
    pub(crate) refresh_hz: f32,
}

impl SurfaceSize {
    pub(crate) fn clamped(width: u32, height: u32, scale: f32, refresh_hz: f32) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
            scale: if scale.is_finite() && scale > 0.0 {
                scale
            } else {
                1.0
            },
            refresh_hz: if refresh_hz.is_finite() && refresh_hz >= 1.0 {
                refresh_hz
            } else {
                DEFAULT_REFRESH_HZ
            },
        }
    }

    pub(crate) fn viewport(&self) -> (f32, f32) {
        (
            self.width as f32 / self.scale,
            self.height as f32 / self.scale,
        )
    }

    pub(crate) fn frame_interval(&self) -> Duration {
        Duration::from_secs_f64(1.0 / f64::from(self.refresh_hz))
    }
}

pub(crate) struct EmbeddedHost {
    shell: AppShell<WgpuRenderer>,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    platform_env: Rc<PlatformEnvironment>,
    input: EmbedInput,
    target: FrameTarget,
    size: SurfaceSize,
    drawn: Vec<u8>,
    shown: Vec<u8>,
    surface_dirty: bool,
    visible: bool,
    next_frame_id: u32,
}

impl EmbeddedHost {
    pub(crate) fn new(
        settings: &AppSettings,
        gpu: HeadlessGpu,
        platform_env: Rc<PlatformEnvironment>,
        content: impl FnMut() + 'static,
        size: SurfaceSize,
    ) -> Self {
        let mut renderer = WgpuRenderer::with_text_system(WgpuTextSystem::from_font_set(
            settings.resolve_font_set(),
        ));
        renderer.warm_shaders(cranpose_liquid::shader_warm_ups());
        renderer.set_root_scale(size.scale);
        renderer.init_gpu(
            Arc::clone(&gpu.device),
            Arc::clone(&gpu.queue),
            FRAME_FORMAT,
            gpu.backend,
            gpu.downlevel,
        );
        let env_for_content = Rc::clone(&platform_env);
        let mut content = content;
        let mut shell = AppShell::new_with_size_and_density(
            renderer,
            default_root_key(),
            move || env_for_content.compose_root(&mut content),
            (size.width, size.height),
            size.viewport(),
            size.scale,
        );
        shell.set_semantics_enabled(true);
        publish_surface_size(size);
        Self {
            target: FrameTarget::new(&gpu.device, size.width, size.height),
            shell,
            device: gpu.device,
            queue: gpu.queue,
            platform_env,
            input: EmbedInput::new(),
            size,
            drawn: Vec::new(),
            shown: Vec::new(),
            surface_dirty: true,
            visible: true,
            next_frame_id: 1,
        }
    }

    pub(crate) fn size(&self) -> SurfaceSize {
        self.size
    }

    pub(crate) fn set_frame_waker(&mut self, waker: impl Fn() + Send + Sync + 'static) {
        self.shell.set_frame_waker(waker);
    }

    pub(crate) fn handle(&mut self, event: HostEvent) {
        match event {
            HostEvent::Resize {
                width,
                height,
                scale,
                refresh_hz,
            } => self.resize(SurfaceSize::clamped(width, height, scale, refresh_hz)),
            HostEvent::Theme { dark } => {
                if self.platform_env.set_system_theme(system_theme(dark)) {
                    self.shell.request_root_render();
                    self.surface_dirty = true;
                }
            }
            HostEvent::Message { channel, payload } => {
                publish_host_message(HostMessage::new(channel, payload));
            }
            HostEvent::Visibility(visible) => self.set_visible(visible),
            event => {
                self.input.dispatch(&mut self.shell, event);
            }
        }
    }

    fn set_visible(&mut self, visible: bool) {
        if visible == self.visible {
            return;
        }
        self.visible = visible;
        if visible {
            self.shell.notify_app_resumed();
            advance_lifecycle(LifecycleState::Resumed);
            self.surface_dirty = true;
        } else {
            self.shell.notify_app_paused();
            advance_lifecycle(LifecycleState::Stopped);
        }
    }

    pub(crate) fn visible(&self) -> bool {
        self.visible
    }

    fn resize(&mut self, size: SurfaceSize) {
        if size == self.size {
            return;
        }
        if (size.width, size.height) != self.target.size() {
            self.target = FrameTarget::new(&self.device, size.width, size.height);
            self.shown.clear();
        }
        self.size = size;
        self.shell.renderer().set_root_scale(size.scale);
        self.shell.renderer().note_surface_reconfigured();
        self.shell.set_density(size.scale);
        self.shell.set_buffer_size(size.width, size.height);
        let (logical_width, logical_height) = size.viewport();
        self.shell.set_viewport(logical_width, logical_height);
        publish_surface_size(size);
        self.surface_dirty = true;
    }

    pub(crate) fn frame_schedule(&self) -> FrameSchedule {
        self.shell.frame_schedule()
    }

    pub(crate) fn wants_frame(&self) -> bool {
        self.surface_dirty
    }

    pub(crate) fn produce_frame(&mut self) -> Result<Option<FrameUpdate<'_>>, EmbedError> {
        self.shell.update();
        let owed = self.shell.take_frame_owed();
        if !(self.surface_dirty || owed || self.shell.needs_redraw()) {
            return Ok(None);
        }
        let (width, height) = self.target.size();
        self.shell
            .renderer()
            .render(self.target.texture(), self.target.view(), width, height)
            .map_err(|error| EmbedError::Render(format!("{error:?}")))?;
        self.target
            .read_into(&self.device, &self.queue, &mut self.drawn)?;
        self.surface_dirty = false;
        let Some(rect) = changed_rect(&self.shown, &self.drawn, width, height) else {
            return Ok(None);
        };
        std::mem::swap(&mut self.shown, &mut self.drawn);
        let frame_id = self.next_frame_id;
        self.next_frame_id = self.next_frame_id.wrapping_add(1);
        Ok(Some(FrameUpdate {
            frame_id,
            buffer_width: width,
            buffer_height: height,
            rect,
            pixels: &self.shown,
        }))
    }

    pub(crate) fn take_cursor_change(&mut self) -> Option<&'static str> {
        self.shell
            .take_pointer_icon_change()
            .map(|icon| cursor_name(&icon))
    }
}

pub(crate) fn cursor_name(icon: &PointerIcon) -> &'static str {
    match icon {
        PointerIcon::System(cursor) => cursor.name(),
        PointerIcon::Custom(_) => "default",
    }
}

fn system_theme(dark: bool) -> SystemTheme {
    if dark {
        SystemTheme::Dark
    } else {
        SystemTheme::Light
    }
}

fn publish_surface_size(size: SurfaceSize) {
    let (width, height) = size.viewport();
    cranpose_services::publish_host_surface_size(
        cranpose_services::host_surface::HostSurfaceSize {
            width,
            height,
            scale: size.scale,
        },
    );
}

pub(crate) enum LoopEvent {
    Host(HostEvent),
    Outgoing(HostMessage),
    Wake,
    Disconnected,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Pacing {
    frames_in_flight: u32,
    last_tick_at: Option<Instant>,
}

impl Pacing {
    pub(crate) fn new() -> Self {
        Self {
            frames_in_flight: 0,
            last_tick_at: None,
        }
    }

    pub(crate) fn sent_frame(&mut self) {
        self.frames_in_flight += 1;
    }

    pub(crate) fn acknowledged(&mut self) {
        self.frames_in_flight = self.frames_in_flight.saturating_sub(1);
    }

    pub(crate) fn ticked(&mut self, at: Instant) {
        self.last_tick_at = Some(at);
    }

    pub(crate) fn may_tick(&self, interval: Duration, now: Instant) -> bool {
        self.frames_in_flight < MAX_FRAMES_IN_FLIGHT
            && self
                .last_tick_at
                .is_none_or(|at| now.saturating_duration_since(at) >= interval)
    }

    pub(crate) fn next_wake(
        &self,
        schedule: FrameSchedule,
        surface_dirty: bool,
        interval: Duration,
        now: Instant,
    ) -> Option<Instant> {
        if self.frames_in_flight >= MAX_FRAMES_IN_FLIGHT {
            return None;
        }
        let tick_at = self.last_tick_at.map_or(now, |at| at + interval);
        if surface_dirty || schedule.needs_frame || schedule.needs_update {
            return Some(tick_at);
        }
        schedule.next_deadline.map(|deadline| deadline.max(tick_at))
    }
}

pub(crate) fn serve(
    host: &mut EmbeddedHost,
    events: &Receiver<LoopEvent>,
    writer: &mut impl Write,
) -> Result<(), EmbedError> {
    let mut pacing = Pacing::new();
    loop {
        let wake = host.visible().then(|| {
            pacing.next_wake(
                host.frame_schedule(),
                host.wants_frame(),
                host.size().frame_interval(),
                Instant::now(),
            )
        });
        let Some(batch) = receive(events, wake.flatten()) else {
            return Ok(());
        };
        if !apply_batch(host, &mut pacing, writer, batch)? {
            return Ok(());
        }
        tick(host, &mut pacing, writer)?;
    }
}

fn receive(events: &Receiver<LoopEvent>, wake: Option<Instant>) -> Option<Vec<LoopEvent>> {
    let first = match wake {
        Some(at) => match events.recv_timeout(at.saturating_duration_since(Instant::now())) {
            Ok(event) => Some(event),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return None,
        },
        None => Some(events.recv().ok()?),
    };
    Some(first.into_iter().chain(events.try_iter()).collect())
}

fn apply_batch(
    host: &mut EmbeddedHost,
    pacing: &mut Pacing,
    writer: &mut impl Write,
    batch: Vec<LoopEvent>,
) -> Result<bool, EmbedError> {
    for event in batch {
        match event {
            LoopEvent::Host(HostEvent::Close) | LoopEvent::Disconnected => return Ok(false),
            LoopEvent::Host(HostEvent::FrameAck(_)) => pacing.acknowledged(),
            LoopEvent::Host(event) => host.handle(event),
            LoopEvent::Outgoing(message) => write_app_event(
                writer,
                &AppEvent::Message {
                    channel: &message.channel,
                    payload: &message.payload,
                },
            )?,
            LoopEvent::Wake => {}
        }
    }
    Ok(true)
}

fn tick(
    host: &mut EmbeddedHost,
    pacing: &mut Pacing,
    writer: &mut impl Write,
) -> Result<(), EmbedError> {
    let now = Instant::now();
    if host.visible() && pacing.may_tick(host.size().frame_interval(), now) {
        pacing.ticked(now);
        if let Some(frame) = host.produce_frame()? {
            write_app_event(writer, &AppEvent::Frame(frame))?;
            pacing.sent_frame();
        }
    }
    if let Some(cursor) = host.take_cursor_change() {
        write_app_event(writer, &AppEvent::Cursor(cursor))?;
    }
    writer.flush()?;
    Ok(())
}

fn await_first_size(
    events: &Receiver<LoopEvent>,
    platform_env: &PlatformEnvironment,
) -> Option<SurfaceSize> {
    while let Ok(event) = events.recv() {
        match event {
            LoopEvent::Host(HostEvent::Resize {
                width,
                height,
                scale,
                refresh_hz,
            }) => return Some(SurfaceSize::clamped(width, height, scale, refresh_hz)),
            LoopEvent::Host(HostEvent::Theme { dark }) => {
                platform_env.set_system_theme(system_theme(dark));
            }
            LoopEvent::Host(HostEvent::Message { channel, payload }) => {
                publish_host_message(HostMessage::new(channel, payload));
            }
            LoopEvent::Host(HostEvent::Close) | LoopEvent::Disconnected => return None,
            LoopEvent::Host(_) | LoopEvent::Outgoing(_) | LoopEvent::Wake => {}
        }
    }
    None
}

fn spawn_reader(stream: TcpStream, sender: Sender<LoopEvent>) -> io::Result<()> {
    std::thread::Builder::new()
        .name("cranpose-embed-reader".to_owned())
        .spawn(move || {
            let mut reader = BufReader::new(stream);
            loop {
                match read_host_event(&mut reader) {
                    Ok(Some(event)) => {
                        if sender.send(LoopEvent::Host(event)).is_err() {
                            return;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        log::warn!("embed: the host connection broke: {error}");
                        break;
                    }
                }
            }
            let _ = sender.send(LoopEvent::Disconnected);
        })
        .map(drop)
}

pub(crate) fn try_run(
    settings: AppSettings,
    endpoint: EmbedEndpoint,
    content: impl FnMut() + 'static,
) -> Result<(), EmbedError> {
    crate::application_id::register(settings.application_id.as_deref());
    let stream = TcpStream::connect(&endpoint.address).map_err(|source| EmbedError::Connect {
        address: endpoint.address.clone(),
        source,
    })?;
    stream.set_nodelay(true)?;
    let mut writer = BufWriter::with_capacity(WRITE_BUFFER_BYTES, stream.try_clone()?);
    write_app_event(
        &mut writer,
        &AppEvent::Hello {
            token: &endpoint.token,
        },
    )?;
    writer.flush()?;

    let (sender, events) = mpsc::channel();
    spawn_reader(stream, sender.clone())?;
    let gpu = HeadlessGpu::request()?;
    let platform_env = PlatformEnvironment::new();
    let Some(size) = await_first_size(&events, &platform_env) else {
        return Ok(());
    };

    let outbox = sender.clone();
    install_host_outbox(move |message| {
        let _ = outbox.send(LoopEvent::Outgoing(message));
    });
    let mut host = EmbeddedHost::new(&settings, gpu, platform_env, content, size);
    host.set_frame_waker(move || {
        let _ = sender.send(LoopEvent::Wake);
    });
    advance_lifecycle(LifecycleState::Resumed);

    let result = serve(&mut host, &events, &mut writer);
    clear_host_outbox();
    advance_lifecycle(LifecycleState::Destroyed);
    result
}

#[cfg(test)]
#[path = "tests/embed_tests.rs"]
mod tests;
