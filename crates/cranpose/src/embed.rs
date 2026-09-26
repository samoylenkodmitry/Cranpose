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
//! One application can draw several surfaces into its host. Besides the
//! panel it was started for, every subtree under
//! [`Modifier::window`](crate::WindowModifierExt::window) becomes a window
//! the host opens (borderless, transparent and shaped by what it draws when
//! it asks to be), and every [`HostOverlay`] becomes a transparent layer the
//! host lays over one of its own views without taking its input.
//!
//! The application keeps its own process, so a crash never takes the host
//! down, and the same binary still runs as an ordinary desktop window when it
//! is started without an endpoint. The
//! [IntelliJ plugin template](https://github.com/samoylenkodmitry/cranpose-intellij-plugin-template)
//! is a complete host and documents the wire format.

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    io::{self, BufReader, BufWriter, Write},
    net::TcpStream,
    rc::Rc,
    sync::{
        Arc,
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    time::{Duration, Instant},
};

use cranpose_app_shell::{AppShell, FrameSchedule, RootId, default_root_key};
use cranpose_core::NodeId;
use cranpose_render_wgpu::{WgpuRenderer, WgpuTextSystem};
use cranpose_services::{
    HostMessage, LifecycleState, SystemTheme, advance_lifecycle, clear_host_outbox,
    install_host_outbox, publish_host_message,
};
use cranpose_ui::{PointerIcon, Size, WindowRootDescriptor};

pub use crate::embed_overlay::HostOverlay;
use crate::{
    AppSettings,
    embed_frame::FRAME_FORMAT,
    embed_input::EmbedInput,
    embed_overlay::HostOverlayRoot,
    embed_protocol::{
        AppEvent, FrameUpdate, HostEvent, PRIMARY_SURFACE, SurfaceCommand, SurfaceEvent, SurfaceId,
        read_host_event, write_app_event,
    },
    embed_surfaces::{SurfaceKind, SurfaceSlot, WindowSurface},
    native_window::{
        NativeWindowRegistry, NativeWindowRequest, WindowResizeDirection,
        has_native_window_requests, native_window_requests, with_native_window_drag_handler,
        with_native_window_registry,
    },
    platform_env::PlatformEnvironment,
};

const DEFAULT_REFRESH_HZ: f32 = 60.0;
const WRITE_BUFFER_BYTES: usize = 1 << 20;

/// Host → application: request an on-demand layout and runtime report. The payload is empty.
pub const INSPECT_REQUEST_CHANNEL: &str = "cranpose.inspector.v1.request";
/// Application → host: a UTF-8 layout and runtime report for the primary surface.
///
/// Reports are produced only on request, never on each rendered frame. They may
/// include visible text from the application. Hosts should keep them local.
pub const INSPECT_SNAPSHOT_CHANNEL: &str = "cranpose.inspector.v1.snapshot";

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

#[derive(Clone)]
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

fn renderer_for(
    text_system: &WgpuTextSystem,
    gpu: &HeadlessGpu,
    scale: f32,
    transparent: bool,
) -> WgpuRenderer {
    let mut renderer = WgpuRenderer::with_text_system(text_system.clone());
    renderer.warm_shaders(cranpose_liquid::shader_warm_ups());
    renderer.set_root_scale(scale);
    renderer.set_transparent_background(transparent);
    renderer.init_gpu(
        Arc::clone(&gpu.device),
        Arc::clone(&gpu.queue),
        FRAME_FORMAT,
        gpu.backend,
        gpu.downlevel,
    );
    renderer
}

pub(crate) struct EmbeddedHost {
    shell: AppShell<WgpuRenderer>,
    gpu: HeadlessGpu,
    text_system: WgpuTextSystem,
    platform_env: Rc<PlatformEnvironment>,
    registry: Rc<NativeWindowRegistry>,
    input: EmbedInput,
    surfaces: BTreeMap<SurfaceId, SurfaceSlot>,
    windows: HashMap<u64, SurfaceId>,
    overlays: HashMap<NodeId, SurfaceId>,
    pending_overlays: HashMap<SurfaceId, (NodeId, Rc<dyn WindowRootDescriptor>)>,
    next_surface: SurfaceId,
    commands: Rc<RefCell<Vec<SurfaceCommand>>>,
    primary_size: SurfaceSize,
    primary_visible: bool,
    surfaces_added: bool,
}

impl EmbeddedHost {
    pub(crate) fn new(
        settings: &AppSettings,
        gpu: HeadlessGpu,
        platform_env: Rc<PlatformEnvironment>,
        content: impl FnMut() + 'static,
        size: SurfaceSize,
    ) -> Self {
        let text_system = WgpuTextSystem::from_font_set(settings.resolve_font_set());
        let renderer = renderer_for(&text_system, &gpu, size.scale, false);
        let registry = Rc::new(NativeWindowRegistry::default());
        let env_for_content = Rc::clone(&platform_env);
        let mut content = content;
        let mut shell = with_native_window_registry(&registry, || {
            AppShell::new_with_size_and_density(
                renderer,
                default_root_key(),
                move || env_for_content.compose_root(&mut content),
                (size.width, size.height),
                size.viewport(),
                size.scale,
            )
        });
        shell.set_semantics_enabled(true);
        publish_surface_size(size);
        let primary = SurfaceSlot::new(
            PRIMARY_SURFACE,
            RootId::Primary,
            SurfaceKind::Primary,
            &gpu.device,
            size,
        );
        Self {
            shell,
            gpu,
            text_system,
            platform_env,
            registry,
            input: EmbedInput::new(),
            surfaces: BTreeMap::from([(PRIMARY_SURFACE, primary)]),
            windows: HashMap::new(),
            overlays: HashMap::new(),
            pending_overlays: HashMap::new(),
            next_surface: PRIMARY_SURFACE + 1,
            commands: Rc::new(RefCell::new(Vec::new())),
            primary_size: size,
            primary_visible: true,
            surfaces_added: false,
        }
    }

    pub(crate) fn size(&self) -> SurfaceSize {
        self.primary_size
    }

    pub(crate) fn set_frame_waker(&mut self, waker: impl Fn() + Send + Sync + 'static) {
        self.shell.set_frame_waker(waker);
    }

    pub(crate) fn handle(&mut self, event: HostEvent) {
        match event {
            HostEvent::Theme { dark } => {
                if self.platform_env.set_system_theme(system_theme(dark)) {
                    self.shell.request_root_render();
                    self.surfaces.values_mut().for_each(SurfaceSlot::mark_dirty);
                }
            }
            HostEvent::Message { channel, payload } => {
                publish_host_message(HostMessage::new(channel, payload));
            }
            HostEvent::Surface { surface, event } => self.handle_surface(surface, event),
            HostEvent::Close => {}
        }
    }

    fn handle_surface(&mut self, surface: SurfaceId, event: SurfaceEvent) {
        match event {
            SurfaceEvent::Resize {
                width,
                height,
                scale,
                refresh_hz,
            } => self.resize(
                surface,
                SurfaceSize::clamped(width, height, scale, refresh_hz),
            ),
            SurfaceEvent::Visibility(visible) => self.set_visible(surface, visible),
            SurfaceEvent::FrameAck(_) => {
                if let Some(slot) = self.surfaces.get_mut(&surface) {
                    slot.acknowledged();
                }
            }
            SurfaceEvent::Moved { x, y } => {
                if let Some(SurfaceKind::Window(window)) =
                    self.surfaces.get_mut(&surface).map(|slot| &mut slot.kind)
                {
                    window.host_moved(x, y);
                }
            }
            SurfaceEvent::CloseRequested => {
                if let Some(SurfaceKind::Window(window)) =
                    self.surfaces.get(&surface).map(|slot| &slot.kind)
                {
                    window.close_requested();
                }
            }
            event => self.dispatch_input(surface, &event),
        }
    }

    fn dispatch_input(&mut self, surface: SurfaceId, event: &SurfaceEvent) {
        let Some((root, is_window)) = self
            .surfaces
            .get(&surface)
            .map(|slot| (slot.root, matches!(slot.kind, SurfaceKind::Window(_))))
        else {
            return;
        };
        let Some(mut target) = self.shell.surface(root) else {
            return;
        };
        if !is_window {
            self.input.dispatch(&mut target, event);
            return;
        }
        let moves = Rc::clone(&self.commands);
        let resizes = Rc::clone(&self.commands);
        let drag: Rc<dyn Fn() -> bool> = Rc::new(move || {
            moves.borrow_mut().push(SurfaceCommand::BeginMove(surface));
            true
        });
        let resize: Rc<dyn Fn(WindowResizeDirection)> = Rc::new(move |direction| {
            resizes
                .borrow_mut()
                .push(SurfaceCommand::BeginResize(surface, direction));
        });
        let input = &self.input;
        with_native_window_drag_handler(drag, resize, || input.dispatch(&mut target, event));
    }

    fn resize(&mut self, surface: SurfaceId, size: SurfaceSize) {
        if let Some((node, descriptor)) = self.pending_overlays.remove(&surface) {
            self.attach_overlay(surface, node, descriptor, size);
            return;
        }
        let Some(slot) = self.surfaces.get_mut(&surface) else {
            return;
        };
        if !slot.resize(&self.gpu.device, size) {
            return;
        }
        let (width, height) = size.viewport();
        slot.kind.set_content_size(Size::new(width, height));
        if slot.root == RootId::Primary {
            self.primary_size = size;
            self.shell.renderer().set_root_scale(size.scale);
            self.shell.renderer().note_surface_reconfigured();
            self.shell.set_density(size.scale);
            self.shell.set_buffer_size(size.width, size.height);
            self.shell.set_viewport(width, height);
            publish_surface_size(size);
        } else if let Some(mut target) = self.shell.surface(slot.root) {
            target.renderer().set_root_scale(size.scale);
            target.renderer().note_surface_reconfigured();
            target.set_buffer_size(size.width, size.height);
            target.set_viewport(width, height);
        }
    }

    fn set_visible(&mut self, surface: SurfaceId, visible: bool) {
        if let Some(slot) = self.surfaces.get_mut(&surface) {
            slot.set_visible(visible);
        }
        if surface != PRIMARY_SURFACE || visible == self.primary_visible {
            return;
        }
        self.primary_visible = visible;
        if visible {
            self.shell.notify_app_resumed();
            advance_lifecycle(LifecycleState::Resumed);
        } else {
            self.shell.notify_app_paused();
            advance_lifecycle(LifecycleState::Stopped);
        }
    }

    pub(crate) fn frame_schedule(&self) -> FrameSchedule {
        self.shell.frame_schedule()
    }

    pub(crate) fn wants_frame(&self) -> bool {
        self.surfaces.values().any(SurfaceSlot::wants_frame)
    }

    pub(crate) fn can_draw(&self) -> bool {
        self.primary_visible && self.surfaces.values().any(SurfaceSlot::has_room)
    }

    pub(crate) fn update(&mut self) {
        let registry = Rc::clone(&self.registry);
        with_native_window_registry(&registry, || self.shell.update());
        self.surfaces_added = false;
        self.sync_windows();
        self.sync_overlays();
        if self.surfaces_added {
            with_native_window_registry(&registry, || self.shell.update());
        }
    }

    fn add_surface(&mut self, key: u64, renderer: WgpuRenderer, size: SurfaceSize) {
        let (width, height) = size.viewport();
        self.shell
            .add_window_surface(key, renderer, (size.width, size.height), (width, height));
        if let Some(mut target) = self.shell.surface(RootId::Window(key)) {
            target.set_viewport(width, height);
        }
        self.surfaces_added = true;
    }

    fn allocate_surface(&mut self) -> SurfaceId {
        let surface = self.next_surface;
        self.next_surface = self.next_surface.wrapping_add(1).max(PRIMARY_SURFACE + 1);
        surface
    }

    fn sync_windows(&mut self) {
        if self.windows.is_empty() && !has_native_window_requests(&self.registry) {
            return;
        }
        let requests: Vec<NativeWindowRequest> = native_window_requests(&self.registry)
            .into_iter()
            .filter(|request| request.options.visible)
            .collect();
        let closed: Vec<u64> = self
            .windows
            .keys()
            .copied()
            .filter(|key| !requests.iter().any(|request| request.key.raw() == *key))
            .collect();
        for key in closed {
            self.close_window(key);
        }
        for request in &requests {
            match self.windows.get(&request.key.raw()).copied() {
                Some(surface) => self.refresh_window(surface, request),
                None => self.open_window(request),
            }
        }
    }

    fn open_window(&mut self, request: &NativeWindowRequest) {
        let surface = self.allocate_surface();
        let window = WindowSurface::from_request(surface, request);
        let scale = self.size().scale;
        let content = window.content_size();
        let size = SurfaceSize::clamped(
            (content.width * scale).ceil() as u32,
            (content.height * scale).ceil() as u32,
            scale,
            self.size().refresh_hz,
        );
        let kind = SurfaceKind::Window(window);
        let renderer = renderer_for(&self.text_system, &self.gpu, scale, kind.transparent());
        let key = request.key.raw();
        if let SurfaceKind::Window(window) = &kind {
            window.set_content_size(content);
        }
        self.add_surface(key, renderer, size);
        if let SurfaceKind::Window(window) = &kind {
            window.set_presented(true);
            self.commands
                .borrow_mut()
                .push(SurfaceCommand::OpenWindow(window.spec.clone()));
        }
        self.windows.insert(key, surface);
        self.surfaces.insert(
            surface,
            SurfaceSlot::new(surface, RootId::Window(key), kind, &self.gpu.device, size),
        );
    }

    fn refresh_window(&mut self, surface: SurfaceId, request: &NativeWindowRequest) {
        let Some(slot) = self.surfaces.get_mut(&surface) else {
            return;
        };
        let SurfaceKind::Window(window) = &mut slot.kind else {
            return;
        };
        if let Some(spec) = window.refresh(request) {
            self.commands
                .borrow_mut()
                .push(SurfaceCommand::OpenWindow(spec));
        }
    }

    fn close_window(&mut self, key: u64) {
        let Some(surface) = self.windows.remove(&key) else {
            return;
        };
        self.shell.remove_window_surface(key);
        if let Some(SurfaceKind::Window(window)) =
            self.surfaces.remove(&surface).map(|slot| slot.kind)
        {
            window.set_presented(false);
        }
        self.commands
            .borrow_mut()
            .push(SurfaceCommand::Close(surface));
    }

    fn sync_overlays(&mut self) {
        let declared: Vec<(NodeId, Rc<dyn WindowRootDescriptor>)> = self
            .shell
            .window_roots()
            .into_iter()
            .filter(|entry| entry.descriptor.as_any().is::<HostOverlayRoot>())
            .map(|entry| (entry.node, entry.descriptor))
            .collect();
        let removed: Vec<NodeId> = self
            .overlays
            .keys()
            .copied()
            .filter(|node| !declared.iter().any(|(declared, _)| declared == node))
            .collect();
        for node in removed {
            self.close_overlay(node);
        }
        for (node, descriptor) in declared {
            if self.overlays.contains_key(&node) {
                continue;
            }
            let Some(anchor) = descriptor
                .as_any()
                .downcast_ref::<HostOverlayRoot>()
                .map(|overlay| overlay.anchor().to_owned())
            else {
                continue;
            };
            let surface = self.allocate_surface();
            self.overlays.insert(node, surface);
            self.pending_overlays.insert(surface, (node, descriptor));
            self.commands
                .borrow_mut()
                .push(SurfaceCommand::OpenOverlay { surface, anchor });
        }
    }

    fn attach_overlay(
        &mut self,
        surface: SurfaceId,
        node: NodeId,
        descriptor: Rc<dyn WindowRootDescriptor>,
        size: SurfaceSize,
    ) {
        let kind = SurfaceKind::Overlay(descriptor);
        let (width, height) = size.viewport();
        kind.set_content_size(Size::new(width, height));
        let renderer = renderer_for(&self.text_system, &self.gpu, size.scale, true);
        let key = node as u64;
        self.add_surface(key, renderer, size);
        self.surfaces.insert(
            surface,
            SurfaceSlot::new(surface, RootId::Window(key), kind, &self.gpu.device, size),
        );
    }

    fn close_overlay(&mut self, node: NodeId) {
        let Some(surface) = self.overlays.remove(&node) else {
            return;
        };
        if self.pending_overlays.remove(&surface).is_none() {
            self.shell.remove_window_surface(node as u64);
            self.surfaces.remove(&surface);
        }
        self.commands
            .borrow_mut()
            .push(SurfaceCommand::Close(surface));
    }

    pub(crate) fn take_commands(&mut self) -> Vec<SurfaceCommand> {
        std::mem::take(&mut *self.commands.borrow_mut())
    }

    pub(crate) fn produce_frames(
        &mut self,
        mut emit: impl FnMut(FrameUpdate<'_>) -> io::Result<()>,
    ) -> Result<(), EmbedError> {
        let Self {
            shell,
            gpu,
            surfaces,
            ..
        } = self;
        for slot in surfaces.values_mut().filter(|slot| slot.has_room()) {
            let Some(mut target) = shell.surface(slot.root) else {
                continue;
            };
            if let Some(frame) = slot.produce(&mut target, &gpu.device, &gpu.queue)? {
                emit(frame)?;
            }
        }
        Ok(())
    }

    pub(crate) fn take_cursor_changes(&mut self) -> Vec<(SurfaceId, &'static str)> {
        let Self {
            shell, surfaces, ..
        } = self;
        surfaces
            .values()
            .filter_map(|slot| {
                let target = shell.surface(slot.root)?;
                let icon = target.take_pointer_icon_change()?;
                Some((slot.id, cursor_name(&icon)))
            })
            .collect()
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
    last_tick_at: Option<Instant>,
}

impl Pacing {
    pub(crate) fn new() -> Self {
        Self { last_tick_at: None }
    }

    pub(crate) fn ticked(&mut self, at: Instant) {
        self.last_tick_at = Some(at);
    }

    pub(crate) fn may_tick(&self, interval: Duration, now: Instant) -> bool {
        self.last_tick_at
            .is_none_or(|at| now.saturating_duration_since(at) >= interval)
    }

    pub(crate) fn next_wake(
        &self,
        schedule: FrameSchedule,
        surface_dirty: bool,
        interval: Duration,
        now: Instant,
    ) -> Option<Instant> {
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
        let wake = host.can_draw().then(|| {
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
        if !apply_batch(host, writer, batch)? {
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
    writer: &mut impl Write,
    batch: Vec<LoopEvent>,
) -> Result<bool, EmbedError> {
    for event in batch {
        match event {
            LoopEvent::Host(HostEvent::Close) | LoopEvent::Disconnected => return Ok(false),
            LoopEvent::Host(HostEvent::Message { channel, .. })
                if channel == INSPECT_REQUEST_CHANNEL =>
            {
                let report = host.shell.debug_info_report();
                write_app_event(
                    writer,
                    &AppEvent::Message {
                        channel: INSPECT_SNAPSHOT_CHANNEL,
                        payload: &report,
                    },
                )?;
            }
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
    if host.can_draw() && pacing.may_tick(host.size().frame_interval(), now) {
        pacing.ticked(now);
        host.update();
        write_commands(host, writer)?;
        host.produce_frames(|frame| write_app_event(writer, &AppEvent::Frame(frame)))?;
    }
    write_commands(host, writer)?;
    for (surface, name) in host.take_cursor_changes() {
        write_app_event(writer, &AppEvent::Cursor { surface, name })?;
    }
    writer.flush()?;
    Ok(())
}

fn write_commands(host: &mut EmbeddedHost, writer: &mut impl Write) -> io::Result<()> {
    for command in host.take_commands() {
        write_app_event(writer, &AppEvent::Command(&command))?;
    }
    Ok(())
}

fn await_first_size(
    events: &Receiver<LoopEvent>,
    platform_env: &PlatformEnvironment,
) -> Option<SurfaceSize> {
    while let Ok(event) = events.recv() {
        match event {
            LoopEvent::Host(HostEvent::Surface {
                surface: PRIMARY_SURFACE,
                event:
                    SurfaceEvent::Resize {
                        width,
                        height,
                        scale,
                        refresh_hz,
                    },
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
