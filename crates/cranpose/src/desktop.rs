//! Desktop runtime for Compose applications.
//!
//! This module provides the desktop event loop implementation using winit.

use crate::desktop_input::dispatch_keyboard_input;
use crate::launcher::{AppSettings, LaunchError};
use crate::native_window::{
    self, NativeWindowEvents, NativeWindowKey, NativeWindowOptions, NativeWindowPositionOrigin,
    NativeWindowRequest, WindowGraphMove, WindowGraphNodeSnapshot, WindowGraphPeerSnapshot,
    WindowGraphState, WindowGroupId, WindowResizeDirection, WindowState,
};
#[cfg(feature = "robot")]
use crate::robot::{
    char_to_key_code, extract_semantics, find_button_in_app, find_text_in_app,
    panic_payload_message, robot_key_code_and_text, robot_wait_for_idle_animation_loop_only, Robot,
    RobotChannel, RobotCommand, RobotResponse, RobotScreenshot,
};
use crate::wgpu_surface::surface_present_required;
use crate::wgpu_surface::{current_surface_texture, SurfaceFrame};
use cranpose_app_shell::{
    default_root_key, AppShell, FramePacingMode, FrameUpdateResult, PointerSource,
};
use cranpose_platform_desktop_winit::DesktopWinitPlatform;
use cranpose_render_wgpu::{WgpuRenderer, WgpuTextSystem};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize, Position};
use winit::event::{
    ButtonSource, ElementState, MouseButton, PointerSource as WinitPointerSource, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{
    ResizeDirection, Window, WindowAttributes, WindowId as WinitWindowId, WindowLevel,
};

const NATIVE_WINDOW_DRAG_POLL_INTERVAL: Duration = Duration::ZERO;
const NATIVE_WINDOW_POSITION_POLL_INTERVAL: Duration = Duration::from_millis(16);
const NATIVE_WINDOW_POSITION_SETTLE_TIMEOUT: Duration = Duration::from_millis(36);
const NATIVE_WINDOW_POSITION_SETTLE_POLL: Duration = Duration::from_millis(1);
const NATIVE_WINDOW_PLACEMENT_MARGIN: f32 = 32.0;
#[cfg(feature = "robot")]
const ROBOT_PUMP_FRAME_INTERVAL: Duration = Duration::from_nanos(16_666_667);
const DEFAULT_DESKTOP_FRAME_TELEMETRY_THRESHOLD_MS: f64 = 4.0;

/// Maps a winit pointer-move source to the framework [`PointerSource`].
fn pointer_source_from_winit(source: &WinitPointerSource) -> PointerSource {
    match source {
        WinitPointerSource::Mouse => PointerSource::Mouse,
        WinitPointerSource::Touch { .. } => PointerSource::Touch,
        WinitPointerSource::TabletTool { .. } => PointerSource::Stylus,
        _ => PointerSource::Unknown,
    }
}

/// Maps a winit button source (mouse click / touch / tablet) to [`PointerSource`].
fn pointer_source_from_button(button: &ButtonSource) -> PointerSource {
    match button {
        ButtonSource::Mouse(_) => PointerSource::Mouse,
        ButtonSource::Touch { .. } => PointerSource::Touch,
        ButtonSource::TabletTool { .. } => PointerSource::Stylus,
        _ => PointerSource::Unknown,
    }
}

#[cfg(feature = "robot")]
use std::sync::mpsc;

fn update_app_with_native_window_registry(
    app: &mut AppShell<WgpuRenderer>,
    registry: &Rc<native_window::NativeWindowRegistry>,
) -> FrameUpdateResult {
    native_window::with_native_window_registry(registry, || app.update())
}

fn desktop_frame_telemetry_threshold_ms() -> Option<f64> {
    static THRESHOLD_MS: std::sync::OnceLock<Option<f64>> = std::sync::OnceLock::new();
    *THRESHOLD_MS.get_or_init(|| {
        let explicit = std::env::var("CRANPOSE_DESKTOP_FRAME_TELEMETRY_MS")
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value >= 0.0);
        explicit.or_else(|| {
            std::env::var_os("CRANPOSE_DESKTOP_FRAME_TELEMETRY")
                .is_some()
                .then_some(DEFAULT_DESKTOP_FRAME_TELEMETRY_THRESHOLD_MS)
        })
    })
}

fn log_desktop_frame_telemetry(
    frame_start: Instant,
    after_update: Instant,
    after_acquire: Instant,
    after_render: Instant,
    after_present: Instant,
    label: &str,
) {
    let Some(threshold_ms) = desktop_frame_telemetry_threshold_ms() else {
        return;
    };

    let total_ms = after_present.duration_since(frame_start).as_secs_f64() * 1000.0;
    if total_ms < threshold_ms {
        return;
    }

    let update_ms = after_update.duration_since(frame_start).as_secs_f64() * 1000.0;
    let acquire_ms = after_acquire.duration_since(after_update).as_secs_f64() * 1000.0;
    let render_ms = after_render.duration_since(after_acquire).as_secs_f64() * 1000.0;
    let present_ms = after_present.duration_since(after_render).as_secs_f64() * 1000.0;
    log::warn!(
        "[desktop-frame-telemetry:{label}] total_ms={total_ms:.2} update_ms={update_ms:.2} acquire_ms={acquire_ms:.2} render_ms={render_ms:.2} present_ms={present_ms:.2}",
    );
}

#[cfg(feature = "robot")]
fn pump_robot_frame(
    app: &mut AppShell<WgpuRenderer>,
    registry: &Rc<native_window::NativeWindowRegistry>,
) {
    for _ in 0..3 {
        if !robot_query_should_drain_frame(app) {
            break;
        }
        update_app_with_native_window_registry(app, registry);
    }
}

#[cfg(feature = "robot")]
fn robot_query_should_drain_frame(app: &AppShell<WgpuRenderer>) -> bool {
    app.needs_redraw()
}

/// Robot controller for the event loop
#[cfg(feature = "robot")]
struct RobotController {
    rx: mpsc::Receiver<RobotCommand>,
    tx: mpsc::Sender<RobotResponse>,
    waiting_for_idle: bool,
    idle_iterations: u32,
    idle_structure_clean_frames: u32,
    waiting_for_present_generation: Option<u64>,
    waiting_for_pump_present_generation: Option<u64>,
    scroll_sequence: Option<RobotScrollSequence>,
    synthetic_primary_down: bool,
}

#[cfg(feature = "robot")]
#[derive(Clone, Copy, Debug)]
struct RobotScrollSequence {
    delta_x: f32,
    delta_y: f32,
    remaining: u32,
}

#[cfg(feature = "robot")]
impl RobotController {
    fn new() -> (Self, Robot) {
        let (channel, robot) = RobotChannel::new();
        let RobotChannel { rx, tx } = channel;

        let controller = RobotController {
            rx,
            tx,
            waiting_for_idle: false,
            idle_iterations: 0,
            idle_structure_clean_frames: 0,
            waiting_for_present_generation: None,
            waiting_for_pump_present_generation: None,
            scroll_sequence: None,
            synthetic_primary_down: false,
        };

        (controller, robot)
    }

    fn begin_synthetic_primary_gesture(&mut self) {
        self.synthetic_primary_down = true;
    }

    fn end_synthetic_primary_gesture(&mut self) {
        self.synthetic_primary_down = false;
    }

    fn synthetic_primary_down(&self) -> bool {
        self.synthetic_primary_down
    }

    fn start_idle_wait(&mut self) {
        self.waiting_for_idle = true;
        self.idle_iterations = 0;
        self.idle_structure_clean_frames = 0;
    }

    fn finish_idle_wait(&mut self) {
        self.waiting_for_idle = false;
        self.waiting_for_present_generation = None;
        self.idle_structure_clean_frames = 0;
    }

    fn record_idle_update_result(&mut self, result: FrameUpdateResult) {
        if !self.waiting_for_idle {
            return;
        }
        if result.structure_changed {
            self.idle_structure_clean_frames = 0;
        } else if result.visual_changed {
            self.idle_structure_clean_frames = self.idle_structure_clean_frames.saturating_add(1);
        }
    }
}

/// Application state that implements winit's ApplicationHandler
struct DesktopGpuContext {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    adapter_backend: wgpu::Backend,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    text_system: WgpuTextSystem,
}

struct NativeWindowSurface {
    key: NativeWindowKey,
    revision: u64,
    options: NativeWindowOptions,
    events: NativeWindowEvents,
    state: Option<WindowState>,
    group: Option<native_window::NativeWindowGroupMembership>,
    window: Arc<dyn Window>,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    surface_caps: wgpu::SurfaceCapabilities,
    surface_dirty: bool,
    app: AppShell<WgpuRenderer>,
    platform: DesktopWinitPlatform,
    last_cursor_position: Option<(f32, f32)>,
    last_cursor_physical_position: Option<PhysicalPosition<f64>>,
    frame_pacing_mode: FramePacingMode,
    last_frame_start_time: Option<Instant>,
    vsync_interval: Duration,
    pending_outer_positions: PendingNativeWindowPositions,
    active_drag: Option<NativeWindowDragSession>,
}

struct NativeWindowShell {
    request: NativeWindowRequest,
    window: Arc<dyn Window>,
    create_started: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct DesktopRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl DesktopRect {
    fn right(self) -> f32 {
        self.x + self.width
    }

    fn bottom(self) -> f32 {
        self.y + self.height
    }

    fn center(self) -> cranpose_ui::Point {
        cranpose_ui::Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    fn contains_rect_with_margin(self, rect: Self, margin: f32) -> bool {
        rect.x >= self.x + margin
            && rect.right() <= self.right() - margin
            && rect.y >= self.y + margin
            && rect.bottom() <= self.bottom() - margin
    }

    fn distance_to_point(self, point: cranpose_ui::Point) -> f32 {
        let dx = if point.x < self.x {
            self.x - point.x
        } else if point.x > self.right() {
            point.x - self.right()
        } else {
            0.0
        };
        let dy = if point.y < self.y {
            self.y - point.y
        } else if point.y > self.bottom() {
            point.y - self.bottom()
        } else {
            0.0
        };
        dx * dx + dy * dy
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum NativeWindowPlacementGroupKey {
    Group(WindowGroupId),
    Window(NativeWindowKey),
}

#[derive(Clone, Copy, Debug)]
enum NativeWindowDragSession {
    Platform { next_poll_at: Instant },
    Polling(NativeWindowPollingDragSession),
}

impl NativeWindowDragSession {
    fn platform(now: Instant) -> Self {
        Self::Platform {
            next_poll_at: now + NATIVE_WINDOW_DRAG_POLL_INTERVAL,
        }
    }

    fn next_poll_at(self) -> Instant {
        match self {
            Self::Platform { next_poll_at } => next_poll_at,
            Self::Polling(session) => session.next_poll_at,
        }
    }

    fn set_next_poll_at(&mut self, next_poll_at: Instant) {
        match self {
            Self::Platform {
                next_poll_at: current,
                ..
            }
            | Self::Polling(NativeWindowPollingDragSession {
                next_poll_at: current,
                ..
            }) => {
                *current = next_poll_at;
            }
        }
    }

    fn polling_mut(&mut self) -> Option<&mut NativeWindowPollingDragSession> {
        match self {
            Self::Platform { .. } => None,
            Self::Polling(session) => Some(session),
        }
    }

    fn finishes_on_global_pointer_release(self) -> bool {
        true
    }

    fn uses_moved_events_as_drag_target(self) -> bool {
        matches!(self, Self::Platform { .. })
    }
}

#[derive(Clone, Copy, Debug)]
struct NativeWindowPollingDragSession {
    start_pointer_screen: PhysicalPosition<f64>,
    start_window_outer: PhysicalPosition<i32>,
    last_target_outer: PhysicalPosition<i32>,
    next_poll_at: Instant,
}

struct NativeWindowPositionRequest {
    window: Arc<dyn Window>,
    logical: LogicalPosition<f64>,
    physical: PhysicalPosition<i32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeWindowPositionApplyMode {
    WaitForSettle,
    FlushOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeWindowPositionObservation {
    Current,
    Superseded,
    External,
}

impl NativeWindowPollingDragSession {
    fn new(
        start_pointer_screen: PhysicalPosition<f64>,
        start_window_outer: PhysicalPosition<i32>,
        now: Instant,
    ) -> Self {
        Self {
            start_pointer_screen,
            start_window_outer,
            last_target_outer: start_window_outer,
            next_poll_at: now + NATIVE_WINDOW_DRAG_POLL_INTERVAL,
        }
    }

    fn target_for_pointer(&self, pointer: PhysicalPosition<f64>) -> PhysicalPosition<i32> {
        PhysicalPosition::new(
            (self.start_window_outer.x as f64 + pointer.x - self.start_pointer_screen.x).round()
                as i32,
            (self.start_window_outer.y as f64 + pointer.y - self.start_pointer_screen.y).round()
                as i32,
        )
    }
}

#[derive(Default)]
struct PendingNativeWindowPositions {
    positions: VecDeque<(f32, f32)>,
}

#[derive(Clone, Copy)]
enum NativeWindowGraphPositionSource {
    CachedThenCurrent,
    CurrentThenCached,
}

impl PendingNativeWindowPositions {
    fn push(&mut self, position: (f32, f32)) {
        if self
            .positions
            .back()
            .is_some_and(|pending| native_window_positions_close(*pending, position))
        {
            return;
        }
        self.positions.push_back(position);
        while self.positions.len() > 16 {
            self.positions.pop_front();
        }
    }

    fn acknowledge(&mut self, position: (f32, f32)) -> bool {
        let Some(index) = self
            .positions
            .iter()
            .position(|pending| native_window_positions_close(*pending, position))
        else {
            return false;
        };
        for _ in 0..=index {
            self.positions.pop_front();
        }
        true
    }

    fn acknowledge_or_matches_known(
        &mut self,
        position: (f32, f32),
        known_position: Option<(f32, f32)>,
    ) -> NativeWindowPositionObservation {
        if self.acknowledge(position) {
            return if self.has_pending() {
                NativeWindowPositionObservation::Superseded
            } else {
                NativeWindowPositionObservation::Current
            };
        }
        if known_position.is_some_and(|known| native_window_positions_close(known, position)) {
            self.clear();
            return NativeWindowPositionObservation::Current;
        }
        NativeWindowPositionObservation::External
    }

    fn clear(&mut self) {
        self.positions.clear();
    }

    fn has_pending(&self) -> bool {
        !self.positions.is_empty()
    }
}

impl NativeWindowSurface {
    fn frame_interval(&self) -> Option<Duration> {
        frame_interval_for_mode(self.frame_pacing_mode, self.vsync_interval)
    }
}

struct App {
    /// Settings for the application
    settings: AppSettings,
    /// Live platform environment (system theme, insets) provided to composition.
    platform_env: Rc<crate::platform_env::PlatformEnvironment>,
    /// Content function to be called (taken on first resume)
    content: Option<Box<dyn FnMut()>>,
    /// Window (created when surfaces can be created)
    window: Option<Arc<dyn Window>>,
    /// WGPU surface
    surface: Option<wgpu::Surface<'static>>,
    /// Surface configuration
    surface_config: Option<wgpu::SurfaceConfiguration>,
    /// Surface capabilities used when switching present modes at runtime.
    surface_caps: Option<wgpu::SurfaceCapabilities>,
    /// Compose app shell
    app: Option<AppShell<WgpuRenderer>>,
    /// Platform adapter
    platform: Option<DesktopWinitPlatform>,
    /// Shared GPU objects used by all desktop surfaces.
    gpu_context: Option<DesktopGpuContext>,
    /// Native sub-window surfaces keyed by the operating-system window id.
    native_windows: HashMap<WinitWindowId, NativeWindowSurface>,
    /// Declarative native-window requests owned by this app instance.
    native_window_registry: Rc<native_window::NativeWindowRegistry>,
    /// Declarative native sub-window ids mapped to their current OS window id.
    native_window_ids: HashMap<NativeWindowKey, WinitWindowId>,
    /// Last observed OS positions for declarative native sub-windows.
    native_window_positions: HashMap<NativeWindowKey, (f32, f32)>,
    /// Native sub-windows closed by the user while still declared by composition.
    closed_native_windows: HashSet<NativeWindowKey>,
    /// Framework-owned peer-window topology and drag sessions.
    window_graph: WindowGraphState,
    next_native_window_position_poll_at: Instant,
    native_window_platform_probe: NativeWindowPlatformProbe,
    native_global_primary_down: bool,
    /// Current keyboard modifiers (shift, ctrl, alt, meta)
    current_modifiers: winit::keyboard::ModifiersState,
    /// Last known cursor position in logical pixels
    last_cursor_position: Option<(f32, f32)>,
    /// Robot controller
    #[cfg(feature = "robot")]
    robot_controller: Option<RobotController>,
    /// Optional robot hook executed on the app thread for deterministic test control.
    #[cfg(feature = "robot")]
    robot_app_hook: Option<Box<crate::RobotAppHook>>,
    /// Input recorder for generating robot tests
    recorder: Option<crate::recorder::InputRecorder>,
    /// Launch failure captured during window/GPU initialization.
    launch_error: Rc<RefCell<Option<LaunchError>>>,
    /// Event-loop wake path used when the primary declaration host is hidden.
    event_proxy: EventLoopProxy,
    frame_pacing_mode: FramePacingMode,
    last_frame_start_time: Option<Instant>,
    primary_redraw_pending: bool,
    primary_surface_dirty: bool,
    /// True while the very first frame still owes the surface a present. macOS
    /// can drop the `request_redraw` issued during `resumed`, leaving a static
    /// initial UI blank until an input event; `about_to_wait` re-requests the
    /// redraw while this is set so the first frame always reaches the screen.
    primary_initial_present_pending: bool,
    vsync_interval: Duration,
    #[cfg(feature = "robot")]
    presented_frame_generation: u64,
    #[cfg(feature = "robot")]
    robot_visible_surface_dirty: bool,
}

impl App {
    fn new(
        mut settings: AppSettings,
        content: impl FnMut() + 'static,
        launch_error: Rc<RefCell<Option<LaunchError>>>,
        event_proxy: EventLoopProxy,
    ) -> Self {
        // Create recorder if recording is enabled
        let recorder = settings
            .record_to
            .take()
            .map(crate::recorder::InputRecorder::new);
        #[cfg(feature = "robot")]
        let robot_app_hook = settings.robot_app_hook.take();
        let frame_pacing_mode = settings.frame_pacing_mode;

        // Wrap the app content once with the platform environment (system
        // theme, insets) so every composition — primary or native sub-window —
        // observes the same live values.
        let platform_env = crate::platform_env::PlatformEnvironment::new();
        let env_for_content = Rc::clone(&platform_env);
        let mut content = content;
        let content = move || env_for_content.compose_root(&mut content);

        Self {
            settings,
            platform_env,
            content: Some(Box::new(content)),
            window: None,
            surface: None,
            surface_config: None,
            surface_caps: None,
            app: None,
            platform: None,
            gpu_context: None,
            native_windows: HashMap::new(),
            native_window_registry: Rc::new(native_window::NativeWindowRegistry::default()),
            native_window_ids: HashMap::new(),
            native_window_positions: HashMap::new(),
            closed_native_windows: HashSet::new(),
            window_graph: WindowGraphState::default(),
            next_native_window_position_poll_at: Instant::now()
                + NATIVE_WINDOW_POSITION_POLL_INTERVAL,
            // `NativeWindowPlatformProbe` is a unit struct on macOS but carries an
            // x11 client field on Linux, so `::default()` is required there. The
            // `default_constructed_unit_structs` lint only fires on the macOS
            // shape (a local-only false positive; Linux sees the field form and
            // the allow is simply unfulfilled there).
            #[allow(clippy::default_constructed_unit_structs)]
            native_window_platform_probe: NativeWindowPlatformProbe::default(),
            native_global_primary_down: false,
            current_modifiers: winit::keyboard::ModifiersState::empty(),
            last_cursor_position: None,
            #[cfg(feature = "robot")]
            robot_controller: None,
            #[cfg(feature = "robot")]
            robot_app_hook,
            recorder,
            launch_error,
            event_proxy,
            frame_pacing_mode,
            last_frame_start_time: None,
            primary_redraw_pending: false,
            primary_surface_dirty: false,
            primary_initial_present_pending: false,
            vsync_interval: default_vsync_interval(),
            #[cfg(feature = "robot")]
            presented_frame_generation: 0,
            #[cfg(feature = "robot")]
            robot_visible_surface_dirty: false,
        }
    }

    fn abort_launch(&self, event_loop: &dyn ActiveEventLoop, error: LaunchError) {
        let mut slot = self.launch_error.borrow_mut();
        if slot.is_none() {
            *slot = Some(error);
        }
        event_loop.exit();
    }

    fn frame_interval(&self) -> Option<Duration> {
        frame_interval_for_mode(self.frame_pacing_mode, self.vsync_interval)
    }

    fn refresh_native_window_requests(&mut self) {
        let registry = Rc::clone(&self.native_window_registry);
        if let Some(app) = &mut self.app {
            update_app_with_native_window_registry(app, &registry);
        }
    }

    fn refresh_and_sync_native_windows(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.refresh_native_window_requests();
        self.sync_native_windows(event_loop);
    }

    fn handle_primary_frame_requested(&mut self, event_loop: &dyn ActiveEventLoop) {
        let registry = Rc::clone(&self.native_window_registry);
        let direct_declaration_update = {
            let Some(app) = &mut self.app else {
                return;
            };
            let needs_redraw = app.frame_schedule().needs_frame;
            if !needs_redraw {
                return;
            }
            let direct_declaration_update = primary_declaration_host_needs_direct_update(
                self.settings.primary_window_visible,
                self.settings.headless,
                needs_redraw,
                false,
            );
            if direct_declaration_update {
                trace_native_window(format_args!(
                    "primary declaration host proxy update visible={} headless={}",
                    self.settings.primary_window_visible, self.settings.headless
                ));
                let frame_started_at = Instant::now();
                let update_result = update_app_with_native_window_registry(app, &registry);
                #[cfg(feature = "robot")]
                if let Some(controller) = &mut self.robot_controller {
                    controller.record_idle_update_result(update_result);
                }
                if update_result.visual_changed {
                    app.record_presented_frame(frame_started_at, Instant::now());
                    self.last_frame_start_time = Some(frame_started_at);
                }
            }
            direct_declaration_update
        };

        if direct_declaration_update {
            self.sync_native_windows(event_loop);
        } else if let Some(window) = &self.window {
            request_redraw_once(window, &mut self.primary_redraw_pending);
        }
    }

    fn poll_primary_pointer_gesture(&mut self) -> bool {
        let active = self
            .app
            .as_ref()
            .is_some_and(AppShell::has_active_pointer_gesture);
        if !active {
            return false;
        }

        #[cfg(feature = "robot")]
        if self
            .robot_controller
            .as_ref()
            .is_some_and(RobotController::synthetic_primary_down)
        {
            return false;
        }

        let Some(pointer) = native_window_global_pointer_state(&self.native_window_platform_probe)
        else {
            return false;
        };

        let (Some(window), Some(platform), Some(app)) =
            (&self.window, &self.platform, self.app.as_mut())
        else {
            return false;
        };

        if !pointer.primary_down {
            let handled = app.pointer_released();
            if handled {
                self.last_frame_start_time = None;
                request_redraw_once(window, &mut self.primary_redraw_pending);
            }
            return handled;
        }

        if self.last_cursor_position.is_some() {
            return false;
        }

        let Some(local) = native_window_local_pointer_physical(
            &self.native_window_platform_probe,
            window,
            pointer.position,
        ) else {
            return false;
        };
        let logical = platform.pointer_position(local);
        self.last_cursor_position = Some((logical.x, logical.y));
        if app.set_cursor(logical.x, logical.y) {
            self.last_frame_start_time = None;
            request_redraw_once(window, &mut self.primary_redraw_pending);
            return true;
        }

        false
    }

    fn refresh_primary_cursor_from_platform_pointer(
        platform_probe: &NativeWindowPlatformProbe,
        window: &Arc<dyn Window>,
        platform: &DesktopWinitPlatform,
        app: &mut AppShell<WgpuRenderer>,
        last_cursor_position: &mut Option<(f32, f32)>,
    ) -> bool {
        let Some(pointer) = native_window_global_pointer_state(platform_probe) else {
            return false;
        };

        let Some(local) =
            native_window_local_pointer_physical(platform_probe, window, pointer.position)
        else {
            return false;
        };

        let logical = platform.pointer_position(local);
        *last_cursor_position = Some((logical.x, logical.y));
        app.set_cursor(logical.x, logical.y)
    }

    fn refresh_native_cursor_from_platform_pointer(
        platform_probe: &NativeWindowPlatformProbe,
        native: &mut NativeWindowSurface,
    ) -> bool {
        let Some(pointer) = native_window_global_pointer_state(platform_probe) else {
            return false;
        };

        let Some(local) =
            native_window_local_pointer_physical(platform_probe, &native.window, pointer.position)
        else {
            return false;
        };

        let logical = native.platform.pointer_position(local);
        native.last_cursor_position = Some((logical.x, logical.y));
        native.last_cursor_physical_position = Some(local);
        native_window::with_native_window_surface_origin(
            native_window_surface_origin(platform_probe, &native.window),
            || native.app.set_cursor(logical.x, logical.y),
        )
    }

    fn sync_native_windows(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.gpu_context.is_none() {
            return;
        }

        let has_requests = native_window::has_native_window_requests(&self.native_window_registry);
        if self.native_windows.is_empty() && self.closed_native_windows.is_empty() && !has_requests
        {
            return;
        }
        if !has_requests
            && self.closed_native_windows.is_empty()
            && self
                .native_windows
                .values()
                .all(|native| !native.options.visible)
        {
            return;
        }

        let sync_started = Instant::now();
        let requests = native_window::native_window_requests(&self.native_window_registry);
        let active_keys: HashSet<NativeWindowKey> =
            requests.iter().map(|request| request.key).collect();
        trace_native_window_timing(format_args!(
            "sync start requests={} existing={}",
            requests.len(),
            self.native_windows.len()
        ));

        self.closed_native_windows
            .retain(|key| active_keys.contains(key));

        let stale_window_ids: Vec<WinitWindowId> = self
            .native_windows
            .iter()
            .filter_map(|(window_id, native)| {
                (!active_keys.contains(&native.key)).then_some(*window_id)
            })
            .collect();
        for window_id in stale_window_ids {
            if let Some(native) = self.native_windows.get(&window_id) {
                trace_native_window(format_args!(
                    "sync stale key={:?} title={:?} visible={}",
                    native.key, native.options.title, native.options.visible
                ));
                if let Some((x, y)) =
                    current_native_window_position(&self.native_window_platform_probe, native)
                {
                    self.native_window_positions.insert(native.key, (x, y));
                    notify_native_window_moved(&native.events, x, y);
                }
            }
            if let Some(native) = self.native_windows.get_mut(&window_id) {
                if native.options.visible {
                    native.window.set_visible(false);
                    native.options.visible = false;
                    cancel_app_input(&mut native.app);
                }
            }
        }

        let mut native_windows_to_create = Vec::new();
        for request in requests {
            if self.closed_native_windows.contains(&request.key) {
                trace_native_window(format_args!(
                    "sync skip closed key={:?} title={:?}",
                    request.key, request.options.title
                ));
                continue;
            }

            let request = self.native_window_request_for_host(&request);
            if let Some(window_id) = self.native_window_ids.get(&request.key).copied() {
                if let Some(native) = self.native_windows.get_mut(&window_id) {
                    native.events = request.events.clone();
                    native.state = request.state;
                    native.group = request.group.clone();
                    let revision_changed = native.revision != request.revision;
                    let options_changed = native.options != request.options;
                    let position_only_options_change = options_changed
                        && native_window_options_change_is_position_only(
                            &native.options,
                            &request.options,
                        );
                    Self::apply_native_window_options(
                        &self.native_window_platform_probe,
                        native,
                        &request.options,
                        self.settings.headless,
                    );
                    if revision_changed {
                        native.revision = request.revision;
                        if position_only_options_change {
                            trace_native_window(format_args!(
                                "sync update position key={:?} title={:?}",
                                native.key, native.options.title
                            ));
                        } else {
                            trace_native_window(format_args!(
                                "sync update content key={:?} title={:?}",
                                native.key, native.options.title
                            ));
                            native.app.request_root_render();
                            native.window.request_redraw();
                        }
                    } else if options_changed && request.options.visible {
                        trace_native_window(format_args!(
                            "sync update options key={:?} title={:?} visible={}",
                            native.key, native.options.title, request.options.visible
                        ));
                        native.window.request_redraw();
                    }
                    continue;
                }
                self.native_window_ids.remove(&request.key);
            }

            native_windows_to_create.push(request);
        }

        self.place_initial_native_windows_on_visible_monitors(
            event_loop,
            &mut native_windows_to_create,
        );

        let mut native_window_shells = Vec::with_capacity(native_windows_to_create.len());
        for request in native_windows_to_create {
            trace_native_window(format_args!(
                "sync create key={:?} title={:?} visible={}",
                request.key, request.options.title, request.options.visible
            ));
            match Self::create_native_window_shell(event_loop, request, self.settings.headless) {
                Ok(shell) => native_window_shells.push(shell),
                Err(error) => {
                    self.abort_launch(event_loop, error);
                    return;
                }
            }
        }

        for shell in native_window_shells {
            match self.create_native_window(shell) {
                Ok(native) => {
                    let window_id = native.window.id();
                    self.remember_native_window_position(&native);
                    self.native_window_ids.insert(native.key, window_id);
                    self.native_windows.insert(window_id, native);
                }
                Err(error) => {
                    self.abort_launch(event_loop, error);
                    return;
                }
            }
        }
        trace_native_window_timing(format_args!(
            "sync done in {}ms",
            sync_started.elapsed().as_millis()
        ));
        if self.hidden_bootstrap_has_no_visible_peer_windows() {
            event_loop.exit();
        }
    }

    fn hidden_bootstrap_has_no_visible_peer_windows(&self) -> bool {
        if self.settings.primary_window_visible || self.settings.headless {
            return false;
        }
        if self
            .native_windows
            .values()
            .any(|native| native.options.visible)
        {
            return false;
        }

        let requests = native_window::native_window_requests(&self.native_window_registry);
        requests.is_empty()
            || requests
                .iter()
                .all(|request| self.closed_native_windows.contains(&request.key))
    }

    fn place_initial_native_windows_on_visible_monitors(
        &self,
        event_loop: &dyn ActiveEventLoop,
        requests: &mut [NativeWindowRequest],
    ) {
        let monitors = logical_monitor_rects(event_loop);
        if monitors.is_empty() {
            return;
        }

        let mut groups: HashMap<NativeWindowPlacementGroupKey, Vec<usize>> = HashMap::new();
        for (index, request) in requests.iter().enumerate() {
            if !request.options.visible
                || !Self::native_window_options_have_screen_position(&request.options)
            {
                continue;
            }
            let key = request.group.as_ref().map_or(
                NativeWindowPlacementGroupKey::Window(request.key),
                |group| NativeWindowPlacementGroupKey::Group(group.id),
            );
            groups.entry(key).or_default().push(index);
        }

        for indices in groups.values() {
            let Some(bounds) = native_window_request_bounds(requests, indices) else {
                continue;
            };
            if monitors.iter().any(|monitor| {
                monitor.contains_rect_with_margin(bounds, NATIVE_WINDOW_PLACEMENT_MARGIN)
            }) {
                continue;
            }

            let Some(monitor) = nearest_monitor_to_rect(&monitors, bounds) else {
                continue;
            };
            let delta =
                clamp_rect_to_monitor_delta(bounds, monitor, NATIVE_WINDOW_PLACEMENT_MARGIN);
            if delta.x.abs() <= f32::EPSILON && delta.y.abs() <= f32::EPSILON {
                continue;
            }

            for index in indices {
                let options = &mut requests[*index].options;
                if let (Some(x), Some(y)) = (options.x, options.y) {
                    options.x = Some(x + delta.x);
                    options.y = Some(y + delta.y);
                }
            }
        }
    }

    fn native_window_request_for_host(&self, request: &NativeWindowRequest) -> NativeWindowRequest {
        let mut request = request.clone();
        Self::apply_native_window_state_to_options(&mut request.options, request.state);
        if Self::native_window_options_have_screen_position(&request.options) {
            request.options = self.resolve_native_window_options(&request.options);
        } else if let Some((x, y)) = self.native_window_positions.get(&request.key).copied() {
            request.options.x = Some(x);
            request.options.y = Some(y);
            request.options.position_origin = NativeWindowPositionOrigin::Screen;
        } else {
            request.options = self.resolve_native_window_options(&request.options);
        }
        request
    }

    fn apply_native_window_state_to_options(
        options: &mut NativeWindowOptions,
        state: Option<WindowState>,
    ) {
        let Some(state) = state else {
            return;
        };
        let size = state.size_non_reactive();
        options.width = size.width;
        options.height = size.height;
        if let Some(position) = state.position_non_reactive() {
            options.x = Some(position.x);
            options.y = Some(position.y);
            options.position_origin = NativeWindowPositionOrigin::Screen;
        }
    }

    fn resolve_native_window_options(&self, options: &NativeWindowOptions) -> NativeWindowOptions {
        let mut options = options.clone();
        if options.position_origin == NativeWindowPositionOrigin::HostWindow {
            if let (Some(x), Some(y), Some((host_x, host_y))) =
                (options.x, options.y, self.host_window_position())
            {
                options.x = Some(host_x + x);
                options.y = Some(host_y + y);
            }
            options.position_origin = NativeWindowPositionOrigin::Screen;
        }
        options
    }

    fn native_window_options_have_screen_position(options: &NativeWindowOptions) -> bool {
        options.position_origin == NativeWindowPositionOrigin::Screen
            && options.x.is_some()
            && options.y.is_some()
    }

    fn host_window_position(&self) -> Option<(f32, f32)> {
        let window = self.window.as_ref()?;
        logical_outer_position(window)
    }

    fn remember_native_window_position(&mut self, native: &NativeWindowSurface) {
        if let Some((x, y)) = Self::initial_native_window_position(
            &native.options,
            current_native_window_position(&self.native_window_platform_probe, native),
        ) {
            self.native_window_positions.insert(native.key, (x, y));
            if let Some(state) = native.state {
                if state.position_non_reactive().is_none() {
                    state.set_position(Some(cranpose_ui::Point::new(x, y)));
                }
            }
            notify_native_window_moved(&native.events, x, y);
        }
    }

    fn initial_native_window_position(
        options: &NativeWindowOptions,
        current_position: Option<(f32, f32)>,
    ) -> Option<(f32, f32)> {
        if options.position_origin == NativeWindowPositionOrigin::Screen {
            if let Some(position) = options.x.zip(options.y) {
                return Some(position);
            }
        }

        current_position
    }

    fn native_window_graph_snapshots(&self) -> Vec<WindowGraphPeerSnapshot> {
        self.native_windows
            .values()
            .filter_map(|native| {
                self.native_window_graph_snapshot(
                    native,
                    None,
                    NativeWindowGraphPositionSource::CachedThenCurrent,
                )
            })
            .collect()
    }

    fn native_window_graph_snapshots_with(
        &self,
        native: &NativeWindowSurface,
        position: Option<cranpose_ui::Point>,
    ) -> Vec<WindowGraphPeerSnapshot> {
        self.native_window_graph_snapshots_with_source(
            native,
            position,
            NativeWindowGraphPositionSource::CachedThenCurrent,
        )
    }

    fn native_window_graph_snapshots_with_current_positions(
        &self,
        native: &NativeWindowSurface,
        position: Option<cranpose_ui::Point>,
    ) -> Vec<WindowGraphPeerSnapshot> {
        self.native_window_graph_snapshots_with_source(
            native,
            position,
            NativeWindowGraphPositionSource::CurrentThenCached,
        )
    }

    fn native_window_graph_snapshots_with_source(
        &self,
        native: &NativeWindowSurface,
        position: Option<cranpose_ui::Point>,
        source: NativeWindowGraphPositionSource,
    ) -> Vec<WindowGraphPeerSnapshot> {
        let mut snapshots: Vec<_> = self
            .native_windows
            .values()
            .filter_map(|native| self.native_window_graph_snapshot(native, None, source))
            .collect();
        snapshots.retain(|snapshot| snapshot.node.id != native.key);
        if let Some(snapshot) = self.native_window_graph_snapshot(native, position, source) {
            snapshots.push(snapshot);
        }
        snapshots
    }

    fn native_window_graph_snapshot(
        &self,
        native: &NativeWindowSurface,
        position: Option<cranpose_ui::Point>,
        source: NativeWindowGraphPositionSource,
    ) -> Option<WindowGraphPeerSnapshot> {
        let cached_position = self.native_window_positions.get(&native.key).copied();
        let current_position =
            current_native_window_position(&self.native_window_platform_probe, native);
        let options_position = native_window_options_position(&native.options);
        let position = native_window_graph_position(
            position,
            cached_position,
            current_position,
            options_position,
            source,
        )?;
        Some(WindowGraphPeerSnapshot {
            node: WindowGraphNodeSnapshot {
                id: native.key,
                position,
                size: native
                    .state
                    .map(WindowState::size_non_reactive)
                    .unwrap_or_else(|| {
                        cranpose_ui::Size::new(native.options.width, native.options.height)
                    }),
            },
            group: native.group.clone(),
        })
    }

    fn apply_window_graph_drag(
        &mut self,
        dragged: NativeWindowKey,
        target: cranpose_ui::Point,
    ) -> bool {
        let moves = self.window_graph.drag_to(dragged, target);
        self.apply_window_graph_moves_with_mode(moves, NativeWindowPositionApplyMode::FlushOnly)
    }

    fn finish_window_graph_drag(&mut self) -> bool {
        let snapshots = self.native_window_graph_snapshots();
        let moves = self.window_graph.finish_drag(&snapshots);
        self.apply_window_graph_moves(moves)
    }

    fn apply_window_graph_moves(&mut self, moves: Vec<WindowGraphMove>) -> bool {
        self.apply_window_graph_moves_with_mode(moves, NativeWindowPositionApplyMode::WaitForSettle)
    }

    fn apply_window_graph_moves_with_mode(
        &mut self,
        moves: Vec<WindowGraphMove>,
        mode: NativeWindowPositionApplyMode,
    ) -> bool {
        let mut moved = false;
        let mut native_position_requests = Vec::new();
        for window_move in moves {
            let Some(window_id) = self.native_window_ids.get(&window_move.id).copied() else {
                continue;
            };
            let Some(native) = self.native_windows.get_mut(&window_id) else {
                continue;
            };
            if let Some(request) = Self::prepare_native_window_position_request(
                &self.native_window_platform_probe,
                native,
                window_move.position,
            ) {
                native_position_requests.push(request);
                self.native_window_positions.insert(
                    window_move.id,
                    (window_move.position.x, window_move.position.y),
                );
                moved = true;
            }
        }
        Self::apply_native_window_position_requests(
            &self.native_window_platform_probe,
            native_position_requests,
            mode,
        );
        moved
    }

    fn create_native_window_shell(
        event_loop: &dyn ActiveEventLoop,
        request: NativeWindowRequest,
        headless: bool,
    ) -> Result<NativeWindowShell, LaunchError> {
        let create_started = Instant::now();
        let options = &request.options;
        let attributes = native_window_attributes(options, headless);

        let window: Arc<dyn Window> = event_loop
            .create_window(attributes)
            .map_err(LaunchError::WindowCreate)?
            .into();
        trace_native_window_timing(format_args!(
            "{} create_window {}ms",
            options.title,
            create_started.elapsed().as_millis()
        ));
        Ok(NativeWindowShell {
            request,
            window,
            create_started,
        })
    }

    fn create_native_window(
        &self,
        shell: NativeWindowShell,
    ) -> Result<NativeWindowSurface, LaunchError> {
        let NativeWindowShell {
            request,
            window,
            create_started,
        } = shell;
        let Some(context) = self.gpu_context.as_ref() else {
            return Err(LaunchError::GpuContextUnavailable);
        };

        let options = &request.options;
        let surface = context
            .instance
            .create_surface(window.clone())
            .map_err(LaunchError::SurfaceCreate)?;
        trace_native_window_timing(format_args!(
            "{} create_surface {}ms",
            options.title,
            create_started.elapsed().as_millis()
        ));
        let surface_caps = surface.get_capabilities(&context.adapter);
        let surface_format = select_surface_format(&surface_caps)?;
        let present_mode = desktop_present_mode(&surface_caps, self.frame_pacing_mode);
        let size = window.surface_size();
        let surface_config = surface_config_for_window(
            &surface_caps,
            surface_format,
            size.width.max(1),
            size.height.max(1),
            present_mode,
            options.transparent,
            self.frame_pacing_mode,
        )?;
        surface.configure(&context.device, &surface_config);
        trace_native_window_timing(format_args!(
            "{} configure {}ms",
            options.title,
            create_started.elapsed().as_millis()
        ));

        let scale_factor = window.scale_factor();
        let renderer = wgpu_renderer_for_surface(
            context.text_system.clone(),
            Arc::clone(&context.device),
            Arc::clone(&context.queue),
            surface_format,
            context.adapter_backend,
            scale_factor,
        );
        trace_native_window_timing(format_args!(
            "{} renderer {}ms",
            options.title,
            create_started.elapsed().as_millis()
        ));
        let content = request.content.clone();
        let content_env = Rc::clone(&self.platform_env);
        let viewport = (
            surface_config.width as f32 / scale_factor as f32,
            surface_config.height as f32 / scale_factor as f32,
        );
        let registry = Rc::clone(&self.native_window_registry);
        let mut app = native_window::with_native_window_registry(&registry, || {
            AppShell::new_with_size_and_density(
                renderer,
                default_root_key(),
                move || {
                    content_env.compose_root(|| (content.borrow_mut())());
                },
                (surface_config.width, surface_config.height),
                viewport,
                scale_factor as f32,
            )
        });
        let mut dev_options = self.settings.dev_options.clone();
        dev_options.frame_pacing_mode = self.frame_pacing_mode;
        dev_options.frame_pacing_controls = false;
        app.set_dev_options(dev_options);
        // Enable/disable the platform IME with text-field focus so composing
        // input methods (CJK, dead keys via IME) work in native windows.
        DesktopTextInput::install(&mut app, &window);
        trace_native_window_timing(format_args!(
            "{} app_shell {}ms",
            options.title,
            create_started.elapsed().as_millis()
        ));

        let frame_waker_window = window.clone();
        app.set_frame_waker(move || {
            frame_waker_window.request_redraw();
        });

        let mut platform = DesktopWinitPlatform::default();
        platform.set_scale_factor(scale_factor);
        window.request_redraw();
        trace_native_window_timing(format_args!(
            "{} create done {}ms",
            options.title,
            create_started.elapsed().as_millis()
        ));

        Ok(NativeWindowSurface {
            key: request.key,
            revision: request.revision,
            options: request.options.clone(),
            events: request.events.clone(),
            state: request.state,
            group: request.group.clone(),
            window,
            surface,
            surface_config,
            surface_caps,
            surface_dirty: true,
            app,
            platform,
            last_cursor_position: None,
            last_cursor_physical_position: None,
            frame_pacing_mode: self.frame_pacing_mode,
            last_frame_start_time: None,
            vsync_interval: default_vsync_interval(),
            pending_outer_positions: PendingNativeWindowPositions::default(),
            active_drag: None,
        })
    }

    fn apply_native_window_options(
        platform_probe: &NativeWindowPlatformProbe,
        native: &mut NativeWindowSurface,
        options: &NativeWindowOptions,
        headless: bool,
    ) {
        if native.options.title != options.title {
            native.window.set_title(&options.title);
        }
        if native.options.decorations != options.decorations {
            native.window.set_decorations(options.decorations);
        }
        if native.options.resizable != options.resizable {
            native.window.set_resizable(options.resizable);
        }
        if native.options.transparent != options.transparent {
            native.window.set_transparent(options.transparent);
        }
        if native.options.always_on_top != options.always_on_top {
            native
                .window
                .set_window_level(native_window_level(options.always_on_top));
        }
        if native.options.min_width != options.min_width
            || native.options.min_height != options.min_height
        {
            native
                .window
                .set_min_surface_size(match (options.min_width, options.min_height) {
                    (Some(width), Some(height)) => {
                        Some(LogicalSize::new(width.max(1.0) as f64, height.max(1.0) as f64).into())
                    }
                    _ => None,
                });
        }
        if native.options.max_width != options.max_width
            || native.options.max_height != options.max_height
        {
            native
                .window
                .set_max_surface_size(match (options.max_width, options.max_height) {
                    (Some(width), Some(height)) => {
                        Some(LogicalSize::new(width.max(1.0) as f64, height.max(1.0) as f64).into())
                    }
                    _ => None,
                });
        }
        if native.options.visible != options.visible {
            native.window.set_visible(!headless && options.visible);
        }
        if native.options.x != options.x || native.options.y != options.y {
            if let (Some(x), Some(y)) = (options.x, options.y) {
                native.pending_outer_positions.push((x, y));
                let logical = LogicalPosition::new(x as f64, y as f64);
                let physical = logical.to_physical::<i32>(native.window.scale_factor());
                if !native_window_set_outer_position_physical(
                    platform_probe,
                    &native.window,
                    physical,
                ) {
                    native.window.set_outer_position(Position::Logical(logical));
                }
            }
        }
        if native.options.width != options.width || native.options.height != options.height {
            if let Some(size) = native.window.request_surface_size(
                LogicalSize::new(
                    options.width.max(1.0) as f64,
                    options.height.max(1.0) as f64,
                )
                .into(),
            ) {
                Self::resize_native_surface(native, size.width, size.height);
            }
        }
        native.options = options.clone();
    }

    fn resize_native_surface(native: &mut NativeWindowSurface, width: u32, height: u32) {
        let viewport = surface_logical_viewport_size(width, height, native.window.scale_factor());
        configure_app_surface_size(
            &mut native.app,
            &native.surface,
            &mut native.surface_config,
            width,
            height,
            viewport,
        );
        native.surface_dirty = surface_reconfigure_requires_redraw(width, height);
    }

    fn sync_native_window_position_from_os(
        platform_probe: &NativeWindowPlatformProbe,
        native: &mut NativeWindowSurface,
        native_window_positions: &mut HashMap<NativeWindowKey, (f32, f32)>,
    ) -> bool {
        let Some(position) = current_native_window_position(platform_probe, native) else {
            return false;
        };
        if native_window_positions
            .get(&native.key)
            .is_some_and(|known| native_window_positions_close(*known, position))
            && native
                .state
                .and_then(|state| state.position_non_reactive())
                .is_some_and(|known| native_window_positions_close((known.x, known.y), position))
        {
            return false;
        }

        let previous_state_position = native.state.and_then(|state| state.position_non_reactive());
        native_window_positions.insert(native.key, position);
        update_native_options_position(&mut native.options, position.0, position.1);
        native.pending_outer_positions.clear();
        notify_native_window_moved(&native.events, position.0, position.1);
        sync_native_window_state_position(
            native.state,
            previous_state_position,
            position.0,
            position.1,
        );
        true
    }

    fn prepare_native_window_position_request(
        platform_probe: &NativeWindowPlatformProbe,
        native: &mut NativeWindowSurface,
        position: cranpose_ui::Point,
    ) -> Option<NativeWindowPositionRequest> {
        let logical_position = (position.x, position.y);
        if current_native_window_position(platform_probe, native).is_some_and(|current| {
            (current.0 - logical_position.0).abs() <= f32::EPSILON
                && (current.1 - logical_position.1).abs() <= f32::EPSILON
        }) {
            update_native_options_position(&mut native.options, position.x, position.y);
            return None;
        }

        native.pending_outer_positions.push(logical_position);
        let logical = LogicalPosition::new(position.x as f64, position.y as f64);
        let physical = logical.to_physical::<i32>(native.window.scale_factor());
        update_native_options_position(&mut native.options, position.x, position.y);
        let previous_state_position = native.state.and_then(|state| state.position_non_reactive());
        notify_native_window_moved(&native.events, position.x, position.y);
        sync_native_window_state_position(
            native.state,
            previous_state_position,
            position.x,
            position.y,
        );
        Some(NativeWindowPositionRequest {
            window: Arc::clone(&native.window),
            logical,
            physical,
        })
    }

    fn apply_native_window_position_requests(
        platform_probe: &NativeWindowPlatformProbe,
        requests: Vec<NativeWindowPositionRequest>,
        mode: NativeWindowPositionApplyMode,
    ) {
        if requests.is_empty() {
            return;
        }
        if native_window_set_outer_positions_physical(platform_probe, &requests) {
            if mode == NativeWindowPositionApplyMode::WaitForSettle {
                wait_native_window_positions_physical(platform_probe, &requests);
            }
            return;
        }
        for request in &requests {
            request
                .window
                .set_outer_position(Position::Logical(request.logical));
        }
        if mode == NativeWindowPositionApplyMode::WaitForSettle {
            wait_native_window_positions_physical(platform_probe, &requests);
        }
    }

    fn handle_native_primary_pressed(&mut self, native: &mut NativeWindowSurface) -> bool {
        let (handled, drag_requested) =
            Self::dispatch_native_primary_pressed(&self.native_window_platform_probe, native);
        if handled {
            apply_pointer_button_frame_request(
                &native.window,
                &mut native.last_frame_start_time,
                pointer_button_frame_request(handled),
            );
            if drag_requested {
                self.begin_native_window_drag(native);
            }
        }
        handled
    }

    fn dispatch_native_primary_pressed(
        platform_probe: &NativeWindowPlatformProbe,
        native: &mut NativeWindowSurface,
    ) -> (bool, bool) {
        let drag_requested = Rc::new(Cell::new(false));
        let drag_requested_for_handler = Rc::clone(&drag_requested);
        let drag_handler: Rc<dyn Fn() -> bool> = Rc::new(move || {
            drag_requested_for_handler.set(true);
            true
        });
        let resize_window = native.window.clone();
        let resize_handler: Rc<dyn Fn(WindowResizeDirection)> = Rc::new(move |direction| {
            if let Err(error) = resize_window.drag_resize_window(native_resize_direction(direction))
            {
                log::debug!("native window resize request failed: {error}");
            }
        });
        let handled =
            native_window::with_native_window_drag_handler(drag_handler, resize_handler, || {
                native_window::with_native_window_surface_origin(
                    native_window_surface_origin(platform_probe, &native.window),
                    || native.app.pointer_pressed(),
                )
            });
        (handled, drag_requested.get())
    }

    fn begin_native_window_drag(&mut self, native: &mut NativeWindowSurface) {
        self.begin_native_window_drag_with_anchor(native, None);
    }

    fn begin_native_window_drag_with_anchor(
        &mut self,
        native: &mut NativeWindowSurface,
        start_pointer_screen: Option<PhysicalPosition<f64>>,
    ) {
        trace_native_window(format_args!("drag requested key={:?}", native.key));
        let platform_probe = &self.native_window_platform_probe;
        Self::sync_native_window_position_from_os(
            platform_probe,
            native,
            &mut self.native_window_positions,
        );
        for other_native in self.native_windows.values_mut() {
            Self::sync_native_window_position_from_os(
                platform_probe,
                other_native,
                &mut self.native_window_positions,
            );
        }
        let graph_snapshots = self.native_window_graph_snapshots_with(native, None);
        self.window_graph.start_drag(&graph_snapshots, native.key);
        if !Self::start_native_window_drag(platform_probe, native, start_pointer_screen) {
            trace_native_window(format_args!(
                "drag cancel key={:?} reason=start-failed",
                native.key
            ));
            self.window_graph.cancel_drag();
        }
    }

    fn start_native_window_drag(
        platform_probe: &NativeWindowPlatformProbe,
        native: &mut NativeWindowSurface,
        start_pointer_screen: Option<PhysicalPosition<f64>>,
    ) -> bool {
        let now = Instant::now();
        if let Some(session) = Self::native_window_polling_drag_session(
            platform_probe,
            native,
            now,
            start_pointer_screen,
        ) {
            let pointer = session.start_pointer_screen;
            let window_outer = session.start_window_outer;
            native.active_drag = Some(NativeWindowDragSession::Polling(session));
            trace_native_window(format_args!(
                "drag start polling key={:?} pointer=({:.1},{:.1}) outer=({},{})",
                native.key, pointer.x, pointer.y, window_outer.x, window_outer.y
            ));
            return true;
        }

        match native.window.drag_window() {
            Ok(()) => {
                native.active_drag = Some(NativeWindowDragSession::platform(now));
                trace_native_window(format_args!("drag start platform key={:?}", native.key));
                return true;
            }
            Err(error) => {
                log::debug!("native window drag request failed: {error}");
            }
        }

        false
    }

    fn native_window_polling_drag_session(
        platform_probe: &NativeWindowPlatformProbe,
        native: &NativeWindowSurface,
        now: Instant,
        start_pointer_screen: Option<PhysicalPosition<f64>>,
    ) -> Option<NativeWindowPollingDragSession> {
        let pointer = start_pointer_screen
            .or_else(|| {
                native_window_global_pointer_state(platform_probe).map(|state| state.position)
            })
            .or_else(|| {
                native.last_cursor_physical_position.and_then(|position| {
                    native_window_screen_pointer_physical(platform_probe, &native.window, position)
                })
            })?;
        let window_outer = current_native_window_physical_position(platform_probe, &native.window)?;
        Some(NativeWindowPollingDragSession::new(
            pointer,
            window_outer,
            now,
        ))
    }

    fn poll_native_window_global_primary_press(&mut self) -> bool {
        let platform_probe = &self.native_window_platform_probe;
        let Some(pointer) = native_window_global_pointer_state(platform_probe) else {
            self.native_global_primary_down = false;
            return false;
        };
        if !pointer.primary_down {
            if self.native_global_primary_down {
                self.native_global_primary_down = false;
                return self.release_native_global_primary_press();
            }
            self.native_global_primary_down = false;
            return false;
        }
        if self.native_global_primary_down {
            return false;
        }

        let Some(window_id) = self.native_windows.iter().find_map(|(window_id, native)| {
            (native.options.visible
                && native.active_drag.is_none()
                && native_window_surface_contains_pointer(platform_probe, native, pointer.position))
            .then_some(*window_id)
        }) else {
            self.native_global_primary_down = false;
            return false;
        };

        let Some(mut native) = self.native_windows.remove(&window_id) else {
            self.native_global_primary_down = false;
            return false;
        };
        if let Some(local) =
            native_window_local_pointer_physical(platform_probe, &native.window, pointer.position)
        {
            let logical = native.platform.pointer_position(local);
            native.last_cursor_position = Some((logical.x, logical.y));
            native.last_cursor_physical_position = Some(local);
            native_window::with_native_window_surface_origin(
                native_window_surface_origin(platform_probe, &native.window),
                || native.app.set_cursor(logical.x, logical.y),
            );
        }

        let (press_handled, drag_requested) =
            Self::dispatch_native_primary_pressed(platform_probe, &mut native);
        if press_handled {
            self.native_global_primary_down = true;
            trace_native_window(format_args!(
                "global primary recovered key={:?} drag_requested={}",
                native.key, drag_requested
            ));
            apply_pointer_button_frame_request(
                &native.window,
                &mut native.last_frame_start_time,
                pointer_button_frame_request(press_handled),
            );
            if drag_requested {
                self.begin_native_window_drag_with_anchor(&mut native, Some(pointer.position));
            } else {
                native.app.sync_selection_to_primary();
            }
        } else {
            self.native_global_primary_down = false;
        }

        self.native_windows.insert(window_id, native);
        press_handled
    }

    fn release_native_global_primary_press(&mut self) -> bool {
        let platform_probe = &self.native_window_platform_probe;
        let mut handled_any = false;
        for native in self.native_windows.values_mut() {
            if native.active_drag.is_some() {
                continue;
            }
            let handled = native_window::with_native_window_surface_origin(
                native_window_surface_origin(platform_probe, &native.window),
                || native.app.pointer_released(),
            );
            native.app.sync_selection_to_primary();
            if handled {
                apply_pointer_button_frame_request(
                    &native.window,
                    &mut native.last_frame_start_time,
                    pointer_button_frame_request(handled),
                );
                native.window.request_redraw();
                handled_any = true;
            }
        }
        handled_any
    }

    fn poll_active_native_window_drags(&mut self, now: Instant) -> bool {
        let has_due_drag = self.native_windows.values().any(|native| {
            native
                .active_drag
                .is_some_and(|active_drag| active_drag.next_poll_at() <= now)
        });
        if !has_due_drag {
            return false;
        }

        let platform_probe = &self.native_window_platform_probe;
        let pointer = native_window_global_pointer_state(platform_probe);
        let mut updates = Vec::new();
        let mut finish_drag = false;
        let mut needs_registry_sync = false;
        for native in self.native_windows.values_mut() {
            let Some(active_drag) = native.active_drag.as_mut() else {
                continue;
            };
            if active_drag.next_poll_at() > now {
                continue;
            }
            active_drag.set_next_poll_at(now + NATIVE_WINDOW_DRAG_POLL_INTERVAL);

            let Some(pointer) = pointer else {
                trace_native_window(format_args!(
                    "drag poll skipped key={:?} reason=no-global-pointer",
                    native.key
                ));
                continue;
            };
            if !pointer.primary_down && active_drag.finishes_on_global_pointer_release() {
                native.active_drag = None;
                finish_drag = true;
                needs_registry_sync = true;
                trace_native_window(format_args!(
                    "drag finish key={:?} reason=global-release",
                    native.key
                ));
                if native.app.pointer_released() {
                    native.window.request_redraw();
                    needs_registry_sync = true;
                }
                native.app.sync_selection_to_primary();
                continue;
            }
            if let Some(update) =
                Self::update_native_window_polling_drag_target(native, pointer.position)
            {
                updates.push(update);
            }
        }

        for (key, position) in updates {
            self.apply_window_graph_drag(key, position);
        }
        if finish_drag {
            self.finish_window_graph_drag();
        }
        needs_registry_sync
    }

    fn poll_external_native_window_moves(&mut self, now: Instant) -> bool {
        if self.native_windows.is_empty() || self.next_native_window_position_poll_at > now {
            return false;
        }
        self.next_native_window_position_poll_at = now + NATIVE_WINDOW_POSITION_POLL_INTERVAL;

        let mut external_moves = Vec::new();
        let native_window_positions = &mut self.native_window_positions;
        let platform_probe = &self.native_window_platform_probe;
        for (window_id, native) in &mut self.native_windows {
            if !native.options.visible || native.active_drag.is_some() {
                continue;
            }
            let Some(position) = current_native_window_position(platform_probe, native) else {
                continue;
            };
            let known_position = native_window_positions.get(&native.key).copied();
            match native
                .pending_outer_positions
                .acknowledge_or_matches_known(position, known_position)
            {
                NativeWindowPositionObservation::Current => {
                    native_window_positions.insert(native.key, position);
                    update_native_options_position(&mut native.options, position.0, position.1);
                    continue;
                }
                NativeWindowPositionObservation::Superseded => {
                    continue;
                }
                NativeWindowPositionObservation::External => {}
            }
            if native.pending_outer_positions.has_pending()
                || native_window_positions
                    .get(&native.key)
                    .is_some_and(|known| native_window_positions_close(*known, position))
            {
                continue;
            }

            let previous_state_position =
                native.state.and_then(|state| state.position_non_reactive());
            let previous_graph_position = native_window_positions
                .get(&native.key)
                .map(|(x, y)| cranpose_ui::Point::new(*x, *y))
                .or(previous_state_position)
                .or_else(|| {
                    native_window_options_position(&native.options)
                        .map(|(x, y)| cranpose_ui::Point::new(x, y))
                });
            external_moves.push((
                *window_id,
                native.key,
                position,
                previous_state_position,
                previous_graph_position,
            ));
        }

        let mut moved = false;
        for (window_id, key, position, previous_state_position, previous_graph_position) in
            external_moves
        {
            moved |= self.reconcile_external_native_window_move(
                window_id,
                key,
                position,
                previous_state_position,
                previous_graph_position,
            );
        }
        moved
    }

    fn reconcile_external_native_window_move(
        &mut self,
        window_id: WinitWindowId,
        key: NativeWindowKey,
        position: (f32, f32),
        previous_state_position: Option<cranpose_ui::Point>,
        previous_graph_position: Option<cranpose_ui::Point>,
    ) -> bool {
        let Some(native) = self.native_windows.get(&window_id) else {
            return false;
        };
        let previous_graph_snapshots = self
            .native_window_graph_snapshots_with_current_positions(native, previous_graph_position);

        let Some(native) = self.native_windows.get_mut(&window_id) else {
            return false;
        };
        if native.active_drag.is_some() || native.pending_outer_positions.has_pending() {
            return false;
        }

        trace_native_window(format_args!(
            "poll external move key={:?} pos=({:.1},{:.1})",
            key, position.0, position.1
        ));
        self.native_window_positions.insert(key, position);
        update_native_options_position(&mut native.options, position.0, position.1);
        let position = cranpose_ui::Point::new(position.0, position.1);
        notify_native_window_moved(&native.events, position.x, position.y);
        sync_native_window_state_position(
            native.state,
            previous_state_position,
            position.x,
            position.y,
        );

        let graph_moves = self
            .window_graph
            .external_move(&previous_graph_snapshots, key, position);
        self.apply_window_graph_moves(graph_moves);
        true
    }

    fn update_native_window_polling_drag_target(
        native: &mut NativeWindowSurface,
        pointer: PhysicalPosition<f64>,
    ) -> Option<(NativeWindowKey, cranpose_ui::Point)> {
        let active_drag = native.active_drag.as_mut()?.polling_mut()?;
        let target = active_drag.target_for_pointer(pointer);
        if target == active_drag.last_target_outer {
            return None;
        }
        active_drag.last_target_outer = target;
        let logical = target.to_logical::<f64>(native.window.scale_factor());
        trace_native_window(format_args!(
            "drag target key={:?} logical=({:.1},{:.1}) physical=({},{})",
            native.key, logical.x, logical.y, target.x, target.y
        ));
        Some((
            native.key,
            cranpose_ui::Point::new(logical.x as f32, logical.y as f32),
        ))
    }

    fn native_window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: WinitWindowId,
        event: WindowEvent,
    ) {
        let Some(mut native) = self.native_windows.remove(&window_id) else {
            return;
        };

        let mut keep_window = true;
        let mut sync_after_event = false;
        let mut graph_drag_after_insert = None::<(NativeWindowKey, cranpose_ui::Point)>;
        let mut graph_moves_after_insert = Vec::<WindowGraphMove>::new();
        let mut graph_moves_apply_mode_after_insert = NativeWindowPositionApplyMode::WaitForSettle;
        let mut finish_graph_drag_after_insert = false;
        match event {
            WindowEvent::CloseRequested => {
                trace_native_window(format_args!("event close-request key={:?}", native.key));
                notify_native_window_close_requested(&native.events);
                self.remember_native_window_position(&native);
                self.native_window_ids.remove(&native.key);
                self.closed_native_windows.insert(native.key);
                keep_window = false;
            }
            WindowEvent::SurfaceResized(new_size) => {
                let previous_state_size = native.state.map(|state| state.size_non_reactive());
                update_native_options_size(
                    &mut native.options,
                    &native.window,
                    new_size.width,
                    new_size.height,
                );
                notify_native_window_resized(
                    &native.events,
                    &native.window,
                    new_size.width,
                    new_size.height,
                );
                sync_native_window_state_size(
                    native.state,
                    previous_state_size,
                    &native.window,
                    new_size.width,
                    new_size.height,
                );
                Self::resize_native_surface(&mut native, new_size.width, new_size.height);
                sync_after_event = true;
            }
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                mut surface_size_writer,
            } => {
                let previous_state_size = native.state.map(|state| state.size_non_reactive());
                update_app_scale_factor(&mut native.app, &mut native.platform, scale_factor);

                let new_size = native.window.surface_size();
                let _ = surface_size_writer.request_surface_size(new_size);
                update_native_options_size(
                    &mut native.options,
                    &native.window,
                    new_size.width,
                    new_size.height,
                );
                notify_native_window_resized(
                    &native.events,
                    &native.window,
                    new_size.width,
                    new_size.height,
                );
                sync_native_window_state_size(
                    native.state,
                    previous_state_size,
                    &native.window,
                    new_size.width,
                    new_size.height,
                );
                Self::resize_native_surface(&mut native, new_size.width, new_size.height);
                sync_after_event = true;
            }
            WindowEvent::Moved(position) => {
                native.vsync_interval = monitor_refresh_interval(&native.window);
                let known_position = self.native_window_positions.get(&native.key).copied();
                let previous_state_position =
                    native.state.and_then(|state| state.position_non_reactive());
                let previous_graph_position = known_position
                    .map(|(x, y)| cranpose_ui::Point::new(x, y))
                    .or(previous_state_position)
                    .or_else(|| match (native.options.x, native.options.y) {
                        (Some(x), Some(y)) => Some(cranpose_ui::Point::new(x, y)),
                        _ => None,
                    });
                let previous_graph_snapshots = self
                    .native_window_graph_snapshots_with_current_positions(
                        &native,
                        previous_graph_position,
                    );
                let platform_probe = &self.native_window_platform_probe;
                let position = current_native_window_position(platform_probe, &native)
                    .unwrap_or_else(|| {
                        let logical = position.to_logical::<f64>(native.window.scale_factor());
                        (logical.x as f32, logical.y as f32)
                    });
                let position_observation = native
                    .pending_outer_positions
                    .acknowledge_or_matches_known(position, known_position);
                trace_native_window(format_args!(
                    "event moved key={:?} pos=({:.1},{:.1}) observation={:?} active_drag={}",
                    native.key,
                    position.0,
                    position.1,
                    position_observation,
                    native.active_drag.is_some()
                ));
                match position_observation {
                    NativeWindowPositionObservation::Current => {
                        self.native_window_positions.insert(native.key, position);
                        update_native_options_position(&mut native.options, position.0, position.1);
                    }
                    NativeWindowPositionObservation::Superseded => {}
                    NativeWindowPositionObservation::External => {
                        if native.active_drag.is_some_and(|active_drag| {
                            !active_drag.uses_moved_events_as_drag_target()
                        }) {
                            trace_native_window(format_args!(
                                "event moved ignored during polling drag key={:?}",
                                native.key
                            ));
                        } else {
                            self.native_window_positions.insert(native.key, position);
                            update_native_options_position(
                                &mut native.options,
                                position.0,
                                position.1,
                            );
                            let position = cranpose_ui::Point::new(position.0, position.1);
                            native.pending_outer_positions.clear();
                            notify_native_window_moved(&native.events, position.x, position.y);
                            sync_native_window_state_position(
                                native.state,
                                previous_state_position,
                                position.x,
                                position.y,
                            );
                            if native.active_drag.is_some_and(|active_drag| {
                                active_drag.uses_moved_events_as_drag_target()
                            }) {
                                graph_moves_after_insert =
                                    self.window_graph.drag_to(native.key, position);
                                graph_moves_apply_mode_after_insert =
                                    NativeWindowPositionApplyMode::FlushOnly;
                            } else if native_window_global_pointer_state(platform_probe)
                                .is_some_and(|pointer| {
                                    pointer.primary_down
                                        && native_window_surface_contains_pointer(
                                            platform_probe,
                                            &native,
                                            pointer.position,
                                        )
                                        && previous_graph_position.is_some_and(|previous| {
                                            native_window_surface_at_logical_position_contains_pointer(
                                                &native.window,
                                                previous,
                                                pointer.position,
                                            )
                                        })
                                })
                            {
                                self.window_graph
                                    .start_drag(&previous_graph_snapshots, native.key);
                                native.active_drag =
                                    Some(NativeWindowDragSession::platform(Instant::now()));
                                trace_native_window(format_args!(
                                    "drag start inferred-platform key={:?}",
                                    native.key
                                ));
                                graph_moves_after_insert =
                                    self.window_graph.drag_to(native.key, position);
                                graph_moves_apply_mode_after_insert =
                                    NativeWindowPositionApplyMode::FlushOnly;
                            } else {
                                graph_moves_after_insert = self.window_graph.external_move(
                                    &previous_graph_snapshots,
                                    native.key,
                                    position,
                                );
                            }
                            sync_after_event = true;
                        }
                    }
                }
            }
            WindowEvent::PointerMoved {
                position, source, ..
            } => {
                let logical = native.platform.pointer_position(position);
                native.last_cursor_position = Some((logical.x, logical.y));
                native.last_cursor_physical_position = Some(position);
                native
                    .app
                    .set_pointer_source(pointer_source_from_winit(&source));
                let platform_probe = &self.native_window_platform_probe;
                let event_pointer =
                    native_window_screen_pointer_physical(platform_probe, &native.window, position);
                let global_pointer = native_window_global_pointer_state(platform_probe);
                let pointer_position = global_pointer.map(|state| state.position).or(event_pointer);
                let handled = native_window::with_native_window_surface_origin(
                    native_window_surface_origin(platform_probe, &native.window),
                    || native.app.set_cursor(logical.x, logical.y),
                );
                if let Some(pointer) = pointer_position {
                    if let Some((key, position)) =
                        Self::update_native_window_polling_drag_target(&mut native, pointer)
                    {
                        graph_drag_after_insert = Some((key, position));
                        sync_after_event = true;
                    }
                }
                if native.active_drag.is_none()
                    && !self.native_global_primary_down
                    && global_pointer.is_some_and(|pointer| {
                        pointer.primary_down
                            && native_window_surface_contains_pointer(
                                platform_probe,
                                &native,
                                pointer.position,
                            )
                    })
                {
                    let (press_handled, drag_requested) =
                        Self::dispatch_native_primary_pressed(platform_probe, &mut native);
                    if press_handled {
                        self.native_global_primary_down = true;
                        trace_native_window(format_args!(
                            "event pointer-move recovered primary-press key={:?} drag_requested={}",
                            native.key, drag_requested
                        ));
                        apply_pointer_button_frame_request(
                            &native.window,
                            &mut native.last_frame_start_time,
                            pointer_button_frame_request(press_handled),
                        );
                        if drag_requested {
                            self.begin_native_window_drag_with_anchor(
                                &mut native,
                                recovered_native_window_drag_start_pointer(
                                    event_pointer,
                                    global_pointer,
                                ),
                            );
                        } else {
                            native.app.sync_selection_to_primary();
                        }
                        sync_after_event = true;
                    }
                }
                if handled {
                    native.window.request_redraw();
                    sync_after_event = true;
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.current_modifiers = modifiers.state();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if native.last_cursor_position.is_none() {
                    Self::refresh_native_cursor_from_platform_pointer(
                        &self.native_window_platform_probe,
                        &mut native,
                    );
                }
                dispatch_mouse_wheel(
                    &mut native.app,
                    &native.platform,
                    self.current_modifiers,
                    native.last_cursor_position,
                    delta,
                );
            }
            WindowEvent::PointerButton {
                state,
                button: button @ ButtonSource::Mouse(MouseButton::Left),
                ..
            } => {
                native
                    .app
                    .set_pointer_source(pointer_source_from_button(&button));
                if native.last_cursor_position.is_none() {
                    Self::refresh_native_cursor_from_platform_pointer(
                        &self.native_window_platform_probe,
                        &mut native,
                    );
                }
                trace_native_window(format_args!(
                    "event pointer-button key={:?} state={:?} cursor={:?}",
                    native.key, state, native.last_cursor_position
                ));
                if let Some((x, y)) = native.last_cursor_position {
                    let platform_probe = &self.native_window_platform_probe;
                    native_window::with_native_window_surface_origin(
                        native_window_surface_origin(platform_probe, &native.window),
                        || native.app.set_cursor(x, y),
                    );
                }
                match state {
                    ElementState::Pressed => {
                        if self.native_global_primary_down || native.active_drag.is_some() {
                            trace_native_window(format_args!(
                                "event pointer-button duplicate-primary-down key={:?}",
                                native.key
                            ));
                            self.native_global_primary_down = true;
                        } else {
                            self.native_global_primary_down = true;
                            if self.handle_native_primary_pressed(&mut native) {
                                sync_after_event = true;
                            }
                        }
                    }
                    ElementState::Released => {
                        self.native_global_primary_down = false;
                        let fallback_pointer =
                            native.last_cursor_physical_position.and_then(|position| {
                                native_window_screen_pointer_physical(
                                    &self.native_window_platform_probe,
                                    &native.window,
                                    position,
                                )
                            });
                        if let Some(pointer) =
                            native_window_global_pointer_state(&self.native_window_platform_probe)
                                .map(|state| state.position)
                                .or(fallback_pointer)
                        {
                            if let Some((key, position)) =
                                Self::update_native_window_polling_drag_target(&mut native, pointer)
                            {
                                graph_drag_after_insert = Some((key, position));
                            }
                        }
                        finish_graph_drag_after_insert = native.active_drag.take().is_some();
                        if finish_graph_drag_after_insert {
                            trace_native_window(format_args!(
                                "drag finish key={:?} reason=local-release",
                                native.key
                            ));
                        }
                        let handled = native_window::with_native_window_surface_origin(
                            native_window_surface_origin(
                                &self.native_window_platform_probe,
                                &native.window,
                            ),
                            || native.app.pointer_released(),
                        );
                        native.app.sync_selection_to_primary();
                        if handled {
                            apply_pointer_button_frame_request(
                                &native.window,
                                &mut native.last_frame_start_time,
                                pointer_button_frame_request(handled),
                            );
                            sync_after_event = true;
                        }
                    }
                }
            }
            WindowEvent::PointerButton {
                state: ElementState::Pressed,
                button: ButtonSource::Mouse(MouseButton::Middle),
                ..
            } => {
                Self::refresh_native_cursor_from_platform_pointer(
                    &self.native_window_platform_probe,
                    &mut native,
                );
                dispatch_middle_click_paste(&mut native.app, native.last_cursor_position);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                dispatch_keyboard_input(&mut native.app, self.current_modifiers, event);
            }
            WindowEvent::Focused(false) if native.active_drag.is_none() => {
                cancel_app_input(&mut native.app);
            }
            WindowEvent::Focused(false) => {}
            WindowEvent::Ime(ime_event) => {
                dispatch_ime_event(&mut native.app, ime_event);
            }
            WindowEvent::PointerLeft { .. } if native.active_drag.is_none() => {
                native.app.cancel_gesture();
            }
            WindowEvent::PointerLeft { .. } => {}
            WindowEvent::RedrawRequested => {
                if let Some(deadline) = native.last_frame_start_time.and_then(|started_at| {
                    native
                        .frame_interval()
                        .map(|interval| started_at + interval)
                }) {
                    if deadline > Instant::now() {
                        event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
                        self.native_windows.insert(window_id, native);
                        return;
                    }
                }
                Self::redraw_native_window(&mut native, &self.native_window_registry);
            }
            _ => {}
        }

        if keep_window {
            self.native_windows.insert(window_id, native);
            if !graph_moves_after_insert.is_empty()
                && self.apply_window_graph_moves_with_mode(
                    graph_moves_after_insert,
                    graph_moves_apply_mode_after_insert,
                )
            {
                sync_after_event = true;
            }
            if let Some((key, position)) = graph_drag_after_insert {
                if self.apply_window_graph_drag(key, position) {
                    sync_after_event = true;
                }
            }
            if finish_graph_drag_after_insert && self.finish_window_graph_drag() {
                sync_after_event = true;
            }
            if sync_after_event {
                self.refresh_and_sync_native_windows(event_loop);
            }
        }
    }

    fn redraw_native_window(
        native: &mut NativeWindowSurface,
        registry: &Rc<native_window::NativeWindowRegistry>,
    ) {
        let frame_started_at = Instant::now();
        let scale_factor = native.window.scale_factor();
        native.app.set_density(scale_factor as f32);
        let update_result = update_app_with_native_window_registry(&mut native.app, registry);
        let after_update = Instant::now();
        if !surface_present_required(
            native.surface_dirty,
            update_result.visual_changed,
            native.app.needs_redraw(),
        ) {
            return;
        }
        native.surface_dirty = true;

        let output = match current_surface_texture(&native.surface, "native window") {
            SurfaceFrame::Ready(output) => output,
            SurfaceFrame::Reconfigure => {
                let size = native.window.surface_size();
                Self::resize_native_surface(native, size.width, size.height);
                if surface_reconfigure_requires_redraw(size.width, size.height) {
                    native.window.request_redraw();
                }
                return;
            }
            SurfaceFrame::Skip => {
                return;
            }
        };
        let after_acquire = Instant::now();

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        if let Err(error) = native.app.renderer().render(
            &view,
            native.surface_config.width,
            native.surface_config.height,
        ) {
            log::error!("native window render failed: {error:?}");
            return;
        }
        let after_render = Instant::now();

        output.present();
        let after_present = Instant::now();
        native.surface_dirty = false;
        native
            .app
            .record_presented_frame(frame_started_at, after_render);
        log_desktop_frame_telemetry(
            frame_started_at,
            after_update,
            after_acquire,
            after_render,
            after_present,
            "native",
        );
        native.last_frame_start_time = Some(frame_started_at);
        if should_chain_no_vsync_redraw(
            native.frame_interval(),
            native.app.frame_schedule().needs_frame,
        ) {
            native.window.request_redraw();
        }
    }

    #[cfg(feature = "robot")]
    fn set_robot_controller(&mut self, controller: RobotController) {
        self.robot_controller = Some(controller);
    }
}

fn apply_frame_pacing_mode(
    app: &mut AppShell<WgpuRenderer>,
    surface: &wgpu::Surface<'static>,
    surface_config: &mut wgpu::SurfaceConfiguration,
    surface_caps: Option<&wgpu::SurfaceCapabilities>,
    mode: FramePacingMode,
) {
    app.set_frame_pacing_mode(mode);
    if let Some(caps) = surface_caps {
        let present_mode = crate::present_mode::select_present_mode_for_frame_pacing(caps, mode);
        let frame_latency = desired_frame_latency(mode);
        if surface_config.present_mode != present_mode
            || surface_config.desired_maximum_frame_latency != frame_latency
        {
            let Some(device) = app.renderer().try_device() else {
                log::error!("desktop surface reconfigure skipped: GPU renderer is not initialized");
                return;
            };
            surface_config.present_mode = present_mode;
            surface_config.desired_maximum_frame_latency = frame_latency;
            surface.configure(device, surface_config);
        }
    }
}

fn desired_frame_latency(mode: FramePacingMode) -> u32 {
    match mode {
        FramePacingMode::Vsync | FramePacingMode::Hard60 | FramePacingMode::Hard120 => 1,
        FramePacingMode::NoVsync => 2,
    }
}

fn frame_interval_for_mode(mode: FramePacingMode, vsync_interval: Duration) -> Option<Duration> {
    match mode {
        FramePacingMode::Vsync => Some(vsync_interval),
        FramePacingMode::Hard60 => Some(Duration::from_nanos(16_666_667)),
        FramePacingMode::Hard120 => Some(Duration::from_nanos(8_333_333)),
        FramePacingMode::NoVsync => None,
    }
}

fn should_chain_no_vsync_redraw(frame_interval: Option<Duration>, needs_frame: bool) -> bool {
    frame_interval.is_none() && needs_frame
}

fn logical_monitor_rects(event_loop: &dyn ActiveEventLoop) -> Vec<DesktopRect> {
    event_loop
        .available_monitors()
        .filter_map(|monitor| logical_monitor_rect(&monitor))
        .collect()
}

fn logical_monitor_rect(monitor: &winit::monitor::MonitorHandle) -> Option<DesktopRect> {
    let position = monitor.position()?;
    let scale_factor = monitor.scale_factor() as f32;
    if scale_factor <= 0.0 {
        return None;
    }
    let size = monitor.current_video_mode()?.size();
    Some(DesktopRect {
        x: position.x as f32 / scale_factor,
        y: position.y as f32 / scale_factor,
        width: size.width as f32 / scale_factor,
        height: size.height as f32 / scale_factor,
    })
}

fn native_window_request_bounds(
    requests: &[NativeWindowRequest],
    indices: &[usize],
) -> Option<DesktopRect> {
    let mut bounds = None::<DesktopRect>;
    for index in indices {
        let options = &requests[*index].options;
        let (Some(x), Some(y)) = (options.x, options.y) else {
            continue;
        };
        let rect = DesktopRect {
            x,
            y,
            width: options.width.max(1.0),
            height: options.height.max(1.0),
        };
        bounds = Some(match bounds {
            Some(current) => union_desktop_rect(current, rect),
            None => rect,
        });
    }
    bounds
}

fn union_desktop_rect(a: DesktopRect, b: DesktopRect) -> DesktopRect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let right = a.right().max(b.right());
    let bottom = a.bottom().max(b.bottom());
    DesktopRect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    }
}

fn nearest_monitor_to_rect(monitors: &[DesktopRect], rect: DesktopRect) -> Option<DesktopRect> {
    let center = rect.center();
    monitors
        .iter()
        .min_by(|a, b| {
            a.distance_to_point(center)
                .total_cmp(&b.distance_to_point(center))
        })
        .copied()
}

fn clamp_rect_to_monitor_delta(
    rect: DesktopRect,
    monitor: DesktopRect,
    margin: f32,
) -> cranpose_ui::Point {
    let target_x = clamped_axis_origin(rect.x, rect.width, monitor.x, monitor.width, margin);
    let target_y = clamped_axis_origin(rect.y, rect.height, monitor.y, monitor.height, margin);
    cranpose_ui::Point::new(target_x - rect.x, target_y - rect.y)
}

fn clamped_axis_origin(
    origin: f32,
    length: f32,
    monitor_origin: f32,
    monitor_length: f32,
    margin: f32,
) -> f32 {
    let min = monitor_origin + margin;
    let max = monitor_origin + monitor_length - margin - length;
    if max >= min {
        origin.clamp(min, max)
    } else {
        monitor_origin + (monitor_length - length) / 2.0
    }
}

fn native_window_attributes(options: &NativeWindowOptions, headless: bool) -> WindowAttributes {
    let mut attributes = WindowAttributes::default()
        .with_title(options.title.clone())
        .with_surface_size(LogicalSize::new(
            options.width.max(1.0) as f64,
            options.height.max(1.0) as f64,
        ))
        .with_decorations(options.decorations)
        .with_transparent(options.transparent)
        .with_resizable(options.resizable)
        .with_visible(!headless && options.visible)
        .with_window_level(native_window_level(options.always_on_top));
    if let (Some(width), Some(height)) = (options.min_width, options.min_height) {
        attributes = attributes.with_min_surface_size(LogicalSize::new(
            width.max(1.0) as f64,
            height.max(1.0) as f64,
        ));
    }
    if let (Some(width), Some(height)) = (options.max_width, options.max_height) {
        attributes = attributes.with_max_surface_size(LogicalSize::new(
            width.max(1.0) as f64,
            height.max(1.0) as f64,
        ));
    }
    if let (Some(x), Some(y)) = (options.x, options.y) {
        attributes =
            attributes.with_position(Position::Logical(LogicalPosition::new(x as f64, y as f64)));
    }
    attributes
}

fn desktop_present_mode(
    surface_caps: &wgpu::SurfaceCapabilities,
    frame_pacing_mode: FramePacingMode,
) -> wgpu::PresentMode {
    if std::env::var_os("CRANPOSE_PRESENT_MODE").is_some() {
        crate::present_mode::select_present_mode(surface_caps)
    } else {
        crate::present_mode::select_present_mode_for_frame_pacing(surface_caps, frame_pacing_mode)
    }
}

fn surface_config_for_window(
    surface_caps: &wgpu::SurfaceCapabilities,
    surface_format: wgpu::TextureFormat,
    width: u32,
    height: u32,
    present_mode: wgpu::PresentMode,
    transparent: bool,
    frame_pacing_mode: FramePacingMode,
) -> Result<wgpu::SurfaceConfiguration, LaunchError> {
    Ok(wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width,
        height,
        present_mode,
        alpha_mode: select_alpha_mode(surface_caps, transparent)?,
        view_formats: vec![],
        desired_maximum_frame_latency: desired_frame_latency(frame_pacing_mode),
    })
}

fn wgpu_renderer_for_surface(
    text_system: WgpuTextSystem,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface_format: wgpu::TextureFormat,
    backend: wgpu::Backend,
    scale_factor: f64,
) -> WgpuRenderer {
    let mut renderer = WgpuRenderer::with_text_system(text_system);
    renderer.set_root_scale(scale_factor as f32);
    renderer.init_gpu(device, queue, surface_format, backend);
    renderer
}

fn select_surface_format(
    surface_caps: &wgpu::SurfaceCapabilities,
) -> Result<wgpu::TextureFormat, LaunchError> {
    crate::surface_format::select_display_surface_format(&surface_caps.formats)
        .ok_or(LaunchError::NoSurfaceFormat)
}

fn select_alpha_mode(
    surface_caps: &wgpu::SurfaceCapabilities,
    transparent: bool,
) -> Result<wgpu::CompositeAlphaMode, LaunchError> {
    if transparent {
        return surface_caps
            .alpha_modes
            .iter()
            .copied()
            .find(|mode| *mode == wgpu::CompositeAlphaMode::PreMultiplied)
            .or_else(|| surface_caps.alpha_modes.first().copied())
            .ok_or(LaunchError::NoSurfaceAlphaMode);
    }

    surface_caps
        .alpha_modes
        .iter()
        .copied()
        .find(|mode| *mode == wgpu::CompositeAlphaMode::Opaque)
        .or_else(|| surface_caps.alpha_modes.first().copied())
        .ok_or(LaunchError::NoSurfaceAlphaMode)
}

fn native_window_level(always_on_top: bool) -> WindowLevel {
    if always_on_top {
        WindowLevel::AlwaysOnTop
    } else {
        WindowLevel::Normal
    }
}

fn current_native_window_physical_position(
    platform_probe: &NativeWindowPlatformProbe,
    window: &Arc<dyn Window>,
) -> Option<PhysicalPosition<i32>> {
    native_window_x11_outer_position_physical(platform_probe, window)
        .or_else(|| window.outer_position().ok())
}

fn current_native_window_position(
    platform_probe: &NativeWindowPlatformProbe,
    native: &NativeWindowSurface,
) -> Option<(f32, f32)> {
    current_native_window_physical_position(platform_probe, &native.window).map(|position| {
        let logical = position.to_logical::<f64>(native.window.scale_factor());
        (logical.x as f32, logical.y as f32)
    })
}

fn native_window_options_position(options: &NativeWindowOptions) -> Option<(f32, f32)> {
    match (options.x, options.y) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    }
}

fn native_window_graph_position(
    override_position: Option<cranpose_ui::Point>,
    cached_position: Option<(f32, f32)>,
    current_position: Option<(f32, f32)>,
    options_position: Option<(f32, f32)>,
    source: NativeWindowGraphPositionSource,
) -> Option<cranpose_ui::Point> {
    override_position.or_else(|| {
        let selected = match source {
            NativeWindowGraphPositionSource::CachedThenCurrent => {
                cached_position.or(current_position).or(options_position)
            }
            NativeWindowGraphPositionSource::CurrentThenCached => {
                current_position.or(cached_position).or(options_position)
            }
        }?;
        Some(cranpose_ui::Point::new(selected.0, selected.1))
    })
}

fn native_window_positions_close(a: (f32, f32), b: (f32, f32)) -> bool {
    (a.0 - b.0).abs() <= 1.0 && (a.1 - b.1).abs() <= 1.0
}

fn native_window_surface_origin(
    platform_probe: &NativeWindowPlatformProbe,
    window: &Arc<dyn Window>,
) -> Option<cranpose_ui::Point> {
    let outer = current_native_window_physical_position(platform_probe, window)?;
    let surface = window.surface_position();
    let scale_factor = window.scale_factor();
    let physical = winit::dpi::PhysicalPosition::new(outer.x + surface.x, outer.y + surface.y);
    let logical = physical.to_logical::<f64>(scale_factor);
    Some(cranpose_ui::Point::new(logical.x as f32, logical.y as f32))
}

fn native_window_screen_pointer_physical(
    platform_probe: &NativeWindowPlatformProbe,
    window: &Arc<dyn Window>,
    local: PhysicalPosition<f64>,
) -> Option<PhysicalPosition<f64>> {
    let outer = current_native_window_physical_position(platform_probe, window)?;
    let surface = window.surface_position();
    Some(PhysicalPosition::new(
        outer.x as f64 + surface.x as f64 + local.x,
        outer.y as f64 + surface.y as f64 + local.y,
    ))
}

fn native_window_local_pointer_physical(
    platform_probe: &NativeWindowPlatformProbe,
    window: &Arc<dyn Window>,
    screen: PhysicalPosition<f64>,
) -> Option<PhysicalPosition<f64>> {
    let outer = current_native_window_physical_position(platform_probe, window)?;
    let surface = window.surface_position();
    Some(PhysicalPosition::new(
        screen.x - outer.x as f64 - surface.x as f64,
        screen.y - outer.y as f64 - surface.y as f64,
    ))
}

fn native_window_surface_contains_pointer(
    platform_probe: &NativeWindowPlatformProbe,
    native: &NativeWindowSurface,
    pointer: PhysicalPosition<f64>,
) -> bool {
    let Some(outer) = current_native_window_physical_position(platform_probe, &native.window)
    else {
        return false;
    };
    physical_surface_rect_contains_pointer(
        outer,
        native.window.surface_position(),
        native.window.surface_size(),
        pointer,
    )
}

fn recovered_native_window_drag_start_pointer(
    event_pointer: Option<PhysicalPosition<f64>>,
    global_pointer: Option<NativeWindowPointerState>,
) -> Option<PhysicalPosition<f64>> {
    event_pointer.or_else(|| global_pointer.map(|pointer| pointer.position))
}

fn primary_pointer_move_should_recover_press(
    active_pointer_gesture: bool,
    synthetic_primary_down: bool,
    global_pointer: Option<NativeWindowPointerState>,
    pointer_over_surface: bool,
) -> bool {
    !active_pointer_gesture
        && !synthetic_primary_down
        && pointer_over_surface
        && global_pointer.is_some_and(|pointer| pointer.primary_down)
}

fn native_window_surface_at_logical_position_contains_pointer(
    window: &Arc<dyn Window>,
    position: cranpose_ui::Point,
    pointer: PhysicalPosition<f64>,
) -> bool {
    let outer = LogicalPosition::new(position.x as f64, position.y as f64)
        .to_physical::<i32>(window.scale_factor());
    physical_surface_rect_contains_pointer(
        outer,
        window.surface_position(),
        window.surface_size(),
        pointer,
    )
}

fn physical_surface_rect_contains_pointer(
    outer: PhysicalPosition<i32>,
    surface: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    pointer: PhysicalPosition<f64>,
) -> bool {
    let x = outer.x as f64 + surface.x as f64;
    let y = outer.y as f64 + surface.y as f64;
    let right = x + size.width as f64;
    let bottom = y + size.height as f64;
    pointer.x >= x && pointer.x <= right && pointer.y >= y && pointer.y <= bottom
}

#[derive(Clone, Copy, Debug)]
struct NativeWindowPointerState {
    position: PhysicalPosition<f64>,
    primary_down: bool,
}

#[cfg(all(
    target_os = "linux",
    not(target_arch = "wasm32"),
    feature = "desktop-x11"
))]
struct X11WindowClient {
    connection: x11rb::rust_connection::RustConnection,
    root: u32,
}

#[cfg(all(
    target_os = "linux",
    not(target_arch = "wasm32"),
    feature = "desktop-x11"
))]
enum X11WindowClientState {
    Available(Box<X11WindowClient>),
    Unavailable,
}

#[cfg(all(
    target_os = "linux",
    not(target_arch = "wasm32"),
    feature = "desktop-x11"
))]
struct NativeWindowPlatformProbe {
    x11_window_client: RefCell<Option<X11WindowClientState>>,
}

#[cfg(all(
    target_os = "linux",
    not(target_arch = "wasm32"),
    feature = "desktop-x11"
))]
impl Default for NativeWindowPlatformProbe {
    fn default() -> Self {
        Self {
            x11_window_client: RefCell::new(None),
        }
    }
}

#[cfg(not(all(
    target_os = "linux",
    not(target_arch = "wasm32"),
    feature = "desktop-x11"
)))]
#[derive(Default)]
struct NativeWindowPlatformProbe;

#[cfg(all(
    target_os = "linux",
    not(target_arch = "wasm32"),
    feature = "desktop-x11"
))]
impl NativeWindowPlatformProbe {
    fn probe_x11_window_client<R>(&self, f: impl FnOnce(&X11WindowClient) -> R) -> Option<R> {
        if self.x11_window_client.borrow().is_none() {
            *self.x11_window_client.borrow_mut() = Some(
                X11WindowClient::connect()
                    .map(Box::new)
                    .map(X11WindowClientState::Available)
                    .unwrap_or(X11WindowClientState::Unavailable),
            );
        }

        match self.x11_window_client.borrow().as_ref()? {
            X11WindowClientState::Available(client) => Some(f(client)),
            X11WindowClientState::Unavailable => None,
        }
    }
}

#[cfg(all(
    target_os = "linux",
    not(target_arch = "wasm32"),
    feature = "desktop-x11"
))]
impl X11WindowClient {
    fn connect() -> Option<Self> {
        use x11rb::connection::Connection;

        let (connection, screen_num) = x11rb::connect(None).ok()?;
        let root = connection.setup().roots.get(screen_num)?.root;
        Some(Self { connection, root })
    }

    fn pointer_state(&self) -> Option<NativeWindowPointerState> {
        use x11rb::protocol::xproto::{ConnectionExt, KeyButMask};

        let reply = self
            .connection
            .query_pointer(self.root)
            .ok()?
            .reply()
            .ok()?;
        let mask = u16::from(reply.mask);
        Some(NativeWindowPointerState {
            position: PhysicalPosition::new(reply.root_x as f64, reply.root_y as f64),
            primary_down: mask & u16::from(KeyButMask::BUTTON1) != 0,
        })
    }

    fn configure_windows(&self, windows: &[(u32, PhysicalPosition<i32>)]) -> Option<()> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{ConfigureWindowAux, ConnectionExt};

        for (window, position) in windows {
            self.connection
                .configure_window(
                    *window,
                    &ConfigureWindowAux::new().x(position.x).y(position.y),
                )
                .ok()?;
        }
        self.connection.flush().ok()?;
        Some(())
    }

    fn window_position(&self, window: u32) -> Option<PhysicalPosition<i32>> {
        use x11rb::protocol::xproto::ConnectionExt;

        let reply = self
            .connection
            .translate_coordinates(window, self.root, 0, 0)
            .ok()?
            .reply()
            .ok()?;
        Some(PhysicalPosition::new(
            reply.dst_x as i32,
            reply.dst_y as i32,
        ))
    }
}

#[cfg(all(
    target_os = "linux",
    not(target_arch = "wasm32"),
    feature = "desktop-x11"
))]
fn native_window_global_pointer_state(
    platform_probe: &NativeWindowPlatformProbe,
) -> Option<NativeWindowPointerState> {
    platform_probe
        .probe_x11_window_client(X11WindowClient::pointer_state)
        .flatten()
}

#[cfg(not(all(
    target_os = "linux",
    not(target_arch = "wasm32"),
    feature = "desktop-x11"
)))]
fn native_window_global_pointer_state(
    _platform_probe: &NativeWindowPlatformProbe,
) -> Option<NativeWindowPointerState> {
    None
}

#[cfg(all(
    target_os = "linux",
    not(target_arch = "wasm32"),
    feature = "desktop-x11"
))]
fn native_window_x11_id(window: &Arc<dyn Window>) -> Option<u32> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Xlib(handle) => Some(handle.window as u32),
        RawWindowHandle::Xcb(handle) => Some(handle.window.get()),
        _ => None,
    }
}

#[cfg(all(
    target_os = "linux",
    not(target_arch = "wasm32"),
    feature = "desktop-x11"
))]
fn native_window_x11_outer_position_physical(
    platform_probe: &NativeWindowPlatformProbe,
    window: &Arc<dyn Window>,
) -> Option<PhysicalPosition<i32>> {
    let window_id = native_window_x11_id(window)?;
    platform_probe
        .probe_x11_window_client(|client| client.window_position(window_id))
        .flatten()
}

#[cfg(not(all(
    target_os = "linux",
    not(target_arch = "wasm32"),
    feature = "desktop-x11"
)))]
fn native_window_x11_outer_position_physical(
    _platform_probe: &NativeWindowPlatformProbe,
    _window: &Arc<dyn Window>,
) -> Option<PhysicalPosition<i32>> {
    None
}

#[cfg(all(
    target_os = "linux",
    not(target_arch = "wasm32"),
    feature = "desktop-x11"
))]
fn native_window_set_outer_position_physical(
    platform_probe: &NativeWindowPlatformProbe,
    window: &Arc<dyn Window>,
    position: PhysicalPosition<i32>,
) -> bool {
    let request = NativeWindowPositionRequest {
        window: Arc::clone(window),
        logical: position.to_logical(window.scale_factor()),
        physical: position,
    };
    native_window_set_outer_positions_physical(platform_probe, &[request])
}

#[cfg(not(all(
    target_os = "linux",
    not(target_arch = "wasm32"),
    feature = "desktop-x11"
)))]
fn native_window_set_outer_position_physical(
    _platform_probe: &NativeWindowPlatformProbe,
    _window: &Arc<dyn Window>,
    _position: PhysicalPosition<i32>,
) -> bool {
    false
}

#[cfg(all(
    target_os = "linux",
    not(target_arch = "wasm32"),
    feature = "desktop-x11"
))]
fn native_window_set_outer_positions_physical(
    platform_probe: &NativeWindowPlatformProbe,
    requests: &[NativeWindowPositionRequest],
) -> bool {
    if requests.is_empty() {
        return true;
    }
    platform_probe
        .probe_x11_window_client(|client| {
            let mut operations = Vec::with_capacity(requests.len());
            for request in requests {
                let Some(window_id) = native_window_x11_id(&request.window) else {
                    return false;
                };
                operations.push((window_id, request.physical));
            }
            client.configure_windows(&operations).is_some()
        })
        .unwrap_or(false)
}

#[cfg(not(all(
    target_os = "linux",
    not(target_arch = "wasm32"),
    feature = "desktop-x11"
)))]
fn native_window_set_outer_positions_physical(
    _platform_probe: &NativeWindowPlatformProbe,
    _requests: &[NativeWindowPositionRequest],
) -> bool {
    false
}

fn wait_native_window_positions_physical(
    platform_probe: &NativeWindowPlatformProbe,
    requests: &[NativeWindowPositionRequest],
) {
    if requests.is_empty() {
        return;
    }
    if current_native_window_physical_position(platform_probe, &requests[0].window).is_none() {
        return;
    }
    let deadline = Instant::now() + NATIVE_WINDOW_POSITION_SETTLE_TIMEOUT;
    while Instant::now() < deadline {
        if requests.iter().all(|request| {
            current_native_window_physical_position(platform_probe, &request.window).is_some_and(
                |current| native_window_physical_positions_close(current, request.physical),
            )
        }) {
            return;
        }
        std::thread::sleep(NATIVE_WINDOW_POSITION_SETTLE_POLL);
    }
}

fn native_window_physical_positions_close(
    a: PhysicalPosition<i32>,
    b: PhysicalPosition<i32>,
) -> bool {
    (a.x - b.x).abs() <= 1 && (a.y - b.y).abs() <= 1
}

fn update_native_options_position(options: &mut NativeWindowOptions, x: f32, y: f32) {
    options.x = Some(x);
    options.y = Some(y);
    options.position_origin = NativeWindowPositionOrigin::Screen;
}

fn update_native_options_size(
    options: &mut NativeWindowOptions,
    window: &Arc<dyn Window>,
    width: u32,
    height: u32,
) {
    let scale_factor = window.scale_factor() as f32;
    if scale_factor > 0.0 {
        options.width = width.max(1) as f32 / scale_factor;
        options.height = height.max(1) as f32 / scale_factor;
    }
}

fn sync_native_window_state_position(
    state: Option<WindowState>,
    previous_position: Option<cranpose_ui::Point>,
    x: f32,
    y: f32,
) {
    let Some(state) = state else {
        return;
    };
    if state.position_non_reactive() == previous_position {
        state.set_position(Some(cranpose_ui::Point::new(x, y)));
    }
}

fn sync_native_window_state_size(
    state: Option<WindowState>,
    previous_size: Option<cranpose_ui::Size>,
    window: &Arc<dyn Window>,
    width: u32,
    height: u32,
) {
    let Some(state) = state else {
        return;
    };
    let Some(previous_size) = previous_size else {
        return;
    };
    if state.size_non_reactive() != previous_size {
        return;
    }
    let scale_factor = window.scale_factor() as f32;
    if scale_factor > 0.0 {
        state.set_size(cranpose_ui::Size::new(
            width.max(1) as f32 / scale_factor,
            height.max(1) as f32 / scale_factor,
        ));
    }
}

fn native_window_options_change_is_position_only(
    previous: &NativeWindowOptions,
    next: &NativeWindowOptions,
) -> bool {
    if previous == next {
        return false;
    }

    let mut previous = previous.clone();
    let next = next.clone();
    previous.x = next.x;
    previous.y = next.y;
    previous.position_origin = next.position_origin;
    previous == next
}

fn native_window_position_poll_needed(
    visible: bool,
    active_drag: bool,
    pending_programmatic_position: bool,
) -> bool {
    visible && !active_drag && pending_programmatic_position
}

fn trace_native_window_timing(args: std::fmt::Arguments<'_>) {
    if std::env::var_os("CRANPOSE_NATIVE_WINDOW_TIMING").is_some() {
        println!("native window timing: {args}");
    }
}

fn trace_native_window(args: std::fmt::Arguments<'_>) {
    if std::env::var_os("CRANPOSE_NATIVE_TRACE").is_some() {
        println!("native window trace: {args}");
    }
}

fn primary_surface_redraw_drives_app(primary_window_visible: bool, headless: bool) -> bool {
    primary_window_visible && !headless
}

fn primary_frame_waker_uses_event_proxy(primary_window_visible: bool, headless: bool) -> bool {
    !primary_surface_redraw_drives_app(primary_window_visible, headless)
}

fn primary_launch_requires_initial_redraw(primary_window_visible: bool, headless: bool) -> bool {
    primary_surface_redraw_drives_app(primary_window_visible, headless)
}

fn surface_reconfigure_requires_redraw(width: u32, height: u32) -> bool {
    width > 0 && height > 0
}

/// Whether `about_to_wait` must re-request the initial frame's redraw. The
/// first present is owed while `initial_present_pending` is set; we only issue
/// a fresh request when none is already in flight, so a redraw dropped by the
/// platform (notably macOS issuing it from `resumed`) still reaches the screen
/// without spinning once a request is pending or the frame has presented.
fn initial_present_redraw_needed(initial_present_pending: bool, redraw_pending: bool) -> bool {
    initial_present_pending && !redraw_pending
}

fn primary_declaration_host_needs_direct_update(
    primary_window_visible: bool,
    headless: bool,
    needs_redraw: bool,
    waiting_for_frame_cap: bool,
) -> bool {
    needs_redraw
        && !waiting_for_frame_cap
        && !primary_surface_redraw_drives_app(primary_window_visible, headless)
}

#[cfg(feature = "robot")]
fn robot_visible_present_target(
    primary_window_visible: bool,
    headless: bool,
    surface_dirty: bool,
    presented_frame_generation: u64,
) -> Option<u64> {
    (surface_dirty && primary_surface_redraw_drives_app(primary_window_visible, headless))
        .then(|| presented_frame_generation.saturating_add(1))
}

#[cfg(feature = "robot")]
fn robot_visible_pump_present_target(
    primary_window_visible: bool,
    headless: bool,
    frame_count: u32,
    presented_frame_generation: u64,
) -> Option<u64> {
    (frame_count > 0 && primary_surface_redraw_drives_app(primary_window_visible, headless))
        .then(|| presented_frame_generation.saturating_add(frame_count as u64))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PointerButtonFrameRequest {
    request_redraw: bool,
    reset_frame_cap: bool,
}

fn pointer_button_frame_request(input_handled: bool) -> PointerButtonFrameRequest {
    immediate_input_frame_request(input_handled)
}

fn scroll_frame_request(input_handled: bool) -> PointerButtonFrameRequest {
    immediate_input_frame_request(input_handled)
}

fn immediate_input_frame_request(input_handled: bool) -> PointerButtonFrameRequest {
    PointerButtonFrameRequest {
        request_redraw: input_handled,
        reset_frame_cap: input_handled,
    }
}

fn request_redraw_once(window: &Arc<dyn Window>, redraw_pending: &mut bool) {
    if *redraw_pending {
        return;
    }
    *redraw_pending = true;
    window.request_redraw();
}

fn apply_pointer_button_frame_request(
    window: &Arc<dyn Window>,
    last_frame_start_time: &mut Option<Instant>,
    request: PointerButtonFrameRequest,
) {
    if request.reset_frame_cap {
        *last_frame_start_time = None;
    }
    if request.request_redraw {
        window.request_redraw();
    }
}

fn apply_primary_pointer_button_frame_request(
    window: &Arc<dyn Window>,
    last_frame_start_time: &mut Option<Instant>,
    redraw_pending: &mut bool,
    request: PointerButtonFrameRequest,
) {
    if request.reset_frame_cap {
        *last_frame_start_time = None;
    }
    if request.request_redraw {
        request_redraw_once(window, redraw_pending);
    }
}

fn configure_app_surface_size(
    app: &mut AppShell<WgpuRenderer>,
    surface: &wgpu::Surface<'static>,
    surface_config: &mut wgpu::SurfaceConfiguration,
    width: u32,
    height: u32,
    viewport: (f32, f32),
) {
    if width == 0 || height == 0 {
        return;
    }

    let Some(device) = app.renderer().try_device() else {
        log::error!("desktop surface resize skipped: GPU renderer is not initialized");
        return;
    };
    surface_config.width = width;
    surface_config.height = height;
    surface.configure(device, surface_config);
    update_app_viewport(app, width, height, viewport);
}

fn update_app_viewport(
    app: &mut AppShell<WgpuRenderer>,
    width: u32,
    height: u32,
    viewport: (f32, f32),
) {
    let (logical_width, logical_height) = viewport;
    app.set_buffer_size(width, height);
    app.set_viewport(logical_width, logical_height);
}

fn surface_logical_viewport_size(width: u32, height: u32, scale_factor: f64) -> (f32, f32) {
    (
        width as f32 / scale_factor as f32,
        height as f32 / scale_factor as f32,
    )
}

fn headless_requested_viewport(settings: &AppSettings) -> Option<(f32, f32)> {
    settings.headless.then_some((
        settings.initial_width.max(1) as f32,
        settings.initial_height.max(1) as f32,
    ))
}

fn viewport_for_surface_size(
    requested_viewport: Option<(f32, f32)>,
    width: u32,
    height: u32,
    scale_factor: f64,
) -> (f32, f32) {
    requested_viewport.unwrap_or_else(|| surface_logical_viewport_size(width, height, scale_factor))
}

fn primary_viewport_for_surface_size(
    settings: &AppSettings,
    width: u32,
    height: u32,
    scale_factor: f64,
) -> (f32, f32) {
    viewport_for_surface_size(
        headless_requested_viewport(settings),
        width,
        height,
        scale_factor,
    )
}

fn update_app_scale_factor(
    app: &mut AppShell<WgpuRenderer>,
    platform: &mut DesktopWinitPlatform,
    scale_factor: f64,
) {
    platform.set_scale_factor(scale_factor);
    app.renderer().set_root_scale(scale_factor as f32);
    app.set_density(scale_factor as f32);
}

fn dispatch_mouse_wheel(
    app: &mut AppShell<WgpuRenderer>,
    platform: &DesktopWinitPlatform,
    current_modifiers: winit::keyboard::ModifiersState,
    cursor_position: Option<(f32, f32)>,
    delta: winit::event::MouseScrollDelta,
) -> bool {
    let cursor_dirty = if let Some((x, y)) = cursor_position {
        app.set_cursor(x, y)
    } else {
        false
    };

    let mut logical_delta = platform.scroll_delta(delta);

    // Ctrl+wheel is the desktop zoom gesture (mirroring browser pinch
    // semantics): translate the vertical wheel delta into a multiplicative
    // zoom step about the cursor instead of scrolling.
    if current_modifiers.contains(winit::keyboard::ModifiersState::CONTROL) {
        let zoom_factor = wheel_zoom_factor(logical_delta.y);
        log::trace!(
            target: "cranpose::input",
            "desktop ctrl+wheel zoom factor={zoom_factor:.4}"
        );
        let zoom_dirty = app.pointer_zoomed(zoom_factor);
        return cursor_dirty || zoom_dirty;
    }

    let alt_pressed = current_modifiers.contains(winit::keyboard::ModifiersState::ALT);
    if alt_pressed {
        if logical_delta.x.abs() <= f32::EPSILON {
            logical_delta.x = logical_delta.y;
        }
        logical_delta.y = 0.0;
    }

    log::trace!(
        target: "cranpose::input",
        "desktop wheel delta ({:.2},{:.2}) alt={}",
        logical_delta.x,
        logical_delta.y,
        alt_pressed
    );

    let scroll_dirty = app.pointer_scrolled(logical_delta.x, logical_delta.y);
    cursor_dirty || scroll_dirty
}

/// Converts a vertical wheel delta (logical px, positive = scroll up) into
/// a multiplicative zoom factor: one standard wheel notch (~40 logical px)
/// zooms by ~1.2x, and opposite deltas invert exactly (`f(-d) == 1/f(d)`).
fn wheel_zoom_factor(delta_y: f32) -> f32 {
    const NOTCH_LOGICAL_PX: f32 = 40.0;
    const ZOOM_PER_NOTCH: f32 = 1.2;
    ZOOM_PER_NOTCH.powf(delta_y / NOTCH_LOGICAL_PX)
}

fn dispatch_middle_click_paste(
    app: &mut AppShell<WgpuRenderer>,
    cursor_position: Option<(f32, f32)>,
) {
    if let Some((x, y)) = cursor_position {
        app.set_cursor(x, y);
    }
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    if let Some(text) = app.get_primary_selection() {
        app.on_paste(&text);
    }
}

fn cancel_app_input(app: &mut AppShell<WgpuRenderer>) {
    app.cancel_gesture();
    let _ = app.on_ime_preedit("", None);
}

fn logical_outer_position(window: &Arc<dyn Window>) -> Option<(f32, f32)> {
    window.outer_position().ok().map(|position| {
        let logical = position.to_logical::<f64>(window.scale_factor());
        (logical.x as f32, logical.y as f32)
    })
}

fn notify_native_window_moved(events: &NativeWindowEvents, x: f32, y: f32) {
    if let Some(on_moved) = &events.on_moved {
        on_moved(x, y);
    }
}

fn notify_native_window_resized(
    events: &NativeWindowEvents,
    window: &Arc<dyn Window>,
    width: u32,
    height: u32,
) {
    if let Some(on_resized) = &events.on_resized {
        let scale_factor = window.scale_factor() as f32;
        if scale_factor > 0.0 {
            on_resized(width as f32 / scale_factor, height as f32 / scale_factor);
        }
    }
}

fn notify_native_window_close_requested(events: &NativeWindowEvents) {
    if let Some(on_close_requested) = &events.on_close_requested {
        on_close_requested();
    }
}

fn native_resize_direction(direction: WindowResizeDirection) -> ResizeDirection {
    match direction {
        WindowResizeDirection::East => ResizeDirection::East,
        WindowResizeDirection::North => ResizeDirection::North,
        WindowResizeDirection::NorthEast => ResizeDirection::NorthEast,
        WindowResizeDirection::NorthWest => ResizeDirection::NorthWest,
        WindowResizeDirection::South => ResizeDirection::South,
        WindowResizeDirection::SouthEast => ResizeDirection::SouthEast,
        WindowResizeDirection::SouthWest => ResizeDirection::SouthWest,
        WindowResizeDirection::West => ResizeDirection::West,
    }
}

fn default_vsync_interval() -> Duration {
    Duration::from_nanos(16_666_667)
}

fn system_theme_from_winit(theme: winit::window::Theme) -> cranpose_services::SystemTheme {
    match theme {
        winit::window::Theme::Dark => cranpose_services::SystemTheme::Dark,
        winit::window::Theme::Light => cranpose_services::SystemTheme::Light,
    }
}

fn monitor_refresh_interval(window: &Arc<dyn Window>) -> Duration {
    window
        .current_monitor()
        .and_then(|monitor| monitor.current_video_mode())
        .and_then(|mode| mode.refresh_rate_millihertz())
        .map(|millihertz| {
            let nanos = 1_000_000_000_000u64 / u64::from(millihertz.get());
            Duration::from_nanos(nanos)
        })
        .unwrap_or_else(default_vsync_interval)
}

fn dispatch_ime_event(app: &mut AppShell<WgpuRenderer>, ime_event: winit::event::Ime) {
    use winit::event::Ime;

    match ime_event {
        Ime::Preedit(text, cursor) => {
            app.on_ime_preedit(&text, cursor);
        }
        Ime::Commit(text) => {
            let _ = app.on_ime_preedit("", None);
            app.on_paste(&text);
        }
        Ime::Enabled => {}
        Ime::Disabled => {
            app.on_ime_preedit("", None);
        }
        Ime::DeleteSurrounding {
            before_bytes,
            after_bytes,
        } => {
            // winit guarantees byte-wise UTF-8 counts, matching the shell API.
            // The buffer implementation already preserves any active preedit
            // region, as this event requires.
            let _ = app.on_ime_delete_surrounding(before_bytes, after_bytes);
        }
    }
}

/// Text-input focus hook for desktop: enables the platform IME on the window
/// while a text field is focused and disables it when focus is lost.
///
/// winit only delivers [`winit::event::Ime`] events (preedit/commit for CJK
/// and other composing input methods) while the IME is enabled via
/// [`winit::window::ImeRequest::Enable`], so without this hook the
/// `dispatch_ime_event` path never fires.
///
/// The candidate-window cursor area (`ImeCapabilities::with_cursor_area`) is
/// not reported yet: the framework does not currently expose the focused
/// field's caret rectangle to the platform layer. Until it does, IME popups
/// may appear at a default position instead of next to the caret.
struct DesktopTextInput {
    window: std::sync::Weak<dyn Window>,
}

impl DesktopTextInput {
    fn install(app: &mut AppShell<WgpuRenderer>, window: &Arc<dyn Window>) {
        app.set_platform_text_input(Rc::new(DesktopTextInput {
            window: Arc::downgrade(window),
        }));
    }
}

impl cranpose_app_shell::PlatformTextInputHandler for DesktopTextInput {
    fn show_keyboard(&self) {
        use winit::window::{ImeCapabilities, ImeEnableRequest, ImeRequest, ImeRequestData};

        let Some(window) = self.window.upgrade() else {
            return;
        };
        // No optional capabilities requested, so the enable request is always
        // well-formed.
        let Some(enable) = ImeEnableRequest::new(ImeCapabilities::new(), ImeRequestData::default())
        else {
            return;
        };
        match window.request_ime_update(ImeRequest::Enable(enable)) {
            // `show_keyboard` intentionally fires on every focus request;
            // an already enabled IME stays enabled.
            Ok(()) | Err(winit::window::ImeRequestError::AlreadyEnabled) => {}
            Err(error) => log::debug!("winit IME enable failed: {error}"),
        }
    }

    fn hide_keyboard(&self) {
        use winit::window::{ImeRequest, ImeRequestError};

        let Some(window) = self.window.upgrade() else {
            return;
        };
        match window.request_ime_update(ImeRequest::Disable) {
            Ok(()) | Err(ImeRequestError::NotEnabled) => {}
            Err(error) => log::debug!("winit IME disable failed: {error}"),
        }
    }
}

impl ApplicationHandler for App {
    fn proxy_wake_up(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.handle_primary_frame_requested(event_loop);
    }

    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        // Create window if not already created
        if self.window.is_some() {
            return;
        }

        let initial_width = self.settings.initial_width;
        let initial_height = self.settings.initial_height;
        let headless = self.settings.headless;
        let primary_window_visible = self.settings.primary_window_visible;

        let window: Arc<dyn Window> = match event_loop.create_window(
            WindowAttributes::default()
                .with_title(self.settings.window_title.clone())
                .with_surface_size(LogicalSize::new(
                    initial_width as f64,
                    initial_height as f64,
                ))
                // Hide window in headless mode for parallel robot testing
                .with_visible(!headless && primary_window_visible),
        ) {
            Ok(window) => window.into(),
            Err(error) => {
                self.abort_launch(event_loop, LaunchError::WindowCreate(error));
                return;
            }
        };

        // Initialize WGPU
        let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_descriptor.backends = wgpu::Backends::all();
        let instance = wgpu::Instance::new(instance_descriptor);

        let surface = match instance.create_surface(window.clone()) {
            Ok(surface) => surface,
            Err(error) => {
                self.abort_launch(event_loop, LaunchError::SurfaceCreate(error));
                return;
            }
        };

        let adapter =
            match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })) {
                Ok(adapter) => adapter,
                Err(error) => {
                    self.abort_launch(event_loop, LaunchError::NoAdapter(error));
                    return;
                }
            };
        let adapter_info = adapter.get_info();
        self.vsync_interval = monitor_refresh_interval(&window);

        let (device, queue) =
            match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("Main Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })) {
                Ok(pair) => pair,
                Err(error) => {
                    self.abort_launch(event_loop, LaunchError::DeviceCreate(error));
                    return;
                }
            };

        let size = window.surface_size();
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = match select_surface_format(&surface_caps) {
            Ok(format) => format,
            Err(error) => {
                self.abort_launch(event_loop, error);
                return;
            }
        };

        let present_mode = desktop_present_mode(&surface_caps, self.frame_pacing_mode);
        let surface_config = match surface_config_for_window(
            &surface_caps,
            surface_format,
            size.width.max(1),
            size.height.max(1),
            present_mode,
            false,
            self.frame_pacing_mode,
        ) {
            Ok(config) => config,
            Err(error) => {
                self.abort_launch(event_loop, error);
                return;
            }
        };

        let device = Arc::new(device);
        let queue = Arc::new(queue);

        surface.configure(&device, &surface_config);

        // Create renderer with fonts from settings
        let fonts: &[&[u8]] = self.settings.fonts.unwrap_or(&[]);
        let text_system = WgpuTextSystem::from_fonts(fonts);
        let initial_scale = window.scale_factor();
        let renderer = wgpu_renderer_for_surface(
            text_system.clone(),
            Arc::clone(&device),
            Arc::clone(&queue),
            surface_format,
            adapter_info.backend,
            initial_scale,
        );

        let viewport = primary_viewport_for_surface_size(
            &self.settings,
            size.width,
            size.height,
            initial_scale,
        );

        let Some(content) = self.content.take() else {
            self.abort_launch(event_loop, LaunchError::ContentUnavailable);
            return;
        };
        let registry = Rc::clone(&self.native_window_registry);
        let mut app = native_window::with_native_window_registry(&registry, || {
            AppShell::new_with_size_and_density(
                renderer,
                default_root_key(),
                content,
                (size.width, size.height),
                viewport,
                initial_scale as f32,
            )
        });
        #[cfg(feature = "robot")]
        app.set_semantics_enabled(self.robot_controller.is_some());

        // Apply dev options (FPS counter, etc.)
        let mut dev_options = self.settings.dev_options.clone();
        dev_options.frame_pacing_mode = self.frame_pacing_mode;
        app.set_dev_options(dev_options);

        // Enable/disable the platform IME with text-field focus so composing
        // input methods (CJK, dead keys via IME) work on desktop.
        DesktopTextInput::install(&mut app, &window);

        // Runtime-driven frame scheduling: Compose invalidations and animations
        // request frames via the runtime waker, not per-input host redraw forcing.
        let frame_waker_window = window.clone();
        let frame_waker_event_proxy = self.event_proxy.clone();
        let use_event_proxy = primary_frame_waker_uses_event_proxy(
            self.settings.primary_window_visible,
            self.settings.headless,
        );
        app.set_frame_waker(move || {
            if use_event_proxy {
                frame_waker_event_proxy.wake_up();
            } else {
                frame_waker_window.request_redraw();
            }
        });

        let mut platform = DesktopWinitPlatform::default();
        platform.set_scale_factor(initial_scale);
        let request_initial_redraw = primary_launch_requires_initial_redraw(
            self.settings.primary_window_visible,
            self.settings.headless,
        );

        // Seed the system theme from the window manager when it reports one;
        // otherwise the cached environment detection in cranpose-services
        // stands. Live changes arrive as `WindowEvent::ThemeChanged`.
        if let Some(theme) = window.theme() {
            self.platform_env
                .set_system_theme(system_theme_from_winit(theme));
        }

        self.window = Some(window);
        self.surface = Some(surface);
        self.surface_config = Some(surface_config);
        self.surface_caps = Some(surface_caps);
        self.app = Some(app);
        self.platform = Some(platform);
        self.gpu_context = Some(DesktopGpuContext {
            instance,
            adapter,
            adapter_backend: adapter_info.backend,
            device,
            queue,
            text_system,
        });
        self.primary_surface_dirty = request_initial_redraw;
        self.primary_initial_present_pending = request_initial_redraw;
        self.refresh_native_window_requests();
        self.sync_native_windows(event_loop);
        if request_initial_redraw {
            if let Some(window) = self.window.clone() {
                request_redraw_once(&window, &mut self.primary_redraw_pending);
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: WinitWindowId,
        event: WindowEvent,
    ) {
        let Some(window) = &self.window else {
            self.native_window_event(event_loop, window_id, event);
            return;
        };
        if window_id != window.id() {
            self.native_window_event(event_loop, window_id, event);
            return;
        }

        let frame_interval = self.frame_interval();
        let last_frame_start_time = self.last_frame_start_time;
        let frame_cap_deadline = last_frame_start_time
            .and_then(|started_at| frame_interval.map(|interval| started_at + interval));

        let primary_viewport_override = headless_requested_viewport(&self.settings);

        let registry = Rc::clone(&self.native_window_registry);
        let Some(app) = &mut self.app else { return };
        let Some(platform) = &mut self.platform else {
            return;
        };
        let Some(surface) = &self.surface else { return };
        let Some(surface_config) = &mut self.surface_config else {
            return;
        };

        let mut sync_native_windows_after_event = false;
        match event {
            WindowEvent::CloseRequested => {
                // Save recording if active
                if let Some(recorder) = self.recorder.take() {
                    if let Err(e) = recorder.finish() {
                        eprintln!("[Recorder] Error saving recording: {}", e);
                    }
                }
                event_loop.exit();
            }
            WindowEvent::SurfaceResized(new_size) if new_size.width > 0 && new_size.height > 0 => {
                let viewport = viewport_for_surface_size(
                    primary_viewport_override,
                    new_size.width,
                    new_size.height,
                    window.scale_factor(),
                );
                configure_app_surface_size(
                    app,
                    surface,
                    surface_config,
                    new_size.width,
                    new_size.height,
                    viewport,
                );
                self.primary_surface_dirty = true;
                request_redraw_once(window, &mut self.primary_redraw_pending);
            }
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                mut surface_size_writer,
            } => {
                update_app_scale_factor(app, platform, scale_factor);

                let new_size = window.surface_size();
                let _ = surface_size_writer.request_surface_size(new_size);
                if new_size.width > 0 && new_size.height > 0 {
                    let viewport = viewport_for_surface_size(
                        primary_viewport_override,
                        new_size.width,
                        new_size.height,
                        window.scale_factor(),
                    );
                    configure_app_surface_size(
                        app,
                        surface,
                        surface_config,
                        new_size.width,
                        new_size.height,
                        viewport,
                    );
                    self.primary_surface_dirty = true;
                    request_redraw_once(window, &mut self.primary_redraw_pending);
                }
            }
            WindowEvent::Moved(_) => {
                self.vsync_interval = monitor_refresh_interval(window);
            }
            WindowEvent::ThemeChanged(theme)
                if self
                    .platform_env
                    .set_system_theme(system_theme_from_winit(theme)) =>
            {
                app.request_root_render();
                self.primary_surface_dirty = true;
                request_redraw_once(window, &mut self.primary_redraw_pending);
            }
            WindowEvent::PointerMoved {
                position, source, ..
            } => {
                let logical = platform.pointer_position(position);
                self.last_cursor_position = Some((logical.x, logical.y));
                log::trace!(
                    target: "cranpose::input",
                    "desktop pointer move ({:.2},{:.2})",
                    logical.x,
                    logical.y
                );
                app.set_pointer_source(pointer_source_from_winit(&source));
                if app.set_cursor(logical.x, logical.y) {
                    request_redraw_once(window, &mut self.primary_redraw_pending);
                }
                let global_pointer =
                    native_window_global_pointer_state(&self.native_window_platform_probe);
                let pointer_over_surface = global_pointer.is_some_and(|pointer| {
                    native_window_local_pointer_physical(
                        &self.native_window_platform_probe,
                        window,
                        pointer.position,
                    )
                    .is_some()
                });
                #[cfg(feature = "robot")]
                let synthetic_primary_down = self
                    .robot_controller
                    .as_ref()
                    .is_some_and(RobotController::synthetic_primary_down);
                #[cfg(not(feature = "robot"))]
                let synthetic_primary_down = false;
                if primary_pointer_move_should_recover_press(
                    app.has_active_pointer_gesture(),
                    synthetic_primary_down,
                    global_pointer,
                    pointer_over_surface,
                ) {
                    log::trace!(
                        target: "cranpose::input",
                        "desktop pointer move recovered primary press"
                    );
                    let request = pointer_button_frame_request(app.pointer_pressed());
                    apply_primary_pointer_button_frame_request(
                        window,
                        &mut self.last_frame_start_time,
                        &mut self.primary_redraw_pending,
                        request,
                    );
                }
                if let Some(recorder) = &mut self.recorder {
                    recorder.record_mouse_move(logical.x, logical.y);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                // Track current keyboard modifiers for key events
                self.current_modifiers = modifiers.state();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.last_cursor_position.is_none()
                    && Self::refresh_primary_cursor_from_platform_pointer(
                        &self.native_window_platform_probe,
                        window,
                        platform,
                        app,
                        &mut self.last_cursor_position,
                    )
                {
                    request_redraw_once(window, &mut self.primary_redraw_pending);
                }
                let request = scroll_frame_request(dispatch_mouse_wheel(
                    app,
                    platform,
                    self.current_modifiers,
                    self.last_cursor_position,
                    delta,
                ));
                apply_primary_pointer_button_frame_request(
                    window,
                    &mut self.last_frame_start_time,
                    &mut self.primary_redraw_pending,
                    request,
                );
            }
            WindowEvent::PointerButton {
                state,
                button: button @ ButtonSource::Mouse(MouseButton::Left),
                ..
            } => {
                app.set_pointer_source(pointer_source_from_button(&button));
                if self.last_cursor_position.is_none()
                    && Self::refresh_primary_cursor_from_platform_pointer(
                        &self.native_window_platform_probe,
                        window,
                        platform,
                        app,
                        &mut self.last_cursor_position,
                    )
                {
                    request_redraw_once(window, &mut self.primary_redraw_pending);
                }
                let cursor_position = self.last_cursor_position;
                if let Some((x, y)) = self.last_cursor_position {
                    log::trace!(
                        target: "cranpose::input",
                        "desktop pointer button {:?} at ({:.2},{:.2})",
                        state,
                        x,
                        y
                    );
                    if app.set_cursor(x, y) {
                        request_redraw_once(window, &mut self.primary_redraw_pending);
                    }
                }
                match state {
                    ElementState::Pressed => {
                        if let Some((x, y)) = cursor_position {
                            if let Some(mode) = app.handle_dev_overlay_click(x, y) {
                                apply_frame_pacing_mode(
                                    app,
                                    surface,
                                    surface_config,
                                    self.surface_caps.as_ref(),
                                    mode,
                                );
                                self.frame_pacing_mode = mode;
                                self.last_frame_start_time = None;
                                request_redraw_once(window, &mut self.primary_redraw_pending);
                                for native in self.native_windows.values_mut() {
                                    apply_frame_pacing_mode(
                                        &mut native.app,
                                        &native.surface,
                                        &mut native.surface_config,
                                        Some(&native.surface_caps),
                                        mode,
                                    );
                                    native.frame_pacing_mode = mode;
                                    native.last_frame_start_time = None;
                                    native.window.request_redraw();
                                }
                                return;
                            }
                        }
                        let request = pointer_button_frame_request(app.pointer_pressed());
                        apply_primary_pointer_button_frame_request(
                            window,
                            &mut self.last_frame_start_time,
                            &mut self.primary_redraw_pending,
                            request,
                        );
                        if let Some(recorder) = &mut self.recorder {
                            recorder.record_mouse_down();
                        }
                    }
                    ElementState::Released => {
                        let request = pointer_button_frame_request(app.pointer_released());
                        app.sync_selection_to_primary();
                        apply_primary_pointer_button_frame_request(
                            window,
                            &mut self.last_frame_start_time,
                            &mut self.primary_redraw_pending,
                            request,
                        );
                        if let Some(recorder) = &mut self.recorder {
                            recorder.record_mouse_up();
                        }
                    }
                }
            }
            // Middle-click paste from Linux primary selection
            WindowEvent::PointerButton {
                state: ElementState::Pressed,
                button: ButtonSource::Mouse(MouseButton::Middle),
                ..
            } => {
                Self::refresh_primary_cursor_from_platform_pointer(
                    &self.native_window_platform_probe,
                    window,
                    platform,
                    app,
                    &mut self.last_cursor_position,
                );
                dispatch_middle_click_paste(app, self.last_cursor_position);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                dispatch_keyboard_input(app, self.current_modifiers, event);
            }
            WindowEvent::Focused(false) => {
                cancel_app_input(app);
            }
            WindowEvent::Ime(ime_event) => {
                dispatch_ime_event(app, ime_event);
            }
            WindowEvent::PointerLeft { .. } => {
                app.cancel_gesture();
            }
            WindowEvent::RedrawRequested => {
                self.primary_redraw_pending = false;
                if let Some(deadline) = frame_cap_deadline {
                    if deadline > Instant::now() {
                        event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
                        return;
                    }
                }
                log::trace!(target: "cranpose::input", "desktop redraw requested");
                let frame_started_at = Instant::now();
                #[cfg(feature = "robot")]
                let robot_surface_dirty_before_update = self.robot_visible_surface_dirty;
                #[cfg(not(feature = "robot"))]
                let robot_surface_dirty_before_update = false;
                let primary_surface_dirty_before_update = self.primary_surface_dirty;
                app.set_density(window.scale_factor() as f32);
                let update_result = update_app_with_native_window_registry(app, &registry);
                #[cfg(feature = "robot")]
                if let Some(controller) = &mut self.robot_controller {
                    controller.record_idle_update_result(update_result);
                }
                let after_update = Instant::now();
                sync_native_windows_after_event = true;

                if surface_present_required(
                    primary_surface_dirty_before_update || robot_surface_dirty_before_update,
                    update_result.visual_changed,
                    app.needs_redraw(),
                ) {
                    self.primary_surface_dirty = true;
                    let output = match current_surface_texture(surface, "primary window") {
                        SurfaceFrame::Ready(output) => output,
                        SurfaceFrame::Reconfigure => {
                            let size = window.surface_size();
                            let viewport = viewport_for_surface_size(
                                primary_viewport_override,
                                size.width,
                                size.height,
                                window.scale_factor(),
                            );
                            configure_app_surface_size(
                                app,
                                surface,
                                surface_config,
                                size.width,
                                size.height,
                                viewport,
                            );
                            if surface_reconfigure_requires_redraw(size.width, size.height) {
                                request_redraw_once(window, &mut self.primary_redraw_pending);
                            }
                            return;
                        }
                        SurfaceFrame::Skip => {
                            return;
                        }
                    };
                    let after_acquire = Instant::now();

                    let view = output
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());

                    if let Err(err) =
                        app.renderer()
                            .render(&view, surface_config.width, surface_config.height)
                    {
                        log::error!("render failed: {err:?}");
                        return;
                    }
                    let after_render = Instant::now();

                    output.present();
                    let after_present = Instant::now();
                    self.primary_surface_dirty = false;
                    self.primary_initial_present_pending = false;
                    app.record_presented_frame(frame_started_at, after_render);
                    log_desktop_frame_telemetry(
                        frame_started_at,
                        after_update,
                        after_acquire,
                        after_render,
                        after_present,
                        "primary",
                    );
                    self.last_frame_start_time = Some(frame_started_at);
                    #[cfg(feature = "robot")]
                    {
                        self.presented_frame_generation =
                            self.presented_frame_generation.saturating_add(1);
                        self.robot_visible_surface_dirty = false;
                    }
                    if should_chain_no_vsync_redraw(
                        frame_interval,
                        app.frame_schedule().needs_frame,
                    ) {
                        request_redraw_once(window, &mut self.primary_redraw_pending);
                    }
                } else {
                    #[cfg(feature = "robot")]
                    {
                        self.robot_visible_surface_dirty = app.needs_redraw();
                    }
                }
            }
            _ => {}
        }

        if sync_native_windows_after_event {
            self.sync_native_windows(event_loop);
        }
    }

    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        let now = Instant::now();
        if self.poll_native_window_global_primary_press() {
            self.refresh_native_window_requests();
            self.sync_native_windows(event_loop);
        }
        if self.poll_active_native_window_drags(now) {
            self.refresh_native_window_requests();
            self.sync_native_windows(event_loop);
        }
        let primary_pointer_polled = self.poll_primary_pointer_gesture();
        if self.poll_external_native_window_moves(now) {
            self.refresh_native_window_requests();
            self.sync_native_windows(event_loop);
        }

        let frame_interval = self.frame_interval();
        let last_frame_start_time = self.last_frame_start_time;
        let registry = Rc::clone(&self.native_window_registry);
        let Some(app) = &mut self.app else { return };
        let Some(window) = self.window.clone() else {
            return;
        };

        // The first frame still owes the surface a present, but the redraw
        // requested during `resumed` can be dropped on macOS, so re-request it
        // here until that frame actually reaches the screen. Cleared on present.
        if initial_present_redraw_needed(
            self.primary_initial_present_pending,
            self.primary_redraw_pending,
        ) {
            request_redraw_once(&window, &mut self.primary_redraw_pending);
        }

        // Handle pending robot commands
        #[cfg(feature = "robot")]
        if let Some(controller) = &mut self.robot_controller {
            let mut robot_visual_dirty = false;
            // Process new commands
            while let Ok(cmd) = controller.rx.try_recv() {
                match cmd {
                    RobotCommand::Click { x, y } => {
                        let cursor_dirty = app.set_cursor(x, y);
                        controller.begin_synthetic_primary_gesture();
                        let press_dirty = app.pointer_pressed();
                        let release_dirty = app.pointer_released();
                        controller.end_synthetic_primary_gesture();
                        robot_visual_dirty |= cursor_dirty || press_dirty || release_dirty;
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::MoveTo { x, y } => {
                        robot_visual_dirty |= app.set_cursor(x, y);
                        // Record for robot test generation
                        if let Some(recorder) = &mut self.recorder {
                            recorder.record_mouse_move(x, y);
                        }
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::MouseDown => {
                        controller.begin_synthetic_primary_gesture();
                        robot_visual_dirty |= app.pointer_pressed();
                        // Record for robot test generation
                        if let Some(recorder) = &mut self.recorder {
                            recorder.record_mouse_down();
                        }
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::MouseUp => {
                        robot_visual_dirty |= app.pointer_released();
                        controller.end_synthetic_primary_gesture();
                        // Record for robot test generation
                        if let Some(recorder) = &mut self.recorder {
                            recorder.record_mouse_up();
                        }
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::MouseScroll { delta_x, delta_y } => {
                        if app.pointer_scrolled(delta_x, delta_y) {
                            robot_visual_dirty = true;
                        }
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::MouseScrollAndWaitForFrame { delta_x, delta_y } => {
                        let consumed = app.pointer_scrolled(delta_x, delta_y);
                        if !consumed {
                            let _ = controller.tx.send(RobotResponse::Ok);
                            continue;
                        }

                        if let Some(target) = robot_visible_pump_present_target(
                            self.settings.primary_window_visible,
                            self.settings.headless,
                            1,
                            self.presented_frame_generation,
                        ) {
                            robot_visual_dirty = true;
                            controller.waiting_for_pump_present_generation = Some(target);
                            self.last_frame_start_time = None;
                            request_redraw_once(&window, &mut self.primary_redraw_pending);
                        } else {
                            let update_result =
                                update_app_with_native_window_registry(app, &registry);
                            self.robot_visible_surface_dirty =
                                app.needs_redraw() || update_result.visual_changed;
                            let _ = controller.tx.send(RobotResponse::Ok);
                        }
                    }
                    RobotCommand::MouseScrollSequenceAndWaitForFrames {
                        delta_x,
                        delta_y,
                        count,
                    } => {
                        if count == 0 {
                            let _ = controller.tx.send(RobotResponse::Ok);
                            continue;
                        }
                        let consumed = app.pointer_scrolled(delta_x, delta_y);
                        if !consumed {
                            let _ = controller.tx.send(RobotResponse::Ok);
                            continue;
                        }

                        if let Some(target) = robot_visible_pump_present_target(
                            self.settings.primary_window_visible,
                            self.settings.headless,
                            1,
                            self.presented_frame_generation,
                        ) {
                            controller.scroll_sequence = Some(RobotScrollSequence {
                                delta_x,
                                delta_y,
                                remaining: count.saturating_sub(1),
                            });
                            robot_visual_dirty = true;
                            controller.waiting_for_pump_present_generation = Some(target);
                            self.last_frame_start_time = None;
                            request_redraw_once(&window, &mut self.primary_redraw_pending);
                        } else {
                            let update_result =
                                update_app_with_native_window_registry(app, &registry);
                            self.robot_visible_surface_dirty =
                                app.needs_redraw() || update_result.visual_changed;
                            let _ = controller.tx.send(RobotResponse::Ok);
                        }
                    }

                    RobotCommand::TouchDown { x, y } => {
                        let cursor_dirty = app.set_cursor(x, y);
                        controller.begin_synthetic_primary_gesture();
                        let press_dirty = app.pointer_pressed();
                        robot_visual_dirty |= cursor_dirty || press_dirty;
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::TouchMove { x, y } => {
                        robot_visual_dirty |= app.set_cursor(x, y);
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::TouchMoveAndWaitForFrame { x, y } => {
                        let visual_dirty = app.set_cursor(x, y);
                        if visual_dirty {
                            robot_visual_dirty = true;
                        }
                        let present_target = visual_dirty
                            .then(|| {
                                robot_visible_pump_present_target(
                                    self.settings.primary_window_visible,
                                    self.settings.headless,
                                    1,
                                    self.presented_frame_generation,
                                )
                            })
                            .flatten();
                        if let Some(target) = present_target {
                            controller.waiting_for_pump_present_generation = Some(target);
                            self.last_frame_start_time = None;
                            request_redraw_once(&window, &mut self.primary_redraw_pending);
                        } else {
                            let update_result =
                                update_app_with_native_window_registry(app, &registry);
                            self.robot_visible_surface_dirty =
                                app.needs_redraw() || update_result.visual_changed;
                            let _ = controller.tx.send(RobotResponse::Ok);
                        }
                    }
                    RobotCommand::TouchUp { x, y } => {
                        let cursor_dirty = app.set_cursor(x, y);
                        let release_dirty = app.pointer_released();
                        controller.end_synthetic_primary_gesture();
                        robot_visual_dirty |= cursor_dirty || release_dirty;
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::GetSemantics => {
                        pump_robot_frame(app, &registry);
                        let semantics = extract_semantics(app);
                        let _ = controller.tx.send(RobotResponse::Semantics(semantics));
                    }
                    RobotCommand::FindText { text, match_kind } => {
                        pump_robot_frame(app, &registry);
                        let result = find_text_in_app(app, &text, match_kind);
                        let _ = controller.tx.send(RobotResponse::SemanticQuery(result));
                    }
                    RobotCommand::FindButton { text, match_kind } => {
                        pump_robot_frame(app, &registry);
                        let result = find_button_in_app(app, &text, match_kind);
                        let _ = controller.tx.send(RobotResponse::SemanticQuery(result));
                    }
                    RobotCommand::GetScreenshot => {
                        pump_robot_frame(app, &registry);
                        match capture_screenshot(app) {
                            Ok(screenshot) => {
                                let _ = controller.tx.send(RobotResponse::Screenshot(screenshot));
                            }
                            Err(err) => {
                                let _ = controller.tx.send(RobotResponse::Error(err));
                            }
                        }
                    }
                    RobotCommand::GetScreenshotWithScale(scale) => {
                        pump_robot_frame(app, &registry);
                        match capture_screenshot_with_scale(app, scale) {
                            Ok(screenshot) => {
                                let _ = controller.tx.send(RobotResponse::Screenshot(screenshot));
                            }
                            Err(err) => {
                                let _ = controller.tx.send(RobotResponse::Error(err));
                            }
                        }
                    }
                    RobotCommand::GetRenderStats => {
                        let _ = controller.tx.send(RobotResponse::RenderStats(Box::new(
                            app.renderer().last_frame_stats(),
                        )));
                    }
                    RobotCommand::GetFpsStats => {
                        let _ = controller.tx.send(RobotResponse::FpsStats(app.fps_stats()));
                    }
                    RobotCommand::ResetFpsStats => {
                        app.reset_fps_stats();
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::GetLastFlingVelocity => {
                        let velocity =
                            app.debug_enter_app_context(cranpose_ui::debug_last_fling_velocity);
                        let _ = controller.tx.send(RobotResponse::F32(velocity));
                    }
                    RobotCommand::ResetLastFlingVelocity => {
                        app.debug_enter_app_context(cranpose_ui::debug_reset_last_fling_velocity);
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::GetRenderCpuAllocationStats => {
                        let _ =
                            controller
                                .tx
                                .send(RobotResponse::RenderCpuAllocationStats(Box::new(
                                    app.renderer().debug_cpu_allocation_stats(),
                                )));
                    }
                    RobotCommand::GetRuntimeLeakDebugStats => {
                        let _ = controller
                            .tx
                            .send(RobotResponse::RuntimeLeakDebugStats(Box::new(
                                app.debug_runtime_leak_stats(),
                            )));
                    }
                    RobotCommand::MeasureText { text, style } => {
                        let metrics = app.debug_enter_app_context(|| {
                            let text = cranpose_ui::text::AnnotatedString::from(text.as_str());
                            cranpose_ui::measure_text(&text, &style)
                        });
                        let _ = controller.tx.send(RobotResponse::TextMetrics(metrics));
                    }
                    RobotCommand::HasFocusedTextField => {
                        let focused = app.debug_enter_app_context(|| {
                            cranpose_ui::text_field_focus::has_focused_field()
                        });
                        let _ = controller.tx.send(RobotResponse::Bool(focused));
                    }
                    RobotCommand::SetSemanticsEnabled(enabled) => {
                        app.set_semantics_enabled(enabled);
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::InvokeAppHook { name, argument } => {
                        let response = match self.robot_app_hook.as_mut() {
                            Some(hook) => hook(name, argument).map(RobotResponse::AppHookResult),
                            None => Err("robot app hook not configured".to_string()),
                        };
                        match response {
                            Ok(response) => {
                                let _ = controller.tx.send(response);
                            }
                            Err(err) => {
                                let _ = controller.tx.send(RobotResponse::Error(err));
                            }
                        }
                        robot_visual_dirty = true;
                    }
                    RobotCommand::DriverPanicked(message) => {
                        self.abort_launch(event_loop, LaunchError::TestDriverPanic(message));
                        return;
                    }
                    RobotCommand::TypeText(text) => {
                        use cranpose_app_shell::{KeyEvent, KeyEventType, Modifiers};

                        // Send key events for each character
                        for ch in text.chars() {
                            // Map character to key code (simplified)
                            let key_code = char_to_key_code(ch);
                            let key_event = KeyEvent::new(
                                key_code,
                                ch.to_string(),
                                Modifiers::NONE,
                                KeyEventType::KeyDown,
                            );
                            app.on_key_event(&key_event);
                        }
                        // Process the key events immediately to update layout/semantics
                        let update_result = update_app_with_native_window_registry(app, &registry);
                        robot_visual_dirty |= update_result.visual_changed || app.needs_redraw();
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::SendKey(key) => {
                        use cranpose_app_shell::{KeyEvent, KeyEventType, Modifiers};

                        let (key_code, text) = robot_key_code_and_text(&key);
                        let key_event =
                            KeyEvent::new(key_code, text, Modifiers::NONE, KeyEventType::KeyDown);
                        app.on_key_event(&key_event);
                        let update_result = update_app_with_native_window_registry(app, &registry);
                        robot_visual_dirty |= update_result.visual_changed || app.needs_redraw();
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::SendKeyWithModifiers {
                        key,
                        shift,
                        ctrl,
                        alt,
                        meta,
                    } => {
                        use cranpose_app_shell::{KeyEvent, KeyEventType, Modifiers};

                        let (key_code, text) = robot_key_code_and_text(&key);
                        let modifiers = Modifiers {
                            shift,
                            ctrl,
                            alt,
                            meta,
                        };
                        let key_event =
                            KeyEvent::new(key_code, text, modifiers, KeyEventType::KeyDown);
                        app.on_key_event(&key_event);
                        let update_result = update_app_with_native_window_registry(app, &registry);
                        robot_visual_dirty |= update_result.visual_changed || app.needs_redraw();
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::WaitForIdle => {
                        controller.start_idle_wait();
                        let visual_frame_pending = self.robot_visible_surface_dirty
                            || robot_visual_dirty
                            || self.primary_redraw_pending;
                        controller.waiting_for_present_generation = robot_visible_present_target(
                            self.settings.primary_window_visible,
                            self.settings.headless,
                            visual_frame_pending,
                            self.presented_frame_generation,
                        );
                        if controller.waiting_for_present_generation.is_some() {
                            request_redraw_once(&window, &mut self.primary_redraw_pending);
                        }
                    }
                    RobotCommand::PumpFrames { count } => {
                        let update_result =
                            native_window::with_native_window_registry(&registry, || {
                                let mut result = FrameUpdateResult::default();
                                for _ in 0..count {
                                    let frame_result =
                                        app.update_after_frame_interval(ROBOT_PUMP_FRAME_INTERVAL);
                                    result.visual_changed |= frame_result.visual_changed;
                                    result.structure_changed |= frame_result.structure_changed;
                                }
                                result
                            });
                        robot_visual_dirty |= update_result.visual_changed || app.needs_redraw();
                        let present_target = robot_visual_dirty
                            .then(|| {
                                robot_visible_pump_present_target(
                                    self.settings.primary_window_visible,
                                    self.settings.headless,
                                    count,
                                    self.presented_frame_generation,
                                )
                            })
                            .flatten();
                        if let Some(target) = present_target {
                            controller.waiting_for_pump_present_generation = Some(target);
                            self.last_frame_start_time = None;
                            request_redraw_once(&window, &mut self.primary_redraw_pending);
                        } else {
                            let _ = controller.tx.send(RobotResponse::Ok);
                        }
                    }
                    RobotCommand::WaitForPresentFrame => {
                        let visual_frame_pending =
                            self.robot_visible_surface_dirty || app.needs_redraw();
                        let present_target = visual_frame_pending
                            .then(|| {
                                robot_visible_pump_present_target(
                                    self.settings.primary_window_visible,
                                    self.settings.headless,
                                    1,
                                    self.presented_frame_generation,
                                )
                            })
                            .flatten();
                        if let Some(target) = present_target {
                            controller.waiting_for_pump_present_generation = Some(target);
                            self.last_frame_start_time = None;
                            request_redraw_once(&window, &mut self.primary_redraw_pending);
                        } else {
                            let update_result =
                                update_app_with_native_window_registry(app, &registry);
                            self.robot_visible_surface_dirty =
                                app.needs_redraw() || update_result.visual_changed;
                            let _ = controller.tx.send(RobotResponse::Ok);
                        }
                    }
                    RobotCommand::Exit => {
                        let _ = controller.tx.send(RobotResponse::Ok);
                        event_loop.exit();
                    }
                }
            }
            if robot_visual_dirty {
                self.robot_visible_surface_dirty = true;
                if primary_surface_redraw_drives_app(
                    self.settings.primary_window_visible,
                    self.settings.headless,
                ) {
                    self.last_frame_start_time = None;
                    request_redraw_once(&window, &mut self.primary_redraw_pending);
                }
            }

            if let Some(target_generation) = controller.waiting_for_pump_present_generation {
                if self.presented_frame_generation >= target_generation {
                    if let Some(mut sequence) = controller.scroll_sequence.take() {
                        if sequence.remaining == 0 {
                            controller.waiting_for_pump_present_generation = None;
                            self.robot_visible_surface_dirty = app.needs_redraw();
                            let _ = controller.tx.send(RobotResponse::Ok);
                        } else if app.pointer_scrolled(sequence.delta_x, sequence.delta_y) {
                            sequence.remaining = sequence.remaining.saturating_sub(1);
                            controller.scroll_sequence = Some(sequence);
                            controller.waiting_for_pump_present_generation =
                                Some(self.presented_frame_generation.saturating_add(1));
                            self.robot_visible_surface_dirty = true;
                            self.last_frame_start_time = None;
                            request_redraw_once(&window, &mut self.primary_redraw_pending);
                        } else {
                            controller.waiting_for_pump_present_generation = None;
                            self.robot_visible_surface_dirty = app.needs_redraw();
                            let _ = controller.tx.send(RobotResponse::Ok);
                        }
                    } else {
                        controller.waiting_for_pump_present_generation = None;
                        self.robot_visible_surface_dirty = app.needs_redraw();
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                } else {
                    request_redraw_once(&window, &mut self.primary_redraw_pending);
                }
            }

            // Handle ongoing wait_for_idle
            if controller.waiting_for_idle {
                const MAX_IDLE_ITERATIONS: u32 = 600;

                let frame_schedule = app.frame_schedule();
                let needs_update = frame_schedule.needs_update;
                let needs_frame = frame_schedule.needs_frame;
                let has_active_animations = app.has_active_animations();
                let visible_redraw_pending = self.robot_visible_surface_dirty || app.needs_redraw();
                if controller.waiting_for_present_generation.is_none()
                    && self.primary_redraw_pending
                    && visible_redraw_pending
                {
                    controller.waiting_for_present_generation = robot_visible_present_target(
                        self.settings.primary_window_visible,
                        self.settings.headless,
                        true,
                        self.presented_frame_generation,
                    );
                }
                let present_still_required = visible_redraw_pending;
                let waiting_for_present = present_still_required
                    && controller
                        .waiting_for_present_generation
                        .is_some_and(|target| self.presented_frame_generation < target);
                let frame_only = needs_frame && !needs_update && !waiting_for_present;
                let animation_loop_only = robot_wait_for_idle_animation_loop_only(
                    has_active_animations,
                    waiting_for_present,
                    controller.idle_iterations,
                    controller.idle_structure_clean_frames,
                );

                if frame_only || animation_loop_only {
                    controller.finish_idle_wait();
                    self.robot_visible_surface_dirty = false;
                    let _ = controller.tx.send(RobotResponse::Ok);
                } else if !needs_frame && !waiting_for_present {
                    let mut finish_idle = true;
                    if needs_update {
                        let update_result = update_app_with_native_window_registry(app, &registry);
                        controller.record_idle_update_result(update_result);
                        self.robot_visible_surface_dirty =
                            app.needs_redraw() || update_result.visual_changed;
                        let follow_up_schedule = app.frame_schedule();
                        if follow_up_schedule.needs_frame || update_result.structure_changed {
                            finish_idle = false;
                            if primary_surface_redraw_drives_app(
                                self.settings.primary_window_visible,
                                self.settings.headless,
                            ) {
                                request_redraw_once(&window, &mut self.primary_redraw_pending);
                            }
                            controller.idle_iterations += 1;
                        }
                    }
                    if finish_idle {
                        controller.finish_idle_wait();
                        self.robot_visible_surface_dirty = false;
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                } else {
                    if needs_frame
                        && primary_surface_redraw_drives_app(
                            self.settings.primary_window_visible,
                            self.settings.headless,
                        )
                    {
                        request_redraw_once(&window, &mut self.primary_redraw_pending);
                    } else {
                        let update_result = update_app_with_native_window_registry(app, &registry);
                        controller.record_idle_update_result(update_result);
                        self.robot_visible_surface_dirty =
                            app.needs_redraw() || update_result.visual_changed;
                    }
                    controller.idle_iterations += 1;

                    if controller.idle_iterations % 50 == 0 {
                        log::debug!(
                            "wait_for_idle iteration {}: needs_update={}, needs_redraw={}, has_animations={}, waiting_for_present={}, presented_frame_generation={}, target_present_generation={:?}",
                            controller.idle_iterations,
                            needs_update,
                            app.needs_redraw(),
                            has_active_animations,
                            waiting_for_present,
                            self.presented_frame_generation,
                            controller.waiting_for_present_generation
                        );
                    }

                    if controller.idle_iterations >= MAX_IDLE_ITERATIONS {
                        controller.finish_idle_wait();
                        let _ = controller.tx.send(RobotResponse::Error(format!(
                            "wait_for_idle: timed out after {} iterations; needs_update={}, needs_redraw={}, has_animations={}, waiting_for_present={}",
                            MAX_IDLE_ITERATIONS,
                            needs_update,
                            app.needs_redraw(),
                            has_active_animations,
                            waiting_for_present
                        )));
                    }
                }
            }
        }

        let frame_schedule = app.frame_schedule();
        let has_active_animations = app.has_active_animations();
        let needs_update = frame_schedule.needs_update;
        let needs_redraw = frame_schedule.needs_frame;
        if needs_redraw {
            log::trace!(
                target: "cranpose::input",
                "about_to_wait needs_redraw={needs_redraw}"
            );
        }
        let next_frame_time = last_frame_start_time
            .and_then(|started_at| frame_interval.map(|interval| started_at + interval));
        let waiting_for_frame_cap =
            needs_redraw && next_frame_time.is_some_and(|deadline| deadline > now);
        let direct_declaration_update = primary_declaration_host_needs_direct_update(
            self.settings.primary_window_visible,
            self.settings.headless,
            needs_redraw,
            waiting_for_frame_cap,
        );
        if needs_update && !needs_redraw && !waiting_for_frame_cap {
            let update_result = update_app_with_native_window_registry(app, &registry);
            if update_result.visual_changed || app.needs_redraw() {
                request_redraw_once(&window, &mut self.primary_redraw_pending);
            }
        } else if needs_redraw && !waiting_for_frame_cap {
            if direct_declaration_update {
                trace_native_window(format_args!(
                    "primary declaration host direct update visible={} headless={}",
                    self.settings.primary_window_visible, self.settings.headless
                ));
                let frame_started_at = Instant::now();
                let update_result = update_app_with_native_window_registry(app, &registry);
                if update_result.visual_changed {
                    app.record_presented_frame(frame_started_at, Instant::now());
                    self.last_frame_start_time = Some(frame_started_at);
                }
            } else {
                request_redraw_once(&window, &mut self.primary_redraw_pending);
            }
        }
        let primary_next_event_time = frame_schedule.next_deadline;
        if direct_declaration_update {
            self.sync_native_windows(event_loop);
        }

        let mut native_has_active_animations = false;
        let mut native_drag_deadline: Option<Instant> = None;
        let native_position_poll_deadline = self
            .native_windows
            .values()
            .any(|native| {
                native_window_position_poll_needed(
                    native.options.visible,
                    native.active_drag.is_some(),
                    native.pending_outer_positions.has_pending(),
                )
            })
            .then_some(self.next_native_window_position_poll_at);
        let mut native_frame_cap_deadline: Option<Instant> = None;
        let mut native_next_event_time: Option<Instant> = None;
        for native in self.native_windows.values_mut() {
            if !native.options.visible {
                continue;
            }

            let frame_schedule = native.app.frame_schedule();
            let has_active_animations = native.app.has_active_animations();
            native_has_active_animations |= has_active_animations;
            let needs_update = frame_schedule.needs_update;
            let needs_redraw = frame_schedule.needs_frame;
            let next_frame_time = native.last_frame_start_time.and_then(|started_at| {
                native
                    .frame_interval()
                    .map(|interval| started_at + interval)
            });
            let waiting_for_frame_cap =
                needs_redraw && next_frame_time.is_some_and(|deadline| deadline > now);

            if needs_update && !needs_redraw && !waiting_for_frame_cap {
                let update_result =
                    update_app_with_native_window_registry(&mut native.app, &registry);
                if update_result.visual_changed || native.app.needs_redraw() {
                    native.window.request_redraw();
                }
            } else if needs_redraw && !waiting_for_frame_cap {
                native.window.request_redraw();
            }
            if let Some(active_drag) = native.active_drag {
                let next_poll_at = active_drag.next_poll_at();
                native_drag_deadline = Some(
                    native_drag_deadline
                        .map(|current| current.min(next_poll_at))
                        .unwrap_or(next_poll_at),
                );
            }
            if waiting_for_frame_cap {
                if let Some(deadline) = next_frame_time {
                    native_frame_cap_deadline = Some(
                        native_frame_cap_deadline
                            .map(|current| current.min(deadline))
                            .unwrap_or(deadline),
                    );
                }
            }
            if let Some(next_time) = frame_schedule.next_deadline {
                native_next_event_time = Some(
                    native_next_event_time
                        .map(|current| current.min(next_time))
                        .unwrap_or(next_time),
                );
            }
        }

        // Smart ControlFlow: only Poll when necessary
        #[cfg(feature = "robot")]
        let robot_needs_poll = self.robot_controller.is_some();

        #[cfg(not(feature = "robot"))]
        let robot_needs_poll = false;

        // Poll continuously when:
        // - Active animations are running
        // - Robot test is active
        if robot_needs_poll
            || primary_pointer_polled
            || native_drag_deadline.is_some()
            || native_position_poll_deadline.is_some_and(|deadline| deadline <= now)
        {
            event_loop.set_control_flow(ControlFlow::Poll);
        } else if let Some(deadline) = [
            next_frame_time.filter(|_| waiting_for_frame_cap),
            native_frame_cap_deadline,
            native_position_poll_deadline,
        ]
        .into_iter()
        .flatten()
        .min()
        {
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        } else if has_active_animations || native_has_active_animations {
            event_loop.set_control_flow(ControlFlow::Poll);
        } else if let Some(next_time) = [primary_next_event_time, native_next_event_time]
            .into_iter()
            .flatten()
            .min()
        {
            // Cursor blink uses timer-based scheduling (not continuous poll)
            event_loop.set_control_flow(ControlFlow::WaitUntil(next_time));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}

/// Runs a desktop Compose application with wgpu rendering.
///
/// Called by `AppLauncher::run_desktop()`. This is the framework-level
/// entrypoint that manages the desktop event loop and rendering.
///
/// **Note:** Applications should use `AppLauncher` instead of calling this directly.
#[allow(unused_mut)]
pub fn try_run(
    mut settings: AppSettings,
    content: impl FnMut() + 'static,
) -> Result<(), LaunchError> {
    let event_loop = EventLoop::builder()
        .build()
        .map_err(LaunchError::EventLoopCreate)?;
    let event_proxy = event_loop.create_proxy();
    let launch_error = Rc::new(RefCell::new(None));

    // Spawn test driver if present
    #[cfg(feature = "robot")]
    let robot_controller = if let Some(driver) = settings.test_driver.take() {
        let (controller, robot) = RobotController::new();
        let panic_tx = robot.command_sender();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                driver(robot);
            }));
            if let Err(payload) = result {
                let _ = panic_tx.send(RobotCommand::DriverPanicked(panic_payload_message(payload)));
            }
        });
        Some(controller)
    } else {
        None
    };

    let mut app = App::new(settings, content, Rc::clone(&launch_error), event_proxy);

    #[cfg(feature = "robot")]
    if let Some(controller) = robot_controller {
        app.set_robot_controller(controller);
    }

    let run_result = event_loop.run_app(app);
    if let Some(error) = launch_error.borrow_mut().take() {
        return Err(error);
    }

    run_result.map_err(LaunchError::EventLoopRun)
}

/// Runs a desktop application and exits the process on success.
///
/// Use [`try_run`] when the caller needs to handle launch failures explicitly.
#[allow(unused_mut)]
pub fn run(settings: AppSettings, content: impl FnMut() + 'static) -> ! {
    try_run(settings, content).unwrap_or_else(|error| {
        crate::launcher::exit_after_launch_error("desktop launch failed", error)
    });
    std::process::exit(0)
}

#[cfg(feature = "robot")]
fn capture_screenshot(app: &mut AppShell<WgpuRenderer>) -> Result<RobotScreenshot, String> {
    let logical_size = app.viewport_size();
    let (width, height, capture_scale) =
        resolve_robot_screenshot_params(app.buffer_size(), Some(logical_size));

    let captured = app
        .renderer()
        .capture_frame_with_scale(width, height, capture_scale)
        .map_err(|err| format!("Failed to capture GPU screenshot: {err:?}"))?;

    let (logical_width, logical_height) = logical_size;

    Ok(RobotScreenshot {
        width: captured.width,
        height: captured.height,
        logical_width,
        logical_height,
        pixels: captured.pixels,
    })
}

#[cfg(feature = "robot")]
fn capture_screenshot_with_scale(
    app: &mut AppShell<WgpuRenderer>,
    scale: f32,
) -> Result<RobotScreenshot, String> {
    let (logical_width, logical_height) = app.viewport_size();
    let width = (logical_width * scale).ceil().max(1.0) as u32;
    let height = (logical_height * scale).ceil().max(1.0) as u32;

    let captured = app
        .renderer()
        .capture_frame_with_scale(width, height, scale)
        .map_err(|err| format!("Failed to capture GPU screenshot: {err:?}"))?;

    Ok(RobotScreenshot {
        width: captured.width,
        height: captured.height,
        logical_width,
        logical_height,
        pixels: captured.pixels,
    })
}

#[cfg(feature = "robot")]
fn resolve_robot_screenshot_params(
    buffer_size: (u32, u32),
    fallback_logical_size: Option<(f32, f32)>,
) -> (u32, u32, f32) {
    if let Some((logical_width, logical_height)) = fallback_logical_size {
        let width = logical_width.ceil().max(1.0) as u32;
        let height = logical_height.ceil().max(1.0) as u32;
        return (width, height, 1.0);
    }

    let (buffer_width, buffer_height) = buffer_size;
    (buffer_width.max(1), buffer_height.max(1), 1.0)
}

#[cfg(test)]
mod tests {
    use super::{
        clamp_rect_to_monitor_delta, frame_interval_for_mode, initial_present_redraw_needed,
        native_window_graph_position, native_window_options_change_is_position_only,
        native_window_position_poll_needed, nearest_monitor_to_rect,
        physical_surface_rect_contains_pointer, pointer_button_frame_request,
        primary_declaration_host_needs_direct_update, primary_frame_waker_uses_event_proxy,
        primary_launch_requires_initial_redraw, primary_pointer_move_should_recover_press,
        primary_surface_redraw_drives_app, primary_viewport_for_surface_size,
        recovered_native_window_drag_start_pointer, scroll_frame_request,
        should_chain_no_vsync_redraw, surface_reconfigure_requires_redraw, App, DesktopRect,
        FramePacingMode, NativeWindowDragSession, NativeWindowGraphPositionSource,
        NativeWindowOptions, NativeWindowPointerState, NativeWindowPollingDragSession,
        NativeWindowPositionObservation, NativeWindowPositionOrigin, PendingNativeWindowPositions,
    };
    use crate::launcher::AppSettings;
    use std::time::Instant;
    use winit::dpi::{PhysicalPosition, PhysicalSize};

    #[cfg(feature = "robot")]
    use super::{
        resolve_robot_screenshot_params, robot_visible_present_target,
        robot_visible_pump_present_target, RobotController,
    };

    #[test]
    fn native_window_screen_position_is_declarative() {
        let options = NativeWindowOptions::new("child", 100.0, 50.0).with_position(10.0, 20.0);
        assert!(App::native_window_options_have_screen_position(&options));
    }

    #[test]
    fn desktop_wheel_dispatch_does_not_short_circuit_after_cursor_update() {
        let source = include_str!("desktop.rs");
        let short_circuit = ["cursor_dirty ", "|| app.pointer_scrolled"].concat();

        assert!(
            !source.contains(&short_circuit),
            "desktop wheel dispatch must always call pointer_scrolled after updating hover cursor"
        );
    }

    #[test]
    fn desktop_input_prefers_winit_cursor_before_x11_global_probe() {
        let source = include_str!("desktop.rs");

        assert!(
            source.contains("self.last_cursor_position.is_none()\n                    && Self::refresh_primary_cursor_from_platform_pointer"),
            "primary pointer-button and wheel handling must not overwrite a winit client cursor with X11 root-pointer coordinates"
        );
        assert!(
            source.contains("if native.last_cursor_position.is_none() {\n                    Self::refresh_native_cursor_from_platform_pointer"),
            "native pointer-button and wheel handling must not overwrite a winit client cursor with X11 root-pointer coordinates"
        );
        assert!(
            source.contains("if self.last_cursor_position.is_some() {\n            return false;\n        }\n\n        let Some(local) = native_window_local_pointer_physical"),
            "active primary gestures must not synthesize drag moves from X11 root-pointer coordinates after a winit client cursor is known"
        );
    }

    #[test]
    fn desktop_robot_idle_wait_considers_update_only_work() {
        let source = include_str!("desktop.rs");

        assert!(
            source.contains("let needs_update = frame_schedule.needs_update;"),
            "robot wait_for_idle must read update-only scheduler state"
        );
        assert!(
            source.contains("if !needs_frame && !waiting_for_present"),
            "robot wait_for_idle must not finish while update-only work is pending"
        );
    }

    #[test]
    fn desktop_robot_idle_wait_allows_frame_only_loops() {
        let source = include_str!("desktop.rs");

        assert!(
            source.contains("let frame_only = needs_frame && !needs_update && !waiting_for_present;"),
            "robot wait_for_idle must not block on frame-only renderer or animation loops after pending UI work has drained"
        );
    }

    #[test]
    fn visible_native_window_does_not_schedule_idle_position_poll() {
        assert!(
            !native_window_position_poll_needed(true, false, false),
            "idle visible native windows must not wake the event loop at 60Hz"
        );
        assert!(
            native_window_position_poll_needed(true, false, true),
            "pending programmatic native-window positions still need a bounded settle poll"
        );
        assert!(!native_window_position_poll_needed(false, false, true));
        assert!(!native_window_position_poll_needed(true, true, true));
    }

    #[test]
    fn native_window_position_only_options_change_does_not_require_content_sync() {
        let previous =
            NativeWindowOptions::borderless("Winamp", 275.0, 116.0).with_position(10.0, 20.0);
        let moved =
            NativeWindowOptions::borderless("Winamp", 275.0, 116.0).with_position(42.0, 48.0);
        let resized =
            NativeWindowOptions::borderless("Winamp", 280.0, 116.0).with_position(42.0, 48.0);
        let retitled =
            NativeWindowOptions::borderless("Player", 275.0, 116.0).with_position(42.0, 48.0);

        assert!(native_window_options_change_is_position_only(
            &previous, &moved
        ));
        assert!(!native_window_options_change_is_position_only(
            &previous, &previous
        ));
        assert!(!native_window_options_change_is_position_only(
            &previous, &resized
        ));
        assert!(!native_window_options_change_is_position_only(
            &previous, &retitled
        ));
    }

    #[test]
    fn desktop_robot_queries_do_not_drive_animation_only_frames() {
        let source = include_str!("desktop.rs");

        assert!(
            source.contains("if !robot_query_should_drain_frame(app)")
                && source.contains("fn robot_query_should_drain_frame"),
            "robot semantic/screenshot queries must use the query drain predicate"
        );
        assert!(
            source.contains("app.needs_redraw()"),
            "robot query drains should apply visible redraw work without advancing update-only frames"
        );
    }

    #[test]
    fn no_vsync_redraw_chain_requires_uncapped_dirty_frame() {
        let vsync_interval = std::time::Duration::from_nanos(16_666_667);

        assert!(should_chain_no_vsync_redraw(
            frame_interval_for_mode(FramePacingMode::NoVsync, vsync_interval),
            true
        ));
        assert!(!should_chain_no_vsync_redraw(
            frame_interval_for_mode(FramePacingMode::NoVsync, vsync_interval),
            false
        ));
        assert!(!should_chain_no_vsync_redraw(
            frame_interval_for_mode(FramePacingMode::Hard120, vsync_interval),
            true
        ));
        assert!(!should_chain_no_vsync_redraw(
            frame_interval_for_mode(FramePacingMode::Vsync, vsync_interval),
            true
        ));
    }

    #[test]
    fn native_window_host_position_needs_resolution() {
        let options =
            NativeWindowOptions::new("child", 100.0, 50.0).with_host_window_position(10.0, 20.0);
        assert_eq!(
            options.position_origin,
            NativeWindowPositionOrigin::HostWindow
        );
        assert!(!App::native_window_options_have_screen_position(&options));
    }

    #[test]
    fn native_window_initial_position_prefers_declaration_over_early_os_position() {
        let options = NativeWindowOptions::new("child", 100.0, 50.0).with_position(10.0, 20.0);

        assert_eq!(
            App::initial_native_window_position(&options, Some((0.0, 0.0))),
            Some((10.0, 20.0))
        );
    }

    #[test]
    fn native_window_initial_position_uses_os_position_without_declaration() {
        let options = NativeWindowOptions::new("child", 100.0, 50.0);

        assert_eq!(
            App::initial_native_window_position(&options, Some((30.0, 40.0))),
            Some((30.0, 40.0))
        );
    }

    #[test]
    fn native_window_group_bounds_move_from_virtual_gap_to_nearest_monitor() {
        let monitors = [
            DesktopRect {
                x: 0.0,
                y: 630.0,
                width: 1420.0,
                height: 800.0,
            },
            DesktopRect {
                x: 1920.0,
                y: 0.0,
                width: 3840.0,
                height: 2160.0,
            },
        ];
        let group_bounds = DesktopRect {
            x: 140.0,
            y: 120.0,
            width: 550.0,
            height: 319.0,
        };

        let monitor = nearest_monitor_to_rect(&monitors, group_bounds).expect("nearest monitor");
        let delta = clamp_rect_to_monitor_delta(group_bounds, monitor, 32.0);

        assert_eq!(monitor, monitors[0]);
        assert_eq!(delta, cranpose_ui::Point::new(0.0, 542.0));
    }

    #[test]
    fn native_window_group_bounds_skip_correction_without_monitors() {
        let group_bounds = DesktopRect {
            x: 140.0,
            y: 120.0,
            width: 550.0,
            height: 319.0,
        };

        assert_eq!(nearest_monitor_to_rect(&[], group_bounds), None);
    }

    #[test]
    fn native_window_group_bounds_preserve_visible_position() {
        let monitor = DesktopRect {
            x: 0.0,
            y: 630.0,
            width: 1420.0,
            height: 800.0,
        };
        let group_bounds = DesktopRect {
            x: 140.0,
            y: 700.0,
            width: 550.0,
            height: 319.0,
        };

        let delta = clamp_rect_to_monitor_delta(group_bounds, monitor, 32.0);

        assert_eq!(delta, cranpose_ui::Point::new(0.0, 0.0));
    }

    #[test]
    fn visible_primary_surface_drives_redraw_updates() {
        assert!(primary_surface_redraw_drives_app(true, false));
        assert!(!primary_surface_redraw_drives_app(false, false));
        assert!(!primary_surface_redraw_drives_app(true, true));
    }

    #[test]
    fn hidden_primary_frame_waker_uses_event_loop_proxy() {
        assert!(!primary_frame_waker_uses_event_proxy(true, false));
        assert!(primary_frame_waker_uses_event_proxy(false, false));
        assert!(primary_frame_waker_uses_event_proxy(true, true));
    }

    #[test]
    fn visible_primary_launch_requests_initial_redraw() {
        assert!(primary_launch_requires_initial_redraw(true, false));
        assert!(!primary_launch_requires_initial_redraw(false, false));
        assert!(!primary_launch_requires_initial_redraw(true, true));
    }

    #[test]
    fn surface_reconfigure_requests_replacement_frame_for_visible_surface() {
        assert!(surface_reconfigure_requires_redraw(1, 1));
        assert!(surface_reconfigure_requires_redraw(1920, 1080));
        assert!(!surface_reconfigure_requires_redraw(0, 1080));
        assert!(!surface_reconfigure_requires_redraw(1920, 0));
    }

    #[test]
    fn initial_present_self_heals_when_redraw_request_is_dropped() {
        // A static initial UI owes the first present but has no redraw in
        // flight (the one from `resumed` was dropped): re-request it.
        assert!(initial_present_redraw_needed(true, false));
        // A request is already pending, so do not pile on another.
        assert!(!initial_present_redraw_needed(true, true));
        // Once the first frame has presented the flag clears: never re-request.
        assert!(!initial_present_redraw_needed(false, false));
        assert!(!initial_present_redraw_needed(false, true));
    }

    #[test]
    fn hidden_primary_declaration_host_updates_without_redraw_event() {
        assert!(primary_declaration_host_needs_direct_update(
            false, false, true, false
        ));
        assert!(primary_declaration_host_needs_direct_update(
            true, true, true, false
        ));
        assert!(!primary_declaration_host_needs_direct_update(
            true, false, true, false
        ));
        assert!(!primary_declaration_host_needs_direct_update(
            false, false, true, true
        ));
        assert!(!primary_declaration_host_needs_direct_update(
            false, false, false, false
        ));
    }

    #[cfg(feature = "robot")]
    #[test]
    fn visible_robot_wait_requires_present_after_visual_mutation() {
        assert_eq!(robot_visible_present_target(true, false, true, 7), Some(8));
        assert_eq!(robot_visible_present_target(true, false, false, 7), None);
        assert_eq!(robot_visible_present_target(true, true, true, 7), None);
        assert_eq!(robot_visible_present_target(false, false, true, 7), None);
    }

    #[cfg(feature = "robot")]
    #[test]
    fn visible_robot_frame_pump_waits_for_presented_frames() {
        assert_eq!(
            robot_visible_pump_present_target(true, false, 3, 11),
            Some(14)
        );
        assert_eq!(robot_visible_pump_present_target(true, false, 0, 11), None);
        assert_eq!(robot_visible_pump_present_target(true, true, 3, 11), None);
        assert_eq!(robot_visible_pump_present_target(false, false, 3, 11), None);
    }

    #[test]
    fn pointer_button_input_requests_uncapped_frame_when_handled() {
        let request = pointer_button_frame_request(true);

        assert!(request.request_redraw);
        assert!(request.reset_frame_cap);
        assert_eq!(
            pointer_button_frame_request(false),
            super::PointerButtonFrameRequest {
                request_redraw: false,
                reset_frame_cap: false,
            }
        );
    }

    #[test]
    fn scroll_input_requests_uncapped_frame_when_handled() {
        let request = scroll_frame_request(true);

        assert!(request.request_redraw);
        assert!(request.reset_frame_cap);
        assert_eq!(
            scroll_frame_request(false),
            super::PointerButtonFrameRequest {
                request_redraw: false,
                reset_frame_cap: false,
            }
        );
    }

    #[test]
    fn pending_native_window_positions_acknowledge_stale_programmatic_moves() {
        let mut pending = PendingNativeWindowPositions::default();
        pending.push((100.0, 200.0));
        pending.push((140.0, 230.0));

        assert!(pending.acknowledge((100.0, 200.0)));
        assert!(pending.acknowledge((140.0, 230.0)));
        assert!(!pending.acknowledge((190.0, 260.0)));
    }

    #[test]
    fn pending_native_window_positions_match_fractional_window_manager_rounding() {
        let mut pending = PendingNativeWindowPositions::default();
        pending.push((100.4, 200.4));

        assert!(pending.acknowledge((101.0, 201.0)));
        assert!(!pending.acknowledge((101.0, 201.0)));
    }

    #[test]
    fn pending_native_window_positions_can_be_cleared_after_external_move() {
        let mut pending = PendingNativeWindowPositions::default();
        pending.push((100.0, 200.0));
        pending.push((140.0, 240.0));

        pending.clear();

        assert!(!pending.acknowledge((100.0, 200.0)));
        assert!(!pending.acknowledge((140.0, 240.0)));
    }

    #[test]
    fn pending_native_window_positions_report_unacknowledged_programmatic_moves() {
        let mut pending = PendingNativeWindowPositions::default();

        assert!(!pending.has_pending());
        pending.push((100.0, 200.0));
        assert!(pending.has_pending());
        assert!(pending.acknowledge((100.0, 200.0)));
        assert!(!pending.has_pending());
    }

    #[test]
    fn pending_native_window_positions_acknowledge_known_graph_position() {
        let mut pending = PendingNativeWindowPositions::default();
        pending.push((100.0, 200.0));
        pending.push((120.0, 220.0));

        assert_eq!(
            pending.acknowledge_or_matches_known((140.0, 240.0), Some((140.0, 240.0))),
            NativeWindowPositionObservation::Current
        );
        assert!(!pending.has_pending());
    }

    #[test]
    fn pending_native_window_positions_reject_unknown_external_move() {
        let mut pending = PendingNativeWindowPositions::default();
        pending.push((100.0, 200.0));

        assert_eq!(
            pending.acknowledge_or_matches_known((140.0, 240.0), Some((120.0, 220.0))),
            NativeWindowPositionObservation::External
        );
        assert!(pending.has_pending());
    }

    #[test]
    fn pending_native_window_positions_distinguish_superseded_programmatic_moves() {
        let mut pending = PendingNativeWindowPositions::default();
        pending.push((100.0, 200.0));
        pending.push((120.0, 220.0));
        pending.push((140.0, 240.0));

        assert_eq!(
            pending.acknowledge_or_matches_known((100.0, 200.0), Some((140.0, 240.0))),
            NativeWindowPositionObservation::Superseded
        );
        assert!(pending.has_pending());
    }

    #[test]
    fn native_window_graph_position_keeps_cache_first_for_programmatic_moves() {
        let position = native_window_graph_position(
            None,
            Some((100.0, 200.0)),
            Some((140.0, 240.0)),
            Some((160.0, 260.0)),
            NativeWindowGraphPositionSource::CachedThenCurrent,
        );

        assert_eq!(position, Some(cranpose_ui::Point::new(100.0, 200.0)));
    }

    #[test]
    fn native_window_graph_position_uses_current_position_for_external_moves() {
        let position = native_window_graph_position(
            None,
            Some((100.0, 200.0)),
            Some((140.0, 240.0)),
            Some((160.0, 260.0)),
            NativeWindowGraphPositionSource::CurrentThenCached,
        );

        assert_eq!(position, Some(cranpose_ui::Point::new(140.0, 240.0)));
    }

    #[test]
    fn native_window_graph_position_override_wins_over_position_source() {
        let position = native_window_graph_position(
            Some(cranpose_ui::Point::new(80.0, 90.0)),
            Some((100.0, 200.0)),
            Some((140.0, 240.0)),
            Some((160.0, 260.0)),
            NativeWindowGraphPositionSource::CurrentThenCached,
        );

        assert_eq!(position, Some(cranpose_ui::Point::new(80.0, 90.0)));
    }

    #[test]
    fn native_window_polling_drag_target_is_anchored_to_drag_start() {
        let session = NativeWindowPollingDragSession::new(
            PhysicalPosition::new(100.0, 50.0),
            PhysicalPosition::new(300, 200),
            Instant::now(),
        );

        assert_eq!(
            session.target_for_pointer(PhysicalPosition::new(112.0, 57.0)),
            PhysicalPosition::new(312, 207)
        );
        assert_eq!(
            session.target_for_pointer(PhysicalPosition::new(120.0, 50.0)),
            PhysicalPosition::new(320, 200)
        );
    }

    #[test]
    fn native_window_polling_drag_target_does_not_accumulate_window_manager_lag() {
        let session = NativeWindowPollingDragSession::new(
            PhysicalPosition::new(100.0, 50.0),
            PhysicalPosition::new(300, 200),
            Instant::now(),
        );

        let first_target = session.target_for_pointer(PhysicalPosition::new(112.0, 50.0));
        let second_target = session.target_for_pointer(PhysicalPosition::new(120.0, 50.0));

        assert_eq!(first_target, PhysicalPosition::new(312, 200));
        assert_eq!(
            second_target,
            PhysicalPosition::new(320, 200),
            "the target must be based on the drag start, not on the last reported window position"
        );
    }

    #[test]
    fn recovered_native_window_drag_prefers_delivered_event_pointer() {
        let event_pointer = PhysicalPosition::new(100.0, 50.0);
        let global_pointer = NativeWindowPointerState {
            position: PhysicalPosition::new(112.0, 57.0),
            primary_down: true,
        };

        assert_eq!(
            recovered_native_window_drag_start_pointer(Some(event_pointer), Some(global_pointer)),
            Some(event_pointer)
        );
        assert_eq!(
            recovered_native_window_drag_start_pointer(None, Some(global_pointer)),
            Some(global_pointer.position)
        );
    }

    #[test]
    fn primary_pointer_move_recovers_missed_x11_press_once() {
        let global_pointer = Some(NativeWindowPointerState {
            position: PhysicalPosition::new(112.0, 57.0),
            primary_down: true,
        });

        assert!(primary_pointer_move_should_recover_press(
            false,
            false,
            global_pointer,
            true
        ));
        assert!(!primary_pointer_move_should_recover_press(
            true,
            false,
            global_pointer,
            true
        ));
        assert!(!primary_pointer_move_should_recover_press(
            false,
            true,
            global_pointer,
            true
        ));
        assert!(!primary_pointer_move_should_recover_press(
            false,
            false,
            global_pointer,
            false
        ));
        assert!(!primary_pointer_move_should_recover_press(
            false,
            false,
            Some(NativeWindowPointerState {
                position: PhysicalPosition::new(112.0, 57.0),
                primary_down: false,
            }),
            true
        ));
    }

    #[test]
    fn native_window_drag_sessions_finish_on_global_release() {
        let now = Instant::now();

        assert!(
            NativeWindowDragSession::Polling(NativeWindowPollingDragSession::new(
                PhysicalPosition::new(100.0, 50.0),
                PhysicalPosition::new(300, 200),
                now,
            ))
            .finishes_on_global_pointer_release()
        );
        assert!(NativeWindowDragSession::platform(now).finishes_on_global_pointer_release());
    }

    #[test]
    fn native_window_polling_drag_does_not_use_moved_events_as_targets() {
        let now = Instant::now();

        assert!(
            !NativeWindowDragSession::Polling(NativeWindowPollingDragSession::new(
                PhysicalPosition::new(100.0, 50.0),
                PhysicalPosition::new(300, 200),
                now,
            ))
            .uses_moved_events_as_drag_target()
        );
        assert!(NativeWindowDragSession::platform(now).uses_moved_events_as_drag_target());
    }

    #[test]
    fn inferred_native_drag_requires_pointer_over_surface() {
        let outer = PhysicalPosition::new(300, 200);
        let surface = PhysicalPosition::new(8, 28);
        let size = PhysicalSize::new(120, 60);

        assert!(physical_surface_rect_contains_pointer(
            outer,
            surface,
            size,
            PhysicalPosition::new(320.0, 240.0)
        ));
        assert!(!physical_surface_rect_contains_pointer(
            outer,
            surface,
            size,
            PhysicalPosition::new(299.0, 240.0)
        ));
    }

    #[cfg(feature = "robot")]
    #[test]
    fn robot_controller_tracks_synthetic_primary_button_lifetime() {
        let (mut controller, _robot) = RobotController::new();

        assert!(!controller.synthetic_primary_down());
        controller.begin_synthetic_primary_gesture();
        assert!(controller.synthetic_primary_down());
        controller.end_synthetic_primary_gesture();
        assert!(!controller.synthetic_primary_down());
    }

    #[cfg(feature = "robot")]
    #[test]
    fn robot_screenshot_prefers_logical_viewport_size() {
        let resolved = resolve_robot_screenshot_params((1600, 1200), Some((800.0, 600.0)));
        assert_eq!(resolved, (800, 600, 1.0));
    }

    #[cfg(feature = "robot")]
    #[test]
    fn robot_screenshot_uses_ceil_on_fractional_logical_size() {
        let resolved = resolve_robot_screenshot_params((0, 0), Some((801.2, 601.3)));
        assert_eq!(resolved, (802, 602, 1.0));
    }

    #[test]
    fn headless_primary_viewport_uses_requested_launcher_size() {
        let settings = AppSettings {
            initial_width: 1600,
            initial_height: 900,
            headless: true,
            ..AppSettings::default()
        };

        let viewport = primary_viewport_for_surface_size(&settings, 1601, 901, 1.0);

        assert_eq!(viewport, (1600.0, 900.0));
    }

    #[test]
    fn visible_primary_viewport_uses_actual_surface_size() {
        let settings = AppSettings {
            initial_width: 1600,
            initial_height: 900,
            headless: false,
            ..AppSettings::default()
        };

        let viewport = primary_viewport_for_surface_size(&settings, 1601, 901, 2.0);

        assert_eq!(viewport, (800.5, 450.5));
    }

    #[cfg(feature = "robot")]
    #[test]
    fn robot_screenshot_falls_back_to_physical_buffer_when_layout_is_missing() {
        let resolved = resolve_robot_screenshot_params((1600, 1200), None);
        assert_eq!(resolved, (1600, 1200, 1.0));
    }

    #[cfg(feature = "robot")]
    #[test]
    fn robot_screenshot_clamps_to_non_zero_target() {
        let resolved = resolve_robot_screenshot_params((0, 0), Some((10.0, 20.0)));
        assert_eq!(resolved, (10, 20, 1.0));
    }
}
