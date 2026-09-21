//! Desktop runtime for Compose applications.
//!
//! This module provides the desktop event loop implementation using winit.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet, VecDeque},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

#[cfg(feature = "robot")]
use cranpose_app_shell::PointerSource;
use cranpose_app_shell::{
    AppShell, FramePacingMode, FrameUpdateResult, RootId, SurfaceMut, default_root_key,
};
use cranpose_platform_desktop_winit::DesktopWinitPlatform;
use cranpose_render_wgpu::{WgpuRenderer, WgpuTextSystem};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize, Position},
    event::{ButtonSource, ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    window::{ResizeDirection, Window, WindowAttributes, WindowId as WinitWindowId, WindowLevel},
};

#[cfg(feature = "robot")]
use crate::robot::{
    Robot, RobotChannel, RobotCommand, RobotResponse, RobotScreenshot, RobotTimelineAction,
    char_to_key_code, extract_semantics, find_button_in_app, find_text_in_app,
    panic_payload_message, robot_key_code_and_text, robot_wait_for_idle_animation_loop_only,
};
use crate::{
    app_launcher::{AppSettings, LaunchError},
    desktop_input::{app_modifiers, dispatch_keyboard_input},
    native_window::{
        self, NativeWindowEvents, NativeWindowKey, NativeWindowOptions, NativeWindowPositionOrigin,
        NativeWindowRequest, WindowFocus, WindowResizeDirection, WindowState,
    },
    wgpu_surface::{
        SurfaceFrame, current_surface_texture, present_initial_placeholder_frame,
        present_initial_placeholder_frame_cleared_to, surface_present_required,
    },
    winit_pointer::{
        is_primary_pointer_button, pointer_source_from_button, pointer_source_from_winit,
    },
};

const NATIVE_WINDOW_DRAG_POLL_INTERVAL: Duration = Duration::from_millis(16);
const NATIVE_WINDOW_GLOBAL_POINTER_POLLED: bool = cfg!(all(
    target_os = "linux",
    not(target_arch = "wasm32"),
    feature = "desktop-x11"
));
const NATIVE_WINDOW_POSITION_POLL_INTERVAL: Duration = Duration::from_millis(16);
const NATIVE_WINDOW_POSITION_SETTLE_TIMEOUT: Duration = Duration::from_millis(36);
const NATIVE_WINDOW_POSITION_SETTLE_POLL: Duration = Duration::from_millis(1);
const NATIVE_WINDOW_PLACEMENT_MARGIN: f32 = 32.0;
#[cfg(feature = "robot")]
const ROBOT_IDLE_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(feature = "robot")]
const ROBOT_IDLE_MIN_ITERATIONS: u32 = 100;
#[cfg(feature = "robot")]
const ROBOT_IDLE_PRESENT_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(feature = "robot")]
const ROBOT_IDLE_STARVATION_CEILING: Duration = Duration::from_secs(60);
#[cfg(feature = "robot")]
const ROBOT_PUMP_FRAME_INTERVAL: Duration = Duration::from_nanos(16_666_667);
#[cfg(feature = "robot")]
const ROBOT_PRESENT_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_DESKTOP_FRAME_TELEMETRY_THRESHOLD_MS: f64 = 4.0;

#[cfg(feature = "robot")]
use std::sync::mpsc;

macro_rules! trace_when {
    ($enabled:expr, $sink:ident, $($arg:tt)*) => {
        if $enabled {
            $sink(format_args!($($arg)*));
        }
    };
}

macro_rules! trace_native_window {
    ($($arg:tt)*) => {
        trace_when!(native_window_trace_enabled(), print_native_window_trace, $($arg)*)
    };
}

macro_rules! trace_native_window_timing {
    ($($arg:tt)*) => {
        trace_when!(native_window_timing_enabled(), print_native_window_timing, $($arg)*)
    };
}

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
fn robot_tree_response(app: &mut AppShell<WgpuRenderer>, command: &RobotCommand) -> RobotResponse {
    match command {
        RobotCommand::GetInspectorState => {
            RobotResponse::InspectorState(Box::new(app.inspector_state().clone()))
        }
        RobotCommand::GetSpokenTree => {
            RobotResponse::SpokenTree(crate::accessibility::spoken_tree(app))
        }
        RobotCommand::AuditAccessibility => audit_response(app),
        _ => RobotResponse::Semantics(extract_semantics(app)),
    }
}

#[cfg(feature = "robot")]
fn audit_response(app: &mut AppShell<WgpuRenderer>) -> RobotResponse {
    match cranpose_app_shell::placed_semantics::placed_semantics_from_shell(app) {
        Some(placed) => RobotResponse::AccessibilityIssues(
            cranpose_app_shell::accessibility_audit::audit_accessibility(&placed)
                .iter()
                .map(ToString::to_string)
                .collect(),
        ),
        None => RobotResponse::Error(
            "the app has no laid out semantics tree yet; wait for a frame first".to_string(),
        ),
    }
}

#[cfg(feature = "robot")]
fn pump_robot_frame(
    app: &mut AppShell<WgpuRenderer>,
    registry: &Rc<native_window::NativeWindowRegistry>,
) -> FrameUpdateResult {
    let mut result = FrameUpdateResult::default();
    for _ in 0..3 {
        if !robot_query_should_drain_frame(app) {
            break;
        }
        let frame_result = update_app_with_native_window_registry(app, registry);
        result.visual_changed |= frame_result.visual_changed;
        result.structure_changed |= frame_result.structure_changed;
    }
    result
}

#[cfg(feature = "robot")]
fn robot_query_should_drain_frame(app: &AppShell<WgpuRenderer>) -> bool {
    app.needs_redraw()
}

#[cfg(feature = "robot")]
fn robot_query_visual_dirty(update_result: FrameUpdateResult, needs_redraw: bool) -> bool {
    update_result.visual_changed || needs_redraw
}

#[cfg(feature = "robot")]
struct RobotController {
    rx: mpsc::Receiver<RobotCommand>,
    tx: mpsc::Sender<RobotResponse>,
    pending_command: Option<RobotCommand>,
    waiting_for_idle: bool,
    idle_started_at: Option<Instant>,
    idle_iterations: u32,
    idle_structure_clean_frames: u32,
    idle_present_blocked_since: Option<Instant>,
    waiting_for_present_generation: Option<u64>,
    waiting_for_pump_present_generation: Option<u64>,
    pump_present_started_at: Option<Instant>,
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IdleWaitTimeout {
    AppNotConverging,
    SurfaceNotPresenting,
    HostStarved,
}

#[cfg(feature = "robot")]
impl RobotController {
    fn new(wake_event_loop: impl Fn() + Send + Sync + 'static) -> (Self, Robot) {
        let (channel, robot) = RobotChannel::new(wake_event_loop);
        let RobotChannel { rx, tx } = channel;

        let controller = RobotController {
            rx,
            tx,
            pending_command: None,
            waiting_for_idle: false,
            idle_started_at: None,
            idle_iterations: 0,
            idle_structure_clean_frames: 0,
            idle_present_blocked_since: None,
            waiting_for_present_generation: None,
            waiting_for_pump_present_generation: None,
            pump_present_started_at: None,
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
        self.start_idle_wait_at(Instant::now());
    }

    fn start_idle_wait_at(&mut self, started_at: Instant) {
        self.waiting_for_idle = true;
        self.idle_started_at = Some(started_at);
        self.idle_iterations = 0;
        self.idle_structure_clean_frames = 0;
        self.idle_present_blocked_since = None;
    }

    fn observe_idle_present_block(&mut self, present_is_sole_blocker: bool, now: Instant) {
        if !present_is_sole_blocker {
            self.idle_present_blocked_since = None;
        } else if self.idle_present_blocked_since.is_none() {
            self.idle_present_blocked_since = Some(now);
        }
    }

    fn idle_wait_timeout(&self, now: Instant) -> Option<IdleWaitTimeout> {
        let started_at = self.idle_started_at?;
        if let Some(blocked_since) = self.idle_present_blocked_since {
            let blocked_for = now.saturating_duration_since(blocked_since);
            return (blocked_for >= ROBOT_IDLE_PRESENT_TIMEOUT)
                .then_some(IdleWaitTimeout::SurfaceNotPresenting);
        }
        let elapsed = now.saturating_duration_since(started_at);
        if elapsed >= ROBOT_IDLE_TIMEOUT && self.idle_iterations >= ROBOT_IDLE_MIN_ITERATIONS {
            return Some(IdleWaitTimeout::AppNotConverging);
        }
        if elapsed >= ROBOT_IDLE_STARVATION_CEILING {
            return Some(IdleWaitTimeout::HostStarved);
        }
        None
    }

    fn begin_pump_present_wait(&mut self, target_generation: u64) {
        self.begin_pump_present_wait_at(target_generation, Instant::now());
    }

    fn begin_pump_present_wait_at(&mut self, target_generation: u64, started_at: Instant) {
        self.waiting_for_pump_present_generation = Some(target_generation);
        self.pump_present_started_at = Some(started_at);
    }

    fn finish_pump_present_wait(&mut self) {
        self.waiting_for_pump_present_generation = None;
        self.pump_present_started_at = None;
    }

    fn pump_present_wait_timed_out(&self, now: Instant) -> bool {
        self.pump_present_started_at.is_some_and(|started_at| {
            now.saturating_duration_since(started_at) >= ROBOT_PRESENT_WAIT_TIMEOUT
        })
    }

    fn stage_pending_command(&mut self) -> bool {
        if self.pending_command.is_none() {
            self.pending_command = self.rx.try_recv().ok();
        }
        self.pending_command.is_some()
    }

    fn next_command(&mut self) -> Option<RobotCommand> {
        self.pending_command
            .take()
            .or_else(|| self.rx.try_recv().ok())
    }

    fn finish_idle_wait(&mut self) {
        self.waiting_for_idle = false;
        self.idle_started_at = None;
        self.waiting_for_present_generation = None;
        self.idle_structure_clean_frames = 0;
        self.idle_present_blocked_since = None;
    }

    fn awaiting_progress(&mut self) -> bool {
        self.waiting_for_idle
            || self.waiting_for_present_generation.is_some()
            || self.waiting_for_pump_present_generation.is_some()
            || self.scroll_sequence.is_some()
            || self.synthetic_primary_down
            || self.stage_pending_command()
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
    window: Arc<dyn Window>,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    surface_caps: wgpu::SurfaceCapabilities,
    surface_dirty: bool,
    root: native_window::NativeWindowRootHandle,
    platform: DesktopWinitPlatform,
    last_cursor_position: Option<(f32, f32)>,
    last_cursor_physical_position: Option<PhysicalPosition<f64>>,
    last_frame_start_time: Option<Instant>,
    vsync_interval: Duration,
    pending_outer_positions: PendingNativeWindowPositions,
    active_drag: Option<NativeWindowPollingDragSession>,
    held_press: Option<PhysicalPosition<f64>>,
}

struct NativeWindowShell {
    request: NativeWindowRequest,
    window: Arc<dyn Window>,
    create_started: Instant,
}

#[derive(Default)]
struct NativeWindowEventSettlement {
    sync_after_event: bool,
    drag_move: Option<(NativeWindowKey, cranpose_ui::Point)>,
    finish_drag: bool,
}

impl NativeWindowEventSettlement {
    fn also(mut self, other: Self) -> Self {
        self.sync_after_event |= other.sync_after_event;
        self.drag_move = self.drag_move.or(other.drag_move);
        self.finish_drag |= other.finish_drag;
        self
    }
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum NativeWindowPositionApplyMode {
    #[default]
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
    fn frame_interval(&self, mode: FramePacingMode) -> Option<Duration> {
        frame_interval_for_mode(mode, self.vsync_interval)
    }

    fn root_id(&self) -> RootId {
        RootId::Window(self.key.raw())
    }
}

fn native_surface<'a>(
    app: &'a mut AppShell<WgpuRenderer>,
    native: &NativeWindowSurface,
) -> Option<SurfaceMut<'a, WgpuRenderer>> {
    app.surface(native.root_id())
}

struct App {
    settings: AppSettings,
    platform_env: Rc<crate::platform_env::PlatformEnvironment>,
    content: Option<Box<dyn FnMut()>>,
    window: Option<Arc<dyn Window>>,
    surface: Option<wgpu::Surface<'static>>,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    surface_caps: Option<wgpu::SurfaceCapabilities>,
    app: Option<AppShell<WgpuRenderer>>,
    accessibility: Option<crate::desktop_accessibility::DesktopAccessibilityBridge>,
    platform: Option<DesktopWinitPlatform>,
    gpu_context: Option<DesktopGpuContext>,
    native_windows: HashMap<WinitWindowId, NativeWindowSurface>,
    native_window_registry: Rc<native_window::NativeWindowRegistry>,
    native_window_ids: HashMap<NativeWindowKey, WinitWindowId>,
    native_window_positions: HashMap<NativeWindowKey, (f32, f32)>,
    closed_native_windows: HashSet<NativeWindowKey>,
    next_native_window_position_poll_at: Instant,
    native_window_platform_probe: NativeWindowPlatformProbe,
    native_global_primary_down: bool,
    primary_held_press: Option<PhysicalPosition<f64>>,
    handed_press: Option<HandedPress>,
    primary_wrap_size: Option<(u32, u32)>,
    cursors: crate::desktop_cursor::DesktopCursors,
    current_modifiers: winit::keyboard::ModifiersState,
    last_cursor_position: Option<(f32, f32)>,
    primary_shown: Arc<std::sync::atomic::AtomicBool>,
    #[cfg(feature = "robot")]
    robot_controller: Option<RobotController>,
    #[cfg(feature = "robot")]
    robot_app_hook: Option<Box<crate::RobotAppHook>>,
    recorder: Option<crate::recorder::InputRecorder>,
    launch_error: Rc<RefCell<Option<LaunchError>>>,
    event_proxy: EventLoopProxy,
    applied_frame_pacing_mode: FramePacingMode,
    last_frame_start_time: Option<Instant>,
    primary_redraw_pending: bool,
    primary_surface_dirty: bool,
    primary_initial_present_pending: bool,
    vsync_interval: Duration,
    exiting: bool,
    #[cfg(feature = "robot")]
    presented_frame_generation: u64,
    #[cfg(feature = "robot")]
    unpresentable_frames_since_present: u32,
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
        let recorder = settings
            .record_to
            .take()
            .map(crate::recorder::InputRecorder::new);
        #[cfg(feature = "robot")]
        let robot_app_hook = settings.robot_app_hook.take();
        let applied_frame_pacing_mode = settings.frame_pacing_mode;

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
            accessibility: None,
            platform: None,
            gpu_context: None,
            native_windows: HashMap::new(),
            native_window_registry: Rc::new(native_window::NativeWindowRegistry::default()),
            native_window_ids: HashMap::new(),
            native_window_positions: HashMap::new(),
            closed_native_windows: HashSet::new(),
            next_native_window_position_poll_at: Instant::now()
                + NATIVE_WINDOW_POSITION_POLL_INTERVAL,
            #[allow(clippy::default_constructed_unit_structs)]
            native_window_platform_probe: NativeWindowPlatformProbe::default(),
            native_global_primary_down: false,
            primary_held_press: None,
            handed_press: None,
            primary_wrap_size: None,
            cursors: crate::desktop_cursor::DesktopCursors::default(),
            current_modifiers: winit::keyboard::ModifiersState::empty(),
            last_cursor_position: None,
            primary_shown: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            #[cfg(feature = "robot")]
            robot_controller: None,
            #[cfg(feature = "robot")]
            robot_app_hook,
            recorder,
            launch_error,
            event_proxy,
            applied_frame_pacing_mode,
            last_frame_start_time: None,
            primary_redraw_pending: false,
            primary_surface_dirty: false,
            primary_initial_present_pending: false,
            vsync_interval: default_vsync_interval(),
            exiting: false,
            #[cfg(feature = "robot")]
            presented_frame_generation: 0,
            #[cfg(feature = "robot")]
            unpresentable_frames_since_present: 0,
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

    fn frame_pacing_mode(&self) -> FramePacingMode {
        self.app
            .as_ref()
            .map_or(self.settings.frame_pacing_mode, |app| {
                app.frame_pacing_mode()
            })
    }

    fn frame_interval(&self) -> Option<Duration> {
        frame_interval_for_mode(self.frame_pacing_mode(), self.vsync_interval)
    }

    fn sync_frame_pacing(&mut self) {
        let mode = self.frame_pacing_mode();
        if mode == self.applied_frame_pacing_mode {
            return;
        }
        self.applied_frame_pacing_mode = mode;

        if let Some(app) = &mut self.app {
            app.set_frame_pacing_mode(mode);
        }
        let Some(device) = self
            .gpu_context
            .as_ref()
            .map(|context| Arc::clone(&context.device))
        else {
            return;
        };
        if let (Some(surface), Some(surface_config)) = (&self.surface, &mut self.surface_config) {
            apply_frame_pacing_mode(
                &device,
                surface,
                surface_config,
                self.surface_caps.as_ref(),
                mode,
                self.vsync_interval,
            );
        }
        self.last_frame_start_time = None;
        if let Some(window) = self.window.clone() {
            request_redraw_once(&window, &mut self.primary_redraw_pending);
        }

        for native in self.native_windows.values_mut() {
            apply_frame_pacing_mode(
                &device,
                &native.surface,
                &mut native.surface_config,
                Some(&native.surface_caps),
                mode,
                native.vsync_interval,
            );
            native.last_frame_start_time = None;
            native.window.request_redraw();
        }
    }

    fn set_native_transparency(
        app: &mut AppShell<WgpuRenderer>,
        native: &mut NativeWindowSurface,
        transparent: bool,
    ) {
        native.window.set_transparent(transparent);
        if let Some(mut surface) = native_surface(app, native) {
            surface.renderer().set_transparent_background(transparent);
        }
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
        let frame_interval = self.frame_interval();
        let waiting_for_frame_cap = self
            .last_frame_start_time
            .and_then(|started_at| frame_interval.map(|interval| started_at + interval))
            .is_some_and(|deadline| deadline > Instant::now());
        let primary_visible = self.primary_visible();
        let direct_declaration_update = {
            let Some(app) = &mut self.app else {
                return;
            };
            let needs_redraw = app.frame_schedule().needs_frame;
            if !needs_redraw {
                return;
            }
            let direct_declaration_update = primary_declaration_host_needs_direct_update(
                primary_visible,
                self.settings.headless,
                needs_redraw,
                waiting_for_frame_cap,
            );
            if direct_declaration_update {
                trace_native_window!(
                    "primary declaration host proxy update visible={} headless={}",
                    primary_visible,
                    self.settings.headless
                );
                let update_result = update_declaration_host_frame(
                    app,
                    &registry,
                    &mut self.last_frame_start_time,
                    frame_interval,
                );
                #[cfg(feature = "robot")]
                if let Some(controller) = &mut self.robot_controller {
                    controller.record_idle_update_result(update_result);
                }
                #[cfg(not(feature = "robot"))]
                let _ = update_result;
            }
            direct_declaration_update
        };

        if direct_declaration_update {
            self.sync_native_windows(event_loop);
        } else if waiting_for_frame_cap {
        } else if let Some(window) = &self.window {
            request_redraw_once(window, &mut self.primary_redraw_pending);
        }
    }

    fn poll_primary_pointer_gesture(&mut self) -> bool {
        let active = self
            .app
            .as_ref()
            .is_some_and(AppShell::has_active_pointer_gesture);
        #[cfg(feature = "robot")]
        let synthetic_primary_down = self
            .robot_controller
            .as_ref()
            .is_some_and(RobotController::synthetic_primary_down);
        #[cfg(not(feature = "robot"))]
        let synthetic_primary_down = false;

        let action = primary_pointer_gesture_poll_action(
            active,
            synthetic_primary_down,
            native_window_global_pointer_state(&self.native_window_platform_probe),
        );

        let (Some(window), Some(platform), Some(app)) =
            (&self.window, &self.platform, self.app.as_mut())
        else {
            return false;
        };

        let pointer = match action {
            PrimaryPointerGesturePollAction::Inactive => return false,
            PrimaryPointerGesturePollAction::ReleaseAt(position) => {
                let event_time = app.realtime_pointer_event_time(None);
                let cursor_dirty = native_window_local_pointer_physical(
                    &self.native_window_platform_probe,
                    window,
                    position,
                )
                .is_some_and(|local| {
                    let logical = platform.pointer_position(local);
                    self.last_cursor_position = Some((logical.x, logical.y));
                    app.set_cursor_at_event_time(logical.x, logical.y, event_time)
                });
                let released = app.pointer_released_at_event_time(event_time);
                let handled = cursor_dirty || released;
                if handled {
                    self.last_frame_start_time = None;
                    request_redraw_once(window, &mut self.primary_redraw_pending);
                }
                return handled;
            }
            PrimaryPointerGesturePollAction::Pressed(pointer) => pointer,
        };

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
        app: &mut AppShell<WgpuRenderer>,
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
        let Some(mut surface) = native_surface(app, native) else {
            return false;
        };
        surface.set_screen_origin(native_window_surface_origin(platform_probe, &native.window));
        surface.set_cursor(logical.x, logical.y)
    }

    fn sync_pointer_icons(&mut self, event_loop: &dyn ActiveEventLoop) {
        let Some(app) = self.app.as_mut() else {
            return;
        };
        if let Some(window) = self.window.as_ref()
            && let Some(icon) = app.take_pointer_icon_change()
        {
            self.cursors.apply(event_loop, window, &icon);
        }
        for native in self.native_windows.values() {
            if let Some(icon) =
                native_surface(app, native).and_then(|surface| surface.take_pointer_icon_change())
            {
                self.cursors.apply(event_loop, &native.window, &icon);
            }
        }
    }

    fn let_the_first_window_take_focus(&self, native: &NativeWindowSurface) {
        if a_new_window_comes_up_key(
            self.settings.headless,
            native.options.visible,
            self.any_window_has_focus(),
            native.options.focus,
        ) {
            native.window.focus_window();
        }
    }

    fn any_window_has_focus(&self) -> bool {
        let primary_focused = self.primary_visible()
            && self
                .window
                .as_ref()
                .is_some_and(|window| window.has_focus());
        primary_focused
            || self
                .native_windows
                .values()
                .any(|open| open.window.has_focus())
    }

    fn primary_window_id(&self) -> Option<WinitWindowId> {
        self.window.as_ref().map(|window| window.id())
    }

    fn refresh_native_window(
        platform_probe: &NativeWindowPlatformProbe,
        registry: &Rc<native_window::NativeWindowRegistry>,
        headless: bool,
        app: &mut AppShell<WgpuRenderer>,
        native: &mut NativeWindowSurface,
        request: &NativeWindowRequest,
    ) {
        native.events = request.events.clone();
        native.state = request.state;
        native.root = Rc::clone(&request.root);
        let revision_changed = native.revision != request.revision;
        let options_changed = native.options != request.options;
        let position_only_options_change = options_changed
            && native_window_options_change_is_position_only(&native.options, &request.options);
        let resized = Self::apply_native_window_options(
            platform_probe,
            app,
            native,
            &request.options,
            headless,
        );
        if revision_changed {
            native.revision = request.revision;
            if position_only_options_change {
                trace_native_window!(
                    "sync update position key={:?} title={:?}",
                    native.key,
                    native.options.title
                );
            } else {
                trace_native_window!(
                    "sync update content key={:?} title={:?}",
                    native.key,
                    native.options.title
                );
                native.window.request_redraw();
            }
        } else if options_changed && request.options.visible {
            trace_native_window!(
                "sync update options key={:?} title={:?} visible={}",
                native.key,
                native.options.title,
                request.options.visible
            );
            native.window.request_redraw();
        }
        if resized {
            Self::present_before_the_desktop_composites(app, native, registry, "resize");
        }
    }

    fn present_before_the_desktop_composites(
        app: &mut AppShell<WgpuRenderer>,
        native: &mut NativeWindowSurface,
        registry: &Rc<native_window::NativeWindowRegistry>,
        why: &str,
    ) {
        let started = Instant::now();
        let presented = Self::redraw_native_window(app, native, registry);
        trace_native_window_timing!(
            "{} {} presented={presented} in {}ms",
            native.options.title,
            why,
            started.elapsed().as_millis()
        );
    }

    fn let_go_of_windows_no_longer_declared(
        &mut self,
        app: &mut AppShell<WgpuRenderer>,
        active_keys: &HashSet<NativeWindowKey>,
    ) {
        let stale_window_ids: Vec<WinitWindowId> = self
            .native_windows
            .iter()
            .filter_map(|(window_id, native)| {
                (!active_keys.contains(&native.key)).then_some(*window_id)
            })
            .collect();
        for window_id in stale_window_ids {
            if let Some(native) = self.native_windows.get(&window_id) {
                trace_native_window!(
                    "sync stale key={:?} title={:?} visible={} shown={:?}",
                    native.key,
                    native.options.title,
                    native.options.visible,
                    native.window.is_visible()
                );
                if let Some((x, y)) =
                    current_native_window_position(&self.native_window_platform_probe, native)
                {
                    self.native_window_positions.insert(native.key, (x, y));
                    notify_native_window_moved(&native.events, x, y);
                }
            }
            if let Some(native) = self.native_windows.get_mut(&window_id) {
                note_native_window_presented(native.state, false);
                native.state = None;
                if native.options.visible {
                    native.options.visible = false;
                    native.window.set_visible(false);
                    if let Some(mut surface) = native_surface(app, native) {
                        cancel_surface_input(&mut surface);
                    }
                }
            }
        }
    }

    fn sync_native_windows(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.gpu_context.is_none() {
            return;
        }
        let Some(mut app) = self.app.take() else {
            return;
        };
        self.sync_native_windows_with(&mut app, event_loop);
        self.app = Some(app);
    }

    fn sync_native_windows_with(
        &mut self,
        app: &mut AppShell<WgpuRenderer>,
        event_loop: &dyn ActiveEventLoop,
    ) {
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
        trace_native_window_timing!(
            "sync start requests={} existing={}",
            requests.len(),
            self.native_windows.len()
        );

        self.closed_native_windows
            .retain(|key| active_keys.contains(key));

        self.let_go_of_windows_no_longer_declared(app, &active_keys);

        let mut shown_again = Vec::new();
        let mut native_windows_to_create =
            self.refresh_native_windows_or_collect_new(app, requests, &mut shown_again);
        for window_id in shown_again {
            self.hand_held_press_to_new_window(app, window_id);
        }

        self.place_initial_native_windows_on_visible_monitors(
            event_loop,
            &mut native_windows_to_create,
        );

        let mut native_window_shells = Vec::with_capacity(native_windows_to_create.len());
        let anything_focused = self.any_window_has_focus();
        for request in native_windows_to_create {
            trace_native_window!(
                "sync create key={:?} title={:?} visible={}",
                request.key,
                request.options.title,
                request.options.visible
            );
            match Self::create_native_window_shell(
                event_loop,
                request,
                self.settings.headless,
                anything_focused,
            ) {
                Ok(shell) => native_window_shells.push(shell),
                Err(error) => {
                    self.abort_launch(event_loop, error);
                    return;
                }
            }
        }

        for shell in native_window_shells {
            match self.create_native_window(app, shell) {
                Ok(native) => {
                    let window_id = native.window.id();
                    self.let_the_first_window_take_focus(&native);
                    self.remember_native_window_position(&native);
                    self.native_window_ids.insert(native.key, window_id);
                    self.native_windows.insert(window_id, native);
                    self.hand_held_press_to_new_window(app, window_id);
                }
                Err(error) => {
                    self.abort_launch(event_loop, error);
                    return;
                }
            }
        }
        trace_native_window_timing!("sync done in {}ms", sync_started.elapsed().as_millis());
        if self.no_window_left_to_show() {
            event_loop.exit();
        }
    }

    fn refresh_native_windows_or_collect_new(
        &mut self,
        app: &mut AppShell<WgpuRenderer>,
        requests: Vec<NativeWindowRequest>,
        shown_again: &mut Vec<WinitWindowId>,
    ) -> Vec<NativeWindowRequest> {
        let mut native_windows_to_create = Vec::new();
        for request in requests {
            if self.closed_native_windows.contains(&request.key) {
                trace_native_window!(
                    "sync skip closed key={:?} title={:?}",
                    request.key,
                    request.options.title
                );
                continue;
            }

            let request = self.native_window_request_for_host(&request);
            if let Some(window_id) = self.native_window_ids.get(&request.key).copied() {
                if let Some(native) = self.native_windows.get_mut(&window_id) {
                    let was_hidden = !native.options.visible;
                    Self::refresh_native_window(
                        &self.native_window_platform_probe,
                        &self.native_window_registry,
                        self.settings.headless,
                        app,
                        native,
                        &request,
                    );
                    if window_shown_again_takes_a_held_press(was_hidden, native.options.visible) {
                        trace_native_window!(
                            "sync shown again key={:?} title={:?}",
                            native.key,
                            native.options.title
                        );
                        shown_again.push(window_id);
                    }
                    continue;
                }
                self.native_window_ids.remove(&request.key);
            }

            native_windows_to_create.push(request);
        }
        native_windows_to_create
    }

    fn primary_visible(&self) -> bool {
        self.primary_shown
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn sync_primary_visibility(&mut self) {
        let (Some(window), Some(app)) = (self.window.clone(), self.app.as_mut()) else {
            return;
        };
        let was_shown = self
            .primary_shown
            .load(std::sync::atomic::Ordering::Relaxed);
        show_primary_when_it_has_content(
            &self.primary_shown,
            app,
            window.as_ref(),
            self.settings.headless,
        );
        if !was_shown
            && self
                .primary_shown
                .load(std::sync::atomic::Ordering::Relaxed)
        {
            request_redraw_once(&window, &mut self.primary_redraw_pending);
        }
    }

    fn sync_primary_size(&mut self) {
        if self.primary_visible() {
            return;
        }
        let (Some(window), Some(app)) = (self.window.clone(), self.app.as_mut()) else {
            return;
        };
        if let Some(size) = wrap_primary_window_to_content(
            window.as_ref(),
            app,
            self.settings.primary_wraps_content,
            self.primary_wrap_size,
        ) {
            self.primary_wrap_size = Some(size);
        }
    }

    fn no_window_left_to_show(&self) -> bool {
        if self.primary_visible() || self.settings.headless {
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

        let mut groups: HashMap<NativeWindowKey, Vec<usize>> = HashMap::new();
        for (index, request) in requests.iter().enumerate() {
            if !request.options.visible
                || !Self::native_window_options_have_screen_position(&request.options)
            {
                continue;
            }
            groups.entry(request.key).or_default().push(index);
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
        tell_the_application_the_window_frame(native.state, &native.window);
        if let Some((x, y)) = Self::initial_native_window_position(
            &native.options,
            current_native_window_position(&self.native_window_platform_probe, native),
        ) {
            self.native_window_positions.insert(native.key, (x, y));
            if let Some(state) = native.state
                && state.position_non_reactive().is_none()
            {
                state.set_position(Some(cranpose_ui::Point::new(x, y)));
            }
            notify_native_window_moved(&native.events, x, y);
        }
    }

    fn initial_native_window_position(
        options: &NativeWindowOptions,
        current_position: Option<(f32, f32)>,
    ) -> Option<(f32, f32)> {
        if options.position_origin == NativeWindowPositionOrigin::Screen
            && let Some(position) = options.x.zip(options.y)
        {
            return Some(position);
        }

        current_position
    }

    fn create_native_window_shell(
        event_loop: &dyn ActiveEventLoop,
        request: NativeWindowRequest,
        headless: bool,
        anything_focused: bool,
    ) -> Result<NativeWindowShell, LaunchError> {
        let create_started = Instant::now();
        let options = &request.options;
        let attributes = native_window_attributes(
            options,
            headless,
            a_new_window_comes_up_key(headless, options.visible, anything_focused, options.focus),
        );

        let window: Arc<dyn Window> = event_loop
            .create_window(attributes)
            .map_err(LaunchError::WindowCreate)?
            .into();
        trace_native_window_timing!(
            "{} create_window {}ms",
            options.title,
            create_started.elapsed().as_millis()
        );
        Ok(NativeWindowShell {
            request,
            window,
            create_started,
        })
    }

    fn create_native_window(
        &self,
        app: &mut AppShell<WgpuRenderer>,
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
        trace_native_window_timing!(
            "{} create_surface {}ms",
            options.title,
            create_started.elapsed().as_millis()
        );
        let surface_caps = surface.get_capabilities(&context.adapter);
        let surface_format = select_surface_format(&surface_caps)?;
        let present_mode = desktop_present_mode(&surface_caps, self.frame_pacing_mode());
        log_desktop_present_mode(self.frame_pacing_mode(), present_mode, &surface_caps);
        let size = window.surface_size();
        let surface_config = surface_config_for_window(
            &surface_caps,
            surface_format,
            size.width.max(1),
            size.height.max(1),
            present_mode,
            options.transparent,
            desired_frame_latency(self.frame_pacing_mode(), monitor_refresh_interval(&window)),
        )?;
        surface.configure(&context.device, &surface_config);
        let placeholder_presented = present_initial_placeholder_frame_cleared_to(
            &surface,
            &context.device,
            &context.queue,
            surface_format,
            "native window initial present",
            cranpose_render_wgpu::frame_clear_color(options.transparent),
        );
        trace_native_window_timing!(
            "{} configure {}ms placeholder_presented={placeholder_presented} alpha={:?}",
            options.title,
            create_started.elapsed().as_millis(),
            surface_config.alpha_mode
        );

        let scale_factor = window.scale_factor();
        let mut renderer = wgpu_renderer_for_surface(
            context.text_system.clone(),
            Arc::clone(&context.device),
            Arc::clone(&context.queue),
            crate::surface_format::display_surface_view_format(surface_format),
            context.adapter_backend,
            context.adapter.get_downlevel_capabilities().flags,
            scale_factor,
        );
        renderer.set_transparent_background(options.transparent);
        trace_native_window_timing!(
            "{} renderer {}ms",
            options.title,
            create_started.elapsed().as_millis()
        );
        let viewport = (
            surface_config.width as f32 / scale_factor as f32,
            surface_config.height as f32 / scale_factor as f32,
        );
        let registry = Rc::clone(&self.native_window_registry);
        let root_id = request.key.raw();
        request
            .root
            .set_size(cranpose_ui::Size::new(viewport.0, viewport.1));
        app.add_window_surface(
            root_id,
            renderer,
            (surface_config.width, surface_config.height),
            viewport,
        );
        if let Some(mut surface) = app.surface(RootId::Window(root_id)) {
            DesktopTextInput::install(&mut surface, &window);
        }
        trace_native_window_timing!(
            "{} surface {}ms",
            options.title,
            create_started.elapsed().as_millis()
        );

        let mut platform = DesktopWinitPlatform::default();
        platform.set_scale_factor(scale_factor);
        window.request_redraw();
        trace_native_window_timing!(
            "{} create done {}ms",
            options.title,
            create_started.elapsed().as_millis()
        );

        let mut native = NativeWindowSurface {
            key: request.key,
            revision: request.revision,
            options: request.options.clone(),
            events: request.events.clone(),
            state: request.state,
            window,
            surface,
            surface_config,
            surface_caps,
            surface_dirty: true,
            root: Rc::clone(&request.root),
            platform,
            last_cursor_position: None,
            last_cursor_physical_position: None,
            last_frame_start_time: None,
            vsync_interval: default_vsync_interval(),
            pending_outer_positions: PendingNativeWindowPositions::default(),
            active_drag: None,
            held_press: None,
        };
        Self::present_before_the_desktop_composites(app, &mut native, &registry, "first frame");
        Ok(native)
    }

    fn apply_native_window_options(
        platform_probe: &NativeWindowPlatformProbe,
        app: &mut AppShell<WgpuRenderer>,
        native: &mut NativeWindowSurface,
        options: &NativeWindowOptions,
        headless: bool,
    ) -> bool {
        let mut resized = false;
        Self::apply_native_window_appearance(app, native, options);
        resized |=
            Self::apply_native_window_geometry(platform_probe, app, native, options, headless);
        native.options = options.clone();
        resized
    }

    fn apply_native_window_appearance(
        app: &mut AppShell<WgpuRenderer>,
        native: &mut NativeWindowSurface,
        options: &NativeWindowOptions,
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
            Self::set_native_transparency(app, native, options.transparent);
        }
        if native.options.shadow != options.shadow {
            set_native_window_shadow(&native.window, options.shadow);
        }
        if native.options.always_on_top != options.always_on_top {
            native
                .window
                .set_window_level(native_window_level(options.always_on_top));
        }
    }

    fn apply_native_window_geometry(
        platform_probe: &NativeWindowPlatformProbe,
        app: &mut AppShell<WgpuRenderer>,
        native: &mut NativeWindowSurface,
        options: &NativeWindowOptions,
        headless: bool,
    ) -> bool {
        let mut resized = false;
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
            note_native_window_presented(native.state, false);
        }
        if (native.options.x != options.x || native.options.y != options.y)
            && let (Some(x), Some(y)) = (options.x, options.y)
        {
            native.pending_outer_positions.push((x, y));
            let logical = LogicalPosition::new(x as f64, y as f64);
            let physical = logical.to_physical::<i32>(native.window.scale_factor());
            if !native_window_set_outer_position_physical(platform_probe, &native.window, physical)
            {
                native.window.set_outer_position(Position::Logical(logical));
            }
        }
        if (native.options.width != options.width || native.options.height != options.height)
            && let Some(size) = native.window.request_surface_size(
                LogicalSize::new(
                    options.width.max(1.0) as f64,
                    options.height.max(1.0) as f64,
                )
                .into(),
            )
        {
            Self::resize_native_surface(app, native, size.width, size.height);
            resized = true;
        }
        resized
    }

    fn resize_native_surface(
        app: &mut AppShell<WgpuRenderer>,
        native: &mut NativeWindowSurface,
        width: u32,
        height: u32,
    ) {
        if width == 0 || height == 0 {
            return;
        }
        let viewport = surface_logical_viewport_size(width, height, native.window.scale_factor());
        let Some(device) = app.renderer().try_device() else {
            log::error!("native surface resize skipped: GPU renderer is not initialized");
            return;
        };
        native.surface_config.width = width;
        native.surface_config.height = height;
        native.surface.configure(device, &native.surface_config);
        native
            .root
            .set_size(cranpose_ui::Size::new(viewport.0, viewport.1));
        if let Some(mut surface) = native_surface(app, native) {
            surface.set_buffer_size(width, height);
            surface.set_viewport(viewport.0, viewport.1);
        }
        native.surface_dirty = surface_reconfigure_requires_redraw(width, height);
    }

    fn apply_native_window_resize(
        app: &mut AppShell<WgpuRenderer>,
        native: &mut NativeWindowSurface,
        width: u32,
        height: u32,
    ) {
        let previous_state_size = native.state.map(|state| state.size_non_reactive());
        update_native_options_size(&mut native.options, &native.window, width, height);
        tell_the_application_the_window_frame(native.state, &native.window);
        notify_native_window_resized(&native.events, &native.window, width, height);
        sync_native_window_state_size(
            native.state,
            previous_state_size,
            &native.window,
            width,
            height,
        );
        Self::resize_native_surface(app, native, width, height);
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
        tell_the_application_the_window_moved(&native.events, native.state, position.0, position.1);
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
        tell_the_application_the_window_moved(&native.events, native.state, position.x, position.y);
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

    fn handle_native_primary_pressed(
        &mut self,
        app: &mut AppShell<WgpuRenderer>,
        native: &mut NativeWindowSurface,
    ) -> bool {
        let (handled, drag_requested) =
            Self::dispatch_native_primary_pressed(&self.native_window_platform_probe, app, native);
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
        app: &mut AppShell<WgpuRenderer>,
        native: &mut NativeWindowSurface,
    ) -> (bool, bool) {
        let Some(mut surface) = native_surface(app, native) else {
            return (false, false);
        };
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
        surface.set_screen_origin(native_window_surface_origin(platform_probe, &native.window));
        let handled =
            native_window::with_native_window_drag_handler(drag_handler, resize_handler, || {
                surface.pointer_pressed()
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
        trace_native_window!("drag requested key={:?}", native.key);
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
        let anchor =
            start_pointer_screen.or_else(|| native_window_press_anchor(platform_probe, native));
        if !Self::start_native_window_drag(platform_probe, native, anchor) {
            trace_native_window!("drag cancel key={:?} reason=start-failed", native.key);
        }
    }

    fn move_dragged_window(&mut self, key: NativeWindowKey, target: cranpose_ui::Point) -> bool {
        let Some(window_id) = self.native_window_ids.get(&key).copied() else {
            return false;
        };
        let Some(native) = self.native_windows.get_mut(&window_id) else {
            return false;
        };
        let Some(request) = Self::prepare_native_window_position_request(
            &self.native_window_platform_probe,
            native,
            target,
        ) else {
            return false;
        };
        self.native_window_positions
            .insert(key, (target.x, target.y));
        Self::apply_native_window_position_requests(
            &self.native_window_platform_probe,
            vec![request],
            NativeWindowPositionApplyMode::FlushOnly,
        );
        true
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
            native.active_drag = Some(session);
            trace_native_window!(
                "drag start polling key={:?} pointer=({:.1},{:.1}) outer=({},{})",
                native.key,
                pointer.x,
                pointer.y,
                window_outer.x,
                window_outer.y
            );
            return true;
        }

        false
    }

    fn native_window_polling_drag_session(
        platform_probe: &NativeWindowPlatformProbe,
        native: &NativeWindowSurface,
        now: Instant,
        start_pointer_screen: Option<PhysicalPosition<f64>>,
    ) -> Option<NativeWindowPollingDragSession> {
        let pointer = native_window_polling_drag_pointer(
            native_window_global_pointer_state(platform_probe),
            start_pointer_screen,
        )?;
        let window_outer = current_native_window_physical_position(platform_probe, &native.window)?;
        Some(NativeWindowPollingDragSession::new(
            pointer,
            window_outer,
            now,
        ))
    }

    fn poll_native_window_global_primary_press(&mut self) -> bool {
        let Some(mut app) = self.app.take() else {
            return false;
        };
        let handled = self.poll_native_window_global_primary_press_with(&mut app);
        self.app = Some(app);
        handled
    }

    fn poll_native_window_global_primary_press_with(
        &mut self,
        app: &mut AppShell<WgpuRenderer>,
    ) -> bool {
        let platform_probe = &self.native_window_platform_probe;
        let Some(pointer) = native_window_global_pointer_state(platform_probe) else {
            self.native_global_primary_down = false;
            return false;
        };
        if !pointer.primary_down {
            if self.native_global_primary_down {
                self.native_global_primary_down = false;
                return self.release_native_global_primary_press(app);
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

        self.recover_primary_press_into(app, window_id, pointer)
    }

    fn hand_held_press_to_new_window(
        &mut self,
        app: &mut AppShell<WgpuRenderer>,
        window_id: WinitWindowId,
    ) {
        let holder = app.root_holding_the_press();
        let platform_probe = &self.native_window_platform_probe;
        let Some(native) = self.native_windows.get(&window_id) else {
            return;
        };
        let belongs = press_belongs_here(holder, native.root_id());
        let dragging_elsewhere = self
            .native_windows
            .values()
            .any(|other| other.active_drag.is_some());
        let held = self.held_press_pointer_state();
        let Some(handover) = press_to_hand_over(
            native_window_global_pointer_state(platform_probe),
            held,
            native.options.visible && !dragging_elsewhere,
            belongs,
            |position| native_window_surface_contains_pointer(platform_probe, native, position),
        ) else {
            return;
        };
        trace_native_window!(
            "held press handed to key={:?} pointer=({:.1},{:.1}) relayed_by={:?}",
            native.key,
            handover.pointer.position.x,
            handover.pointer.position.y,
            handover.relayed_by
        );
        self.cancel_held_press_elsewhere(app, window_id);
        let recovered = self.recover_primary_press_into(app, window_id, handover.pointer);
        self.handed_press = handover
            .relayed_by
            .filter(|_| recovered)
            .map(|holder| HandedPress {
                holder,
                taker: window_id,
            });
    }

    fn held_press_pointer_state(&self) -> Option<(WinitWindowId, NativeWindowPointerState)> {
        let platform_probe = &self.native_window_platform_probe;
        let primary = self
            .window
            .as_ref()
            .map(|window| (window, self.primary_held_press));
        let peers = self
            .native_windows
            .values()
            .map(|native| (&native.window, native.held_press));
        primary.into_iter().chain(peers).find_map(|(window, held)| {
            let pointer = held_press_on_screen(platform_probe, window, held?)?;
            Some((window.id(), pointer))
        })
    }

    fn relay_primary_held_press(&mut self, event_loop: &dyn ActiveEventLoop, event: &WindowEvent) {
        let Some(step) = held_press_step(event) else {
            return;
        };
        self.primary_held_press = held_press_after_step(self.primary_held_press, step);
        if self.handed_press.is_none() {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        let Some(mut app) = self.app.take() else {
            return;
        };
        let settlement = self.relay_held_press_step(&mut app, window.id(), &window, step);
        self.app = Some(app);
        self.settle_native_window_event(event_loop, settlement);
    }

    fn relay_native_held_press(
        &mut self,
        app: &mut AppShell<WgpuRenderer>,
        native: &mut NativeWindowSurface,
        event: &WindowEvent,
    ) -> NativeWindowEventSettlement {
        let Some(step) = held_press_step(event) else {
            return NativeWindowEventSettlement::default();
        };
        native.held_press = held_press_after_step(native.held_press, step);
        self.relay_held_press_step(app, native.window.id(), &native.window, step)
    }

    fn relay_held_press_step(
        &mut self,
        app: &mut AppShell<WgpuRenderer>,
        holder: WinitWindowId,
        holder_window: &Arc<dyn Window>,
        step: HeldPressStep,
    ) -> NativeWindowEventSettlement {
        let Some(taker) = self
            .handed_press
            .and_then(|handed| handed.taker_for(holder))
        else {
            return NativeWindowEventSettlement::default();
        };
        if let HeldPressStep::Pressed(_) = step {
            self.handed_press = None;
            return NativeWindowEventSettlement::default();
        }
        let platform_probe = &self.native_window_platform_probe;
        let Some(native) = self.native_windows.get_mut(&taker) else {
            self.handed_press = None;
            return NativeWindowEventSettlement::default();
        };
        let Some(screen) =
            native_window_screen_pointer_physical(platform_probe, holder_window, step.position())
        else {
            return NativeWindowEventSettlement::default();
        };
        Self::set_native_cursor_from_screen(platform_probe, app, native, screen);
        match step {
            HeldPressStep::Pressed(_) | HeldPressStep::Moved(_) => NativeWindowEventSettlement {
                drag_move: Self::update_native_window_polling_drag_target(native, screen),
                ..NativeWindowEventSettlement::default()
            },
            HeldPressStep::Released(_) => {
                self.handed_press = None;
                self.native_global_primary_down = false;
                let settlement = Self::finish_native_press(
                    platform_probe,
                    app,
                    native,
                    Some(screen),
                    "relayed-release",
                    |surface| surface.pointer_released(),
                );
                trace_native_window!("held press released, key={:?} raised", native.key);
                native.window.focus_window();
                settlement
            }
        }
    }

    fn set_native_cursor_from_screen(
        platform_probe: &NativeWindowPlatformProbe,
        app: &mut AppShell<WgpuRenderer>,
        native: &mut NativeWindowSurface,
        screen: PhysicalPosition<f64>,
    ) {
        let Some(local) =
            native_window_local_pointer_physical(platform_probe, &native.window, screen)
        else {
            return;
        };
        let logical = native.platform.pointer_position(local);
        native.last_cursor_position = Some((logical.x, logical.y));
        native.last_cursor_physical_position = Some(local);
        if let Some(mut surface) = native_surface(app, native) {
            surface.set_screen_origin(native_window_surface_origin(platform_probe, &native.window));
            surface.set_cursor(logical.x, logical.y);
        }
    }

    fn finish_native_press(
        platform_probe: &NativeWindowPlatformProbe,
        app: &mut AppShell<WgpuRenderer>,
        native: &mut NativeWindowSurface,
        pointer: Option<PhysicalPosition<f64>>,
        reason: &str,
        release: impl FnOnce(&mut SurfaceMut<'_, WgpuRenderer>) -> bool,
    ) -> NativeWindowEventSettlement {
        let drag_move = pointer
            .and_then(|pointer| Self::update_native_window_polling_drag_target(native, pointer));
        let finish_drag = native.active_drag.take().is_some();
        if finish_drag {
            trace_native_window!("drag finish key={:?} reason={reason}", native.key);
        }
        let handled = native_surface(app, native).is_some_and(|mut surface| {
            surface.set_screen_origin(native_window_surface_origin(platform_probe, &native.window));
            release(&mut surface)
        });
        app.sync_selection_to_primary();
        if handled {
            apply_pointer_button_frame_request(
                &native.window,
                &mut native.last_frame_start_time,
                pointer_button_frame_request(handled),
            );
        }
        NativeWindowEventSettlement {
            sync_after_event: handled,
            drag_move,
            finish_drag,
        }
    }

    fn cancel_held_press_elsewhere(
        &mut self,
        app: &mut AppShell<WgpuRenderer>,
        keep: WinitWindowId,
    ) {
        if app.has_active_pointer_gesture() {
            cancel_app_input(app);
        }
        for native in self.native_windows.values_mut() {
            if native.window.id() == keep {
                continue;
            }
            if let Some(mut surface) = native_surface(app, native)
                && surface.has_active_pointer_gesture()
            {
                cancel_surface_input(&mut surface);
            }
        }
        self.native_global_primary_down = false;
    }

    fn recover_primary_press_into(
        &mut self,
        app: &mut AppShell<WgpuRenderer>,
        window_id: WinitWindowId,
        pointer: NativeWindowPointerState,
    ) -> bool {
        let platform_probe = &self.native_window_platform_probe;
        let Some(mut native) = self.native_windows.remove(&window_id) else {
            self.native_global_primary_down = false;
            return false;
        };
        Self::set_native_cursor_from_screen(platform_probe, app, &mut native, pointer.position);

        let (press_handled, drag_requested) =
            Self::dispatch_native_primary_pressed(platform_probe, app, &mut native);
        if press_handled {
            self.native_global_primary_down = true;
            trace_native_window!(
                "global primary recovered key={:?} drag_requested={}",
                native.key,
                drag_requested
            );
            apply_pointer_button_frame_request(
                &native.window,
                &mut native.last_frame_start_time,
                pointer_button_frame_request(press_handled),
            );
            if drag_requested {
                self.begin_native_window_drag_with_anchor(&mut native, Some(pointer.position));
            } else {
                app.sync_selection_to_primary();
            }
        } else {
            self.native_global_primary_down = false;
        }

        self.native_windows.insert(window_id, native);
        press_handled
    }

    fn release_native_global_primary_press(&mut self, app: &mut AppShell<WgpuRenderer>) -> bool {
        let platform_probe = &self.native_window_platform_probe;
        let mut handled_any = false;
        for native in self.native_windows.values_mut() {
            if native.active_drag.is_some() {
                continue;
            }
            let Some(mut surface) = native_surface(app, native) else {
                continue;
            };
            surface.set_screen_origin(native_window_surface_origin(platform_probe, &native.window));
            let handled = surface.pointer_released();
            app.sync_selection_to_primary();
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
                .and_then(|active_drag| {
                    native_window_drag_poll_deadline(
                        active_drag.next_poll_at,
                        NATIVE_WINDOW_GLOBAL_POINTER_POLLED,
                    )
                })
                .is_some_and(|deadline| deadline <= now)
        });
        if !has_due_drag {
            return false;
        }
        let Some(mut app) = self.app.take() else {
            return false;
        };
        let needs_registry_sync = self.poll_active_native_window_drags_with(&mut app, now);
        self.app = Some(app);
        needs_registry_sync
    }

    fn poll_active_native_window_drags_with(
        &mut self,
        app: &mut AppShell<WgpuRenderer>,
        now: Instant,
    ) -> bool {
        let platform_probe = &self.native_window_platform_probe;
        let pointer = native_window_global_pointer_state(platform_probe);
        let mut updates = Vec::new();
        let mut needs_registry_sync = false;
        for native in self.native_windows.values_mut() {
            let Some(active_drag) = native.active_drag.as_mut() else {
                continue;
            };
            if active_drag.next_poll_at > now {
                continue;
            }
            active_drag.next_poll_at = now + NATIVE_WINDOW_DRAG_POLL_INTERVAL;

            let Some(pointer) = pointer else {
                trace_native_window!(
                    "drag poll skipped key={:?} reason=no-global-pointer",
                    native.key
                );
                continue;
            };
            if !pointer.primary_down {
                native.active_drag = None;
                needs_registry_sync = true;
                trace_native_window!("drag finish key={:?} reason=global-release", native.key);
                if native_surface(app, native).is_some_and(|mut surface| surface.pointer_released())
                {
                    native.window.request_redraw();
                    needs_registry_sync = true;
                }
                app.sync_selection_to_primary();
                continue;
            }
            if let Some(update) =
                Self::update_native_window_polling_drag_target(native, pointer.position)
            {
                updates.push(update);
            }
        }

        for (key, position) in updates {
            self.move_dragged_window(key, position);
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
            external_moves.push((*window_id, native.key, position, previous_state_position));
        }

        let mut moved = false;
        for (window_id, key, position, previous_state_position) in external_moves {
            moved |= self.reconcile_external_native_window_move(
                window_id,
                key,
                position,
                previous_state_position,
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
    ) -> bool {
        let Some(native) = self.native_windows.get_mut(&window_id) else {
            return false;
        };
        if native.active_drag.is_some() || native.pending_outer_positions.has_pending() {
            return false;
        }

        trace_native_window!(
            "poll external move key={:?} pos=({:.1},{:.1})",
            key,
            position.0,
            position.1
        );
        self.native_window_positions.insert(key, position);
        update_native_options_position(&mut native.options, position.0, position.1);
        let position = cranpose_ui::Point::new(position.0, position.1);
        tell_the_application_the_window_moved(&native.events, native.state, position.x, position.y);
        sync_native_window_state_position(
            native.state,
            previous_state_position,
            position.x,
            position.y,
        );
        true
    }

    fn update_native_window_polling_drag_target(
        native: &mut NativeWindowSurface,
        pointer: PhysicalPosition<f64>,
    ) -> Option<(NativeWindowKey, cranpose_ui::Point)> {
        let active_drag = native.active_drag.as_mut()?;
        let target = active_drag.target_for_pointer(pointer);
        if target == active_drag.last_target_outer {
            return None;
        }
        active_drag.last_target_outer = target;
        let logical = target.to_logical::<f64>(native.window.scale_factor());
        trace_native_window!(
            "drag target key={:?} logical=({:.1},{:.1}) physical=({},{})",
            native.key,
            logical.x,
            logical.y,
            target.x,
            target.y
        );
        Some((
            native.key,
            cranpose_ui::Point::new(logical.x as f32, logical.y as f32),
        ))
    }

    fn dispatch_native_window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: WinitWindowId,
        event: WindowEvent,
    ) {
        let Some(mut native) = self.native_windows.remove(&window_id) else {
            return;
        };
        let Some(mut app) = self.app.take() else {
            self.native_windows.insert(window_id, native);
            return;
        };
        let relayed = self.relay_native_held_press(&mut app, &mut native, &event);
        let (keep_window, settlement) =
            self.native_window_event(&mut app, event_loop, &mut native, event);
        self.app = Some(app);
        if keep_window {
            self.native_windows.insert(window_id, native);
            self.settle_native_window_event(event_loop, settlement.also(relayed));
        }
    }

    fn native_window_event(
        &mut self,
        app: &mut AppShell<WgpuRenderer>,
        event_loop: &dyn ActiveEventLoop,
        native: &mut NativeWindowSurface,
        event: WindowEvent,
    ) -> (bool, NativeWindowEventSettlement) {
        let mut keep_window = true;
        let mut sync_after_event = false;
        let mut drag_move_after_insert = None::<(NativeWindowKey, cranpose_ui::Point)>;
        let mut finish_drag_after_insert = false;
        if let Some(label) = native_window_lifecycle_event(&event) {
            trace_native_window!("event {label} key={:?}", native.key);
        }
        let Some(event) = deliver_native_surface_event(app, native, self.current_modifiers, event)
        else {
            return (true, NativeWindowEventSettlement::default());
        };
        match event {
            WindowEvent::CloseRequested => {
                trace_native_window!("event close-request key={:?}", native.key);
                notify_native_window_close_requested(&native.events);
                self.remember_native_window_position(native);
                self.native_window_ids.remove(&native.key);
                self.closed_native_windows.insert(native.key);
                app.remove_window_surface(native.key.raw());
                keep_window = false;
            }
            WindowEvent::SurfaceResized(new_size) => {
                Self::apply_native_window_resize(app, native, new_size.width, new_size.height);
                Self::present_before_the_desktop_composites(
                    app,
                    native,
                    &self.native_window_registry,
                    "resize",
                );
                sync_after_event = true;
            }
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                mut surface_size_writer,
            } => {
                native.platform.set_scale_factor(scale_factor);
                if let Some(mut surface) = native_surface(app, native) {
                    surface.renderer().set_root_scale(scale_factor as f32);
                }

                let new_size = native.window.surface_size();
                let _ = surface_size_writer.request_surface_size(new_size);
                Self::apply_native_window_resize(app, native, new_size.width, new_size.height);
                sync_after_event = true;
            }
            WindowEvent::Focused(true) => {
                native_window_focused(app, native);
            }
            WindowEvent::Moved(position) => {
                native.vsync_interval = monitor_refresh_interval(&native.window);
                let known_position = self.native_window_positions.get(&native.key).copied();
                let previous_state_position =
                    native.state.and_then(|state| state.position_non_reactive());
                let platform_probe = &self.native_window_platform_probe;
                let position = current_native_window_position(platform_probe, native)
                    .unwrap_or_else(|| {
                        let logical = position.to_logical::<f64>(native.window.scale_factor());
                        (logical.x as f32, logical.y as f32)
                    });
                let position_observation = native
                    .pending_outer_positions
                    .acknowledge_or_matches_known(position, known_position);
                trace_native_window!(
                    "event moved key={:?} pos=({:.1},{:.1}) observation={:?} active_drag={}",
                    native.key,
                    position.0,
                    position.1,
                    position_observation,
                    native.active_drag.is_some()
                );
                match position_observation {
                    NativeWindowPositionObservation::Current => {
                        self.native_window_positions.insert(native.key, position);
                        update_native_options_position(&mut native.options, position.0, position.1);
                    }
                    NativeWindowPositionObservation::Superseded => {}
                    NativeWindowPositionObservation::External => {
                        if native.active_drag.is_some() {
                            trace_native_window!(
                                "event moved ignored during polling drag key={:?}",
                                native.key
                            );
                        } else {
                            self.native_window_positions.insert(native.key, position);
                            update_native_options_position(
                                &mut native.options,
                                position.0,
                                position.1,
                            );
                            let position = cranpose_ui::Point::new(position.0, position.1);
                            native.pending_outer_positions.clear();
                            tell_the_application_the_window_moved(
                                &native.events,
                                native.state,
                                position.x,
                                position.y,
                            );
                            sync_native_window_state_position(
                                native.state,
                                previous_state_position,
                                position.x,
                                position.y,
                            );
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
                let platform_probe = &self.native_window_platform_probe;
                let event_pointer =
                    native_window_screen_pointer_physical(platform_probe, &native.window, position);
                let global_pointer = native_window_global_pointer_state(platform_probe);
                let pointer_position = global_pointer.map(|state| state.position).or(event_pointer);
                let handled = native_surface(app, native).is_some_and(|mut surface| {
                    surface.set_pointer_source(pointer_source_from_winit(&source));
                    surface.set_screen_origin(native_window_surface_origin(
                        platform_probe,
                        &native.window,
                    ));
                    surface.set_cursor(logical.x, logical.y)
                });
                if let Some(pointer) = pointer_position
                    && let Some((key, position)) =
                        Self::update_native_window_polling_drag_target(native, pointer)
                {
                    drag_move_after_insert = Some((key, position));
                    sync_after_event = true;
                }
                if native.active_drag.is_none()
                    && !self.native_global_primary_down
                    && global_pointer.is_some_and(|pointer| {
                        pointer.primary_down
                            && native_window_surface_contains_pointer(
                                platform_probe,
                                native,
                                pointer.position,
                            )
                    })
                {
                    let (press_handled, drag_requested) =
                        Self::dispatch_native_primary_pressed(platform_probe, app, native);
                    if press_handled {
                        self.native_global_primary_down = true;
                        trace_native_window!(
                            "event pointer-move recovered primary-press key={:?} drag_requested={}",
                            native.key,
                            drag_requested
                        );
                        apply_pointer_button_frame_request(
                            &native.window,
                            &mut native.last_frame_start_time,
                            pointer_button_frame_request(press_handled),
                        );
                        if drag_requested {
                            self.begin_native_window_drag_with_anchor(
                                native,
                                recovered_native_window_drag_start_pointer(
                                    event_pointer,
                                    global_pointer,
                                ),
                            );
                        } else {
                            app.sync_selection_to_primary();
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
                app.set_modifiers(app_modifiers(self.current_modifiers));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if native.last_cursor_position.is_none() {
                    Self::refresh_native_cursor_from_platform_pointer(
                        &self.native_window_platform_probe,
                        app,
                        native,
                    );
                }
                if let Some(mut surface) = native_surface(app, native) {
                    dispatch_mouse_wheel(
                        &mut surface,
                        &native.platform,
                        self.current_modifiers,
                        native.last_cursor_position,
                        delta,
                    );
                }
            }
            WindowEvent::PointerButton {
                state,
                position,
                button,
                ..
            } if is_primary_pointer_button(&button) => {
                let source = pointer_source_from_button(&button);
                let logical = native.platform.pointer_position(position);
                native.last_cursor_position = Some((logical.x, logical.y));
                native.last_cursor_physical_position = Some(position);
                trace_native_window!(
                    "event pointer-button key={:?} state={:?} cursor={:?}",
                    native.key,
                    state,
                    native.last_cursor_position
                );
                if let Some(mut surface) = native_surface(app, native) {
                    surface.set_pointer_source(source);
                    let platform_probe = &self.native_window_platform_probe;
                    surface.set_screen_origin(native_window_surface_origin(
                        platform_probe,
                        &native.window,
                    ));
                    surface.set_cursor(logical.x, logical.y);
                }
                match state {
                    ElementState::Pressed => {
                        if self.native_global_primary_down || native.active_drag.is_some() {
                            trace_native_window!(
                                "event pointer-button duplicate-primary-down key={:?}",
                                native.key
                            );
                            self.native_global_primary_down = true;
                        } else {
                            self.native_global_primary_down = true;
                            if self.handle_native_primary_pressed(app, native) {
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
                        let pointer =
                            native_window_global_pointer_state(&self.native_window_platform_probe)
                                .map(|state| state.position)
                                .or(fallback_pointer);
                        let released = Self::finish_native_press(
                            &self.native_window_platform_probe,
                            app,
                            native,
                            pointer,
                            "local-release",
                            |surface| {
                                if source.is_touch_like() {
                                    surface.pointer_released_at_position(logical.x, logical.y)
                                } else {
                                    surface.pointer_released()
                                }
                            },
                        );
                        drag_move_after_insert = released.drag_move;
                        finish_drag_after_insert = released.finish_drag;
                        sync_after_event |= released.sync_after_event;
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
                    app,
                    native,
                );
                if let Some(mut surface) = native_surface(app, native) {
                    dispatch_middle_click_paste(&mut surface, native.last_cursor_position);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(deadline) = native.last_frame_start_time.and_then(|started_at| {
                    native
                        .frame_interval(app.frame_pacing_mode())
                        .map(|interval| started_at + interval)
                }) && deadline > Instant::now()
                {
                    event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
                    return (true, NativeWindowEventSettlement::default());
                }
                Self::redraw_native_window(app, native, &self.native_window_registry);
            }
            event => {
                if pointer_icon_is_owed_again(&event)
                    && let Some(surface) = native_surface(app, native)
                {
                    surface.refresh_pointer_icon();
                }
                present_native_frame_owed_while_hidden(native, &event);
            }
        }

        let settlement = NativeWindowEventSettlement {
            sync_after_event,
            drag_move: drag_move_after_insert,
            finish_drag: finish_drag_after_insert,
        };
        (keep_window, settlement)
    }

    fn settle_native_window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        settlement: NativeWindowEventSettlement,
    ) {
        let mut sync_after_event = settlement.sync_after_event;
        if let Some((key, position)) = settlement.drag_move
            && self.move_dragged_window(key, position)
        {
            sync_after_event = true;
        }
        if sync_after_event {
            self.refresh_and_sync_native_windows(event_loop);
        }
    }

    fn redraw_native_window(
        app: &mut AppShell<WgpuRenderer>,
        native: &mut NativeWindowSurface,
        registry: &Rc<native_window::NativeWindowRegistry>,
    ) -> bool {
        if native_window_redraw_held_while_hidden(native.options.visible) {
            return false;
        }
        let frame_started_at = Instant::now();
        update_app_with_native_window_registry(app, registry);
        let after_update = Instant::now();
        let Some(mut surface) = native_surface(app, native) else {
            return false;
        };
        let frame_owed = surface.take_frame_owed();
        let present_required =
            surface_present_required(native.surface_dirty, frame_owed, surface.needs_redraw());
        pace_after_empty_redraw(
            &mut native.last_frame_start_time,
            frame_started_at,
            present_required,
        );
        if !present_required {
            return false;
        }
        native.surface_dirty = true;

        let output = match current_surface_texture(&native.surface, "native window") {
            SurfaceFrame::Ready(output) => output,
            SurfaceFrame::Reconfigure => {
                trace_native_window!("redraw surface outdated key={:?}", native.key);
                let size = native.window.surface_size();
                Self::resize_native_surface(app, native, size.width, size.height);
                if surface_reconfigure_requires_redraw(size.width, size.height) {
                    native.window.request_redraw();
                }
                return false;
            }
            SurfaceFrame::Skip => {
                trace_native_window!("redraw surface unavailable key={:?}", native.key);
                pace_after_empty_redraw(&mut native.last_frame_start_time, frame_started_at, false);
                return false;
            }
        };
        let after_acquire = Instant::now();

        let view = output.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(crate::surface_format::display_surface_view_format(
                native.surface_config.format,
            )),
            ..Default::default()
        });
        let Some(mut surface) = native_surface(app, native) else {
            return false;
        };
        if let Err(error) = surface.renderer().render_surface_texture(
            &output.texture,
            &view,
            native.surface_config.width,
            native.surface_config.height,
        ) {
            log::error!("native window render failed: {error:?}");
            return false;
        }
        let after_render = Instant::now();

        native.window.pre_present_notify();
        output.present();
        note_native_window_presented(native.state, true);
        let after_present = Instant::now();
        trace_native_window_timing!(
            "{} presented {}x{} key={:?}",
            native.options.title,
            native.surface_config.width,
            native.surface_config.height,
            native.key
        );
        native.surface_dirty = false;
        app.record_presented_frame(frame_started_at, after_render);
        log_desktop_frame_telemetry(
            frame_started_at,
            after_update,
            after_acquire,
            after_render,
            after_present,
            "native",
        );
        native.last_frame_start_time = Some(frame_started_at);
        let needs_frame =
            native_surface(app, native).is_some_and(|surface| surface.frame_schedule().needs_frame);
        if should_chain_no_vsync_redraw(native.frame_interval(app.frame_pacing_mode()), needs_frame)
        {
            native.window.request_redraw();
        }
        true
    }

    #[cfg(feature = "robot")]
    fn set_robot_controller(&mut self, controller: RobotController) {
        self.robot_controller = Some(controller);
    }

    #[cfg(feature = "robot")]
    fn robot_drives(&self) -> bool {
        self.robot_controller.is_some()
    }

    #[cfg(not(feature = "robot"))]
    fn robot_drives(&self) -> bool {
        false
    }
}

#[cfg(feature = "robot")]
fn begin_robot_pump_present_wait(
    controller: &mut RobotController,
    window: &Arc<dyn Window>,
    last_frame_start_time: &mut Option<Instant>,
    primary_redraw_pending: &mut bool,
    target: u64,
) {
    controller.begin_pump_present_wait(target);
    *last_frame_start_time = None;
    request_redraw_once(window, primary_redraw_pending);
}

#[cfg(feature = "robot")]
fn robot_finish_without_wait(
    app: &mut AppShell<WgpuRenderer>,
    registry: &Rc<native_window::NativeWindowRegistry>,
    controller: &RobotController,
    robot_visible_surface_dirty: &mut bool,
) {
    let update_result = update_app_with_native_window_registry(app, registry);
    *robot_visible_surface_dirty = app.needs_redraw() || update_result.visual_changed;
    let _ = controller.tx.send(RobotResponse::Ok);
}

#[cfg(feature = "robot")]
#[allow(clippy::too_many_arguments)]
fn robot_present_or_update(
    controller: &mut RobotController,
    window: &Arc<dyn Window>,
    app: &mut AppShell<WgpuRenderer>,
    registry: &Rc<native_window::NativeWindowRegistry>,
    last_frame_start_time: &mut Option<Instant>,
    primary_redraw_pending: &mut bool,
    robot_visible_surface_dirty: &mut bool,
    target: Option<u64>,
) {
    match target {
        Some(target) => begin_robot_pump_present_wait(
            controller,
            window,
            last_frame_start_time,
            primary_redraw_pending,
            target,
        ),
        None => robot_finish_without_wait(app, registry, controller, robot_visible_surface_dirty),
    }
}

#[cfg(feature = "robot")]
fn robot_present_or_ack(
    controller: &mut RobotController,
    window: &Arc<dyn Window>,
    last_frame_start_time: &mut Option<Instant>,
    primary_redraw_pending: &mut bool,
    target: Option<u64>,
) {
    match target {
        Some(target) => begin_robot_pump_present_wait(
            controller,
            window,
            last_frame_start_time,
            primary_redraw_pending,
            target,
        ),
        None => {
            let _ = controller.tx.send(RobotResponse::Ok);
        }
    }
}

#[cfg(feature = "robot")]
fn robot_scroll_present_target(
    app: &mut AppShell<WgpuRenderer>,
    delta_x: f32,
    delta_y: f32,
    primary_window_visible: bool,
    headless: bool,
    presented_frame_generation: u64,
) -> Option<Option<u64>> {
    let consumed = app.pointer_scrolled(delta_x, delta_y);
    if !consumed {
        return None;
    }
    Some(robot_visible_pump_present_target(
        primary_window_visible,
        headless,
        1,
        presented_frame_generation,
    ))
}

#[cfg(feature = "robot")]
fn robot_frame_diagnostics(
    command: &RobotCommand,
    app: &mut AppShell<WgpuRenderer>,
    config: Option<&wgpu::SurfaceConfiguration>,
    caps: Option<&wgpu::SurfaceCapabilities>,
    interval: Duration,
) -> RobotResponse {
    match command {
        RobotCommand::GetRenderStats => {
            RobotResponse::RenderStats(Box::new(app.renderer().last_frame_stats()))
        }
        RobotCommand::GetFpsStats => RobotResponse::FpsStats(app.fps_stats()),
        RobotCommand::GetPresentationInfo => match (config, caps) {
            (Some(config), Some(caps)) => {
                RobotResponse::PresentationInfo(crate::RobotPresentationInfo {
                    requested_mode: std::env::var("CRANPOSE_PRESENT_MODE").ok(),
                    present_mode: crate::present_mode::resolved_present_mode(
                        config.present_mode,
                        caps,
                    ),
                    supported_modes: caps.present_modes.clone(),
                    frame_pacing_mode: app.frame_pacing_mode(),
                    refresh_rate_hz: 1.0 / interval.as_secs_f64(),
                })
            }
            _ => RobotResponse::Error("Window presentation is not configured".into()),
        },
        _ => RobotResponse::Error("Command is not a frame diagnostics query".into()),
    }
}

fn apply_frame_pacing_mode(
    device: &wgpu::Device,
    surface: &wgpu::Surface<'static>,
    surface_config: &mut wgpu::SurfaceConfiguration,
    surface_caps: Option<&wgpu::SurfaceCapabilities>,
    mode: FramePacingMode,
    vsync_interval: Duration,
) {
    let Some(caps) = surface_caps else {
        log::error!(
            "desktop surface pacing not applied for {}: surface capabilities are unknown",
            mode.label()
        );
        return;
    };
    let present_mode = desktop_present_mode(caps, mode);
    let frame_latency = desired_frame_latency(mode, vsync_interval);
    if surface_config.present_mode == present_mode
        && surface_config.desired_maximum_frame_latency == frame_latency
    {
        return;
    }
    surface_config.present_mode = present_mode;
    surface_config.desired_maximum_frame_latency = frame_latency;
    surface.configure(device, surface_config);
    log_desktop_present_mode(mode, present_mode, caps);
}

fn log_desktop_present_mode(
    mode: FramePacingMode,
    present_mode: wgpu::PresentMode,
    caps: &wgpu::SurfaceCapabilities,
) {
    log::info!(
        "desktop present mode: pacing={} chose {present_mode:?}; surface offers {:?}",
        mode.label(),
        caps.present_modes,
    );
}

fn desired_frame_latency(mode: FramePacingMode, vsync_interval: Duration) -> u32 {
    match mode {
        FramePacingMode::Vsync | FramePacingMode::NoVsync => 2,
        FramePacingMode::Hard60 | FramePacingMode::Hard120 => {
            match frame_interval_for_mode(mode, vsync_interval) {
                Some(interval) if interval >= vsync_interval => 1,
                _ => 2,
            }
        }
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

fn update_declaration_host_frame(
    app: &mut AppShell<WgpuRenderer>,
    registry: &Rc<native_window::NativeWindowRegistry>,
    last_frame_start_time: &mut Option<Instant>,
    frame_interval: Option<Duration>,
) -> FrameUpdateResult {
    let frame_started_at = Instant::now();
    let update_result = update_app_with_native_window_registry(app, registry);
    let presented = app.take_frame_owed();
    if presented {
        app.record_presented_frame(frame_started_at, Instant::now());
    }
    *last_frame_start_time = declaration_host_frame_anchor(
        *last_frame_start_time,
        frame_started_at,
        frame_interval,
        presented,
        app.frame_schedule().needs_frame,
    );
    update_result
}

fn declaration_host_frame_anchor(
    previous: Option<Instant>,
    frame_started_at: Instant,
    interval: Option<Duration>,
    presented: bool,
    still_needs_frame: bool,
) -> Option<Instant> {
    if presented || still_needs_frame {
        Some(next_frame_anchor(previous, frame_started_at, interval))
    } else {
        previous
    }
}

fn native_surface_needs_frame(
    needs_frame: bool,
    frame_owed: bool,
    scene_dirty: bool,
    surface_dirty: bool,
) -> bool {
    needs_frame || frame_owed || scene_dirty || surface_dirty
}

fn should_chain_no_vsync_redraw(frame_interval: Option<Duration>, needs_frame: bool) -> bool {
    frame_interval.is_none() && needs_frame
}

fn free_running_frame(
    frame_interval: Option<Duration>,
    needs_redraw: bool,
    redraw_pending: bool,
) -> bool {
    frame_interval.is_none() && (needs_redraw || redraw_pending)
}

struct LoopControlInputs {
    robot_needs_poll: bool,
    free_running: bool,
    primary_pointer_polled: bool,
    drag_poll_deadline: Option<Instant>,
    position_poll_deadline: Option<Instant>,
    frame_cap_deadline: Option<Instant>,
    has_active_animations: bool,
    next_event_time: Option<Instant>,
}

fn event_loop_control_flow(now: Instant, inputs: LoopControlInputs) -> ControlFlow {
    let due = |deadline: Option<Instant>| deadline.is_some_and(|deadline| deadline <= now);
    if inputs.robot_needs_poll
        || inputs.free_running
        || inputs.primary_pointer_polled
        || due(inputs.drag_poll_deadline)
        || due(inputs.position_poll_deadline)
    {
        return ControlFlow::Poll;
    }
    let deadline = [
        inputs.frame_cap_deadline,
        inputs.position_poll_deadline,
        inputs.drag_poll_deadline,
    ]
    .into_iter()
    .flatten()
    .min();
    match deadline {
        Some(deadline) => ControlFlow::WaitUntil(deadline),
        None if inputs.has_active_animations => ControlFlow::Poll,
        None => inputs
            .next_event_time
            .map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
    }
}

fn native_window_drag_poll_deadline(
    next_poll_at: Instant,
    global_pointer_polled: bool,
) -> Option<Instant> {
    global_pointer_polled.then_some(next_poll_at)
}

fn pace_after_empty_redraw(
    last_frame_start_time: &mut Option<Instant>,
    attempt_started_at: Instant,
    presented_something: bool,
) {
    if !presented_something {
        *last_frame_start_time = Some(attempt_started_at);
    }
}

fn primary_wrap_request(
    content: Option<cranpose_ui::Size>,
    last: Option<(u32, u32)>,
) -> Option<(u32, u32)> {
    let content = content.filter(|size| size.width > 0.0 && size.height > 0.0)?;
    let wanted = (content.width.ceil() as u32, content.height.ceil() as u32);
    (last != Some(wanted)).then_some(wanted)
}

fn wrap_primary_window_to_content(
    window: &dyn Window,
    app: &mut AppShell<WgpuRenderer>,
    wraps_content: bool,
    last: Option<(u32, u32)>,
) -> Option<(u32, u32)> {
    let content = wraps_content.then(|| app.primary_content_size()).flatten();
    let (width, height) = primary_wrap_request(content, last)?;
    trace_native_window!("primary window wraps {width}x{height}");
    let origin = window.outer_position().ok();
    let _ = window.request_surface_size(LogicalSize::new(width as f64, height as f64).into());
    if let Some(origin) = origin {
        window.set_outer_position(Position::Physical(origin));
    }
    Some((width, height))
}

#[allow(clippy::too_many_arguments)]
fn wrap_primary_window_for_frame(
    app: &mut AppShell<WgpuRenderer>,
    surface: &wgpu::Surface<'static>,
    surface_config: &mut wgpu::SurfaceConfiguration,
    window: &Arc<dyn Window>,
    registry: &Rc<native_window::NativeWindowRegistry>,
    primary_viewport_override: Option<(f32, f32)>,
    wraps_content: bool,
    last: &mut Option<(u32, u32)>,
) {
    let content = wraps_content.then(|| app.primary_content_size()).flatten();
    let Some((width, height)) = primary_wrap_request(content, *last) else {
        return;
    };
    trace_native_window!("primary window wraps {width}x{height} with the frame it drew");
    let origin = window.outer_position().ok();
    let applied = window.request_surface_size(LogicalSize::new(width as f64, height as f64).into());
    if let Some(origin) = origin {
        window.set_outer_position(Position::Physical(origin));
    }
    *last = Some((width, height));
    if let Some(size) = applied {
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
        update_app_with_native_window_registry(app, registry);
    }
}

fn note_native_window_presented(state: Option<WindowState>, presented: bool) {
    if let Some(state) = state {
        state.set_presented(presented);
    }
}

#[derive(Default)]
struct PacingDiag {
    presents: u32,
    redraw_events: u32,
    updates: u32,
    direct_updates: u32,
    skipped_no_present: u32,
    control_flow_poll: u32,
    control_flow_wait_until: u32,
    control_flow_wait: u32,
    window_started: Option<Instant>,
}

thread_local! {
    static PACING_DIAG: RefCell<PacingDiag> = RefCell::new(PacingDiag::default());
}

fn pacing_diag_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CRANPOSE_PACING_DIAG").is_some())
}

fn record_pacing_event(counter: fn(&mut PacingDiag) -> &mut u32) {
    if !pacing_diag_enabled() {
        return;
    }
    PACING_DIAG.with(|cell| {
        let Ok(mut diag) = cell.try_borrow_mut() else {
            return;
        };
        *counter(&mut diag) += 1;
        let now = Instant::now();
        let started = *diag.window_started.get_or_insert(now);
        if now.duration_since(started) < Duration::from_secs(1) {
            return;
        }
        log::warn!(
            "[pacing] present={} redraw_events={} update={} direct_update={} no_present={} control_flow(poll={} wait_until={} wait={})",
            diag.presents,
            diag.redraw_events,
            diag.updates,
            diag.direct_updates,
            diag.skipped_no_present,
            diag.control_flow_poll,
            diag.control_flow_wait_until,
            diag.control_flow_wait,
        );
        *diag = PacingDiag {
            window_started: Some(now),
            ..PacingDiag::default()
        };
    });
}

fn next_frame_anchor(
    previous: Option<Instant>,
    frame_started_at: Instant,
    interval: Option<Duration>,
) -> Instant {
    let (Some(previous), Some(interval)) = (previous, interval) else {
        return frame_started_at;
    };
    if interval.is_zero() {
        return frame_started_at;
    }
    let anchor = previous + interval;
    if frame_started_at.saturating_duration_since(anchor) >= interval {
        frame_started_at
    } else {
        anchor
    }
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

fn a_new_window_comes_up_key(
    headless: bool,
    visible: bool,
    anything_focused: bool,
    focus: WindowFocus,
) -> bool {
    let wanted = match focus {
        WindowFocus::Never => false,
        WindowFocus::WhenNoneFocused => !anything_focused,
        WindowFocus::Always => true,
    };
    !headless && visible && wanted
}

fn native_window_polling_drag_pointer(
    global: Option<NativeWindowPointerState>,
    start_pointer_screen: Option<PhysicalPosition<f64>>,
) -> Option<PhysicalPosition<f64>> {
    start_pointer_screen.or(global.map(|global| global.position))
}

fn native_window_attributes(
    options: &NativeWindowOptions,
    headless: bool,
    active: bool,
) -> WindowAttributes {
    let mut attributes = WindowAttributes::default()
        .with_active(active)
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
    attributes = with_native_window_shadow(attributes, options.shadow);
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
    frame_latency: u32,
) -> Result<wgpu::SurfaceConfiguration, LaunchError> {
    Ok(wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width,
        height,
        present_mode,
        alpha_mode: select_alpha_mode(surface_caps, transparent)?,
        view_formats: crate::surface_format::display_surface_view_formats(surface_format),
        desired_maximum_frame_latency: frame_latency,
    })
}

fn wgpu_renderer_for_surface(
    text_system: WgpuTextSystem,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface_format: wgpu::TextureFormat,
    backend: wgpu::Backend,
    downlevel: wgpu::DownlevelFlags,
    scale_factor: f64,
) -> WgpuRenderer {
    let mut renderer = WgpuRenderer::with_text_system(text_system);
    renderer.warm_shaders(cranpose_liquid::shader_warm_ups());
    renderer.set_root_scale(scale_factor as f32);
    renderer.init_gpu(device, queue, surface_format, backend, downlevel);
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
        return transparent_alpha_mode(&surface_caps.alpha_modes)
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

fn transparent_alpha_mode(
    offered: &[wgpu::CompositeAlphaMode],
) -> Option<wgpu::CompositeAlphaMode> {
    [
        wgpu::CompositeAlphaMode::PreMultiplied,
        wgpu::CompositeAlphaMode::PostMultiplied,
    ]
    .into_iter()
    .find(|wanted| offered.contains(wanted))
    .or_else(|| offered.first().copied())
}

fn native_window_level(always_on_top: bool) -> WindowLevel {
    if always_on_top {
        WindowLevel::AlwaysOnTop
    } else {
        WindowLevel::Normal
    }
}

#[cfg(target_os = "macos")]
fn with_native_window_shadow(attributes: WindowAttributes, shadow: bool) -> WindowAttributes {
    use winit::platform::macos::WindowAttributesMacOS;

    attributes.with_platform_attributes(Box::new(
        WindowAttributesMacOS::default().with_has_shadow(shadow),
    ))
}

#[cfg(not(target_os = "macos"))]
fn with_native_window_shadow(attributes: WindowAttributes, _shadow: bool) -> WindowAttributes {
    attributes
}

#[cfg(target_os = "macos")]
fn set_native_window_shadow(window: &Arc<dyn Window>, shadow: bool) {
    use winit::platform::macos::WindowExtMacOS;

    window.set_has_shadow(shadow);
}

#[cfg(not(target_os = "macos"))]
fn set_native_window_shadow(_window: &Arc<dyn Window>, _shadow: bool) {}

fn current_native_window_physical_position(
    platform_probe: &NativeWindowPlatformProbe,
    window: &Arc<dyn Window>,
) -> Option<PhysicalPosition<i32>> {
    native_window_x11_surface_position_physical(platform_probe, window)
        .map(|surface_origin| {
            physical_outer_origin_from_surface(surface_origin, window.surface_position())
        })
        .or_else(|| window.outer_position().ok())
}

fn current_native_window_surface_physical_position(
    platform_probe: &NativeWindowPlatformProbe,
    window: &Arc<dyn Window>,
) -> Option<PhysicalPosition<i32>> {
    native_window_x11_surface_position_physical(platform_probe, window).or_else(|| {
        window
            .outer_position()
            .ok()
            .map(|outer| physical_surface_origin_from_outer(outer, window.surface_position()))
    })
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

fn native_window_positions_close(a: (f32, f32), b: (f32, f32)) -> bool {
    (a.0 - b.0).abs() <= 1.0 && (a.1 - b.1).abs() <= 1.0
}

fn native_window_surface_origin(
    platform_probe: &NativeWindowPlatformProbe,
    window: &Arc<dyn Window>,
) -> Option<cranpose_ui::Point> {
    let physical = current_native_window_surface_physical_position(platform_probe, window)?;
    let scale_factor = window.scale_factor();
    let logical = physical.to_logical::<f64>(scale_factor);
    Some(cranpose_ui::Point::new(logical.x as f32, logical.y as f32))
}

fn native_window_screen_pointer_physical(
    platform_probe: &NativeWindowPlatformProbe,
    window: &Arc<dyn Window>,
    local: PhysicalPosition<f64>,
) -> Option<PhysicalPosition<f64>> {
    let surface = current_native_window_surface_physical_position(platform_probe, window)?;
    Some(PhysicalPosition::new(
        surface.x as f64 + local.x,
        surface.y as f64 + local.y,
    ))
}

fn native_window_local_pointer_physical(
    platform_probe: &NativeWindowPlatformProbe,
    window: &Arc<dyn Window>,
    screen: PhysicalPosition<f64>,
) -> Option<PhysicalPosition<f64>> {
    let surface = current_native_window_surface_physical_position(platform_probe, window)?;
    Some(physical_surface_local_pointer(surface, screen))
}

fn native_window_surface_contains_pointer(
    platform_probe: &NativeWindowPlatformProbe,
    native: &NativeWindowSurface,
    pointer: PhysicalPosition<f64>,
) -> bool {
    let Some(surface) =
        current_native_window_surface_physical_position(platform_probe, &native.window)
    else {
        return false;
    };
    physical_surface_rect_contains_pointer(
        surface,
        PhysicalPosition::new(0, 0),
        native.window.surface_size(),
        pointer,
    )
}

fn physical_surface_origin_from_outer(
    outer: PhysicalPosition<i32>,
    surface_offset: PhysicalPosition<i32>,
) -> PhysicalPosition<i32> {
    PhysicalPosition::new(outer.x + surface_offset.x, outer.y + surface_offset.y)
}

fn physical_outer_origin_from_surface(
    surface_origin: PhysicalPosition<i32>,
    surface_offset: PhysicalPosition<i32>,
) -> PhysicalPosition<i32> {
    PhysicalPosition::new(
        surface_origin.x - surface_offset.x,
        surface_origin.y - surface_offset.y,
    )
}

fn physical_surface_local_pointer(
    surface_origin: PhysicalPosition<i32>,
    screen: PhysicalPosition<f64>,
) -> PhysicalPosition<f64> {
    PhysicalPosition::new(
        screen.x - surface_origin.x as f64,
        screen.y - surface_origin.y as f64,
    )
}

fn recovered_native_window_drag_start_pointer(
    event_pointer: Option<PhysicalPosition<f64>>,
    global_pointer: Option<NativeWindowPointerState>,
) -> Option<PhysicalPosition<f64>> {
    event_pointer.or_else(|| global_pointer.map(|pointer| pointer.position))
}

fn held_press_to_hand_over(
    pointer: Option<NativeWindowPointerState>,
    window_can_take_it: bool,
    belongs: PressBelongsHere,
    window_contains: impl FnOnce(PhysicalPosition<f64>) -> bool,
) -> Option<NativeWindowPointerState> {
    let pointer = pointer.filter(|pointer| pointer.primary_down)?;
    if !window_can_take_it {
        return None;
    }
    let takes_it = match belongs {
        PressBelongsHere::ItsNodeMovedHere => true,
        PressBelongsHere::AskTheRectangle => window_contains(pointer.position),
    };
    takes_it.then_some(pointer)
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

#[derive(Clone, Copy, Debug, PartialEq)]
struct NativeWindowPointerState {
    position: PhysicalPosition<f64>,
    primary_down: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HandedPress {
    holder: WinitWindowId,
    taker: WinitWindowId,
}

impl HandedPress {
    fn taker_for(self, holder: WinitWindowId) -> Option<WinitWindowId> {
        (self.holder == holder).then_some(self.taker)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum HeldPressStep {
    Pressed(PhysicalPosition<f64>),
    Moved(PhysicalPosition<f64>),
    Released(PhysicalPosition<f64>),
}

impl HeldPressStep {
    fn position(self) -> PhysicalPosition<f64> {
        match self {
            Self::Pressed(position) | Self::Moved(position) | Self::Released(position) => position,
        }
    }
}

fn held_press_step(event: &WindowEvent) -> Option<HeldPressStep> {
    match event {
        WindowEvent::PointerButton {
            state: ElementState::Pressed,
            position,
            button,
            ..
        } if is_primary_pointer_button(button) => Some(HeldPressStep::Pressed(*position)),
        WindowEvent::PointerButton {
            state: ElementState::Released,
            position,
            button,
            ..
        } if is_primary_pointer_button(button) => Some(HeldPressStep::Released(*position)),
        WindowEvent::PointerMoved { position, .. } => Some(HeldPressStep::Moved(*position)),
        _ => None,
    }
}

fn held_press_after_step(
    held: Option<PhysicalPosition<f64>>,
    step: HeldPressStep,
) -> Option<PhysicalPosition<f64>> {
    match step {
        HeldPressStep::Pressed(position) => Some(position),
        HeldPressStep::Moved(position) => held.map(|_| position),
        HeldPressStep::Released(_) => None,
    }
}

fn native_window_press_anchor(
    platform_probe: &NativeWindowPlatformProbe,
    native: &NativeWindowSurface,
) -> Option<PhysicalPosition<f64>> {
    let local = native.last_cursor_physical_position?;
    native_window_screen_pointer_physical(platform_probe, &native.window, local)
}

fn held_press_on_screen(
    platform_probe: &NativeWindowPlatformProbe,
    window: &Arc<dyn Window>,
    local: PhysicalPosition<f64>,
) -> Option<NativeWindowPointerState> {
    let position = native_window_screen_pointer_physical(platform_probe, window, local)?;
    Some(NativeWindowPointerState {
        position,
        primary_down: true,
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PressToHandOver {
    pointer: NativeWindowPointerState,
    relayed_by: Option<WinitWindowId>,
}

/// Whose the press is, as the application itself says.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PressBelongsHere {
    /// The node that took the press now draws in this window, so the press
    /// came here with it and the window takes it wherever it is on screen.
    ItsNodeMovedHere,
    /// No node of this window took the press. It may still belong here — a
    /// gesture can be carried by a node that stays where it is, as a tab's
    /// drag-and-drop source stays in the strip the tab was pulled out of —
    /// so the pointer and the window's rectangle are all there is to go on.
    AskTheRectangle,
}

fn press_belongs_here(holder: Option<RootId>, window: RootId) -> PressBelongsHere {
    match holder == Some(window) {
        true => PressBelongsHere::ItsNodeMovedHere,
        false => PressBelongsHere::AskTheRectangle,
    }
}

fn press_to_hand_over(
    platform_pointer: Option<NativeWindowPointerState>,
    held: Option<(WinitWindowId, NativeWindowPointerState)>,
    window_can_take_it: bool,
    belongs: PressBelongsHere,
    window_contains: impl FnOnce(PhysicalPosition<f64>) -> bool,
) -> Option<PressToHandOver> {
    let relayed_by = match platform_pointer {
        Some(_) => None,
        None => Some(held?.0),
    };
    let pointer = held
        .map(|(_, pointer)| pointer)
        .or(platform_pointer)
        .and_then(|pointer| {
            held_press_to_hand_over(Some(pointer), window_can_take_it, belongs, window_contains)
        })?;
    Some(PressToHandOver {
        pointer,
        relayed_by,
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum PrimaryPointerGesturePollAction {
    Inactive,
    Pressed(NativeWindowPointerState),
    ReleaseAt(PhysicalPosition<f64>),
}

fn primary_pointer_gesture_poll_action(
    active: bool,
    synthetic_primary_down: bool,
    pointer: Option<NativeWindowPointerState>,
) -> PrimaryPointerGesturePollAction {
    if !active || synthetic_primary_down {
        return PrimaryPointerGesturePollAction::Inactive;
    }

    match pointer {
        Some(pointer) if pointer.primary_down => PrimaryPointerGesturePollAction::Pressed(pointer),
        Some(pointer) => PrimaryPointerGesturePollAction::ReleaseAt(pointer.position),
        None => PrimaryPointerGesturePollAction::Inactive,
    }
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
        use x11rb::{
            connection::Connection,
            protocol::xproto::{ConfigureWindowAux, ConnectionExt},
        };

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

    fn window_surface_position(&self, window: u32) -> Option<PhysicalPosition<i32>> {
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
fn native_window_x11_surface_position_physical(
    platform_probe: &NativeWindowPlatformProbe,
    window: &Arc<dyn Window>,
) -> Option<PhysicalPosition<i32>> {
    let window_id = native_window_x11_id(window)?;
    platform_probe
        .probe_x11_window_client(|client| client.window_surface_position(window_id))
        .flatten()
}

#[cfg(not(all(
    target_os = "linux",
    not(target_arch = "wasm32"),
    feature = "desktop-x11"
)))]
fn native_window_x11_surface_position_physical(
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

fn native_window_timing_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CRANPOSE_NATIVE_WINDOW_TIMING").is_some())
}

fn print_native_window_timing(args: std::fmt::Arguments<'_>) {
    println!(
        "native window timing: t={:.1}ms {args}",
        timing_trace_clock().elapsed().as_secs_f64() * 1000.0
    );
}

fn timing_trace_clock() -> Instant {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *START.get_or_init(Instant::now)
}

fn native_window_trace_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CRANPOSE_NATIVE_TRACE").is_some())
}

fn print_native_window_trace(args: std::fmt::Arguments<'_>) {
    println!("native window trace: {args}");
}

fn primary_surface_redraw_drives_app(primary_window_visible: bool, headless: bool) -> bool {
    primary_window_visible && !headless
}

fn primary_window_should_show(headless: bool, has_content: bool) -> bool {
    !headless && has_content
}

fn show_primary_when_it_has_content(
    shown: &std::sync::atomic::AtomicBool,
    app: &mut AppShell<WgpuRenderer>,
    window: &dyn Window,
    headless: bool,
) {
    let show = primary_window_should_show(headless, app.primary_has_content());
    if show != shown.load(std::sync::atomic::Ordering::Relaxed) {
        shown.store(show, std::sync::atomic::Ordering::Relaxed);
        trace_native_window!("primary window {}", if show { "shown" } else { "hidden" });
        window.set_visible(show);
    }
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

fn occlusion_leaves_a_frame_owed(occluded: bool) -> bool {
    !occluded
}

fn window_shown_again_takes_a_held_press(was_hidden: bool, visible_now: bool) -> bool {
    was_hidden && visible_now
}

fn native_window_lifecycle_event(event: &WindowEvent) -> Option<&'static str> {
    Some(match event {
        WindowEvent::Occluded(true) => "occluded",
        WindowEvent::Occluded(false) => "unoccluded",
        WindowEvent::Focused(true) => "focused",
        WindowEvent::Focused(false) => "unfocused",
        WindowEvent::Moved(_) => "moved",
        WindowEvent::SurfaceResized(_) => "resized",
        WindowEvent::ScaleFactorChanged { .. } => "rescaled",
        WindowEvent::PointerEntered { .. } => "pointer-entered",
        WindowEvent::PointerLeft { .. } => "pointer-left",
        WindowEvent::CloseRequested => "close-requested",
        _ => return None,
    })
}

fn native_window_redraw_held_while_hidden(visible: bool) -> bool {
    !visible
}

fn pointer_icon_is_owed_again(event: &WindowEvent) -> bool {
    matches!(
        event,
        WindowEvent::Focused(true) | WindowEvent::PointerEntered { .. }
    )
}

fn restore_pointer_icon_the_window_system_drew_over(
    app: Option<&AppShell<WgpuRenderer>>,
    event: &WindowEvent,
) {
    if let Some(app) = app.filter(|_| pointer_icon_is_owed_again(event)) {
        app.refresh_pointer_icon();
    }
}

fn primary_window_focus_changed(app: &mut AppShell<WgpuRenderer>, focused: bool) {
    if focused {
        app.set_active_root(RootId::Primary);
        app.refresh_pointer_icon();
    } else {
        cancel_app_input(app);
    }
}

fn native_window_focused(app: &mut AppShell<WgpuRenderer>, native: &NativeWindowSurface) {
    app.set_active_root(native.root_id());
    if let Some(surface) = native_surface(app, native) {
        surface.refresh_pointer_icon();
    }
}

fn deliver_native_surface_event(
    app: &mut AppShell<WgpuRenderer>,
    native: &NativeWindowSurface,
    current_modifiers: winit::keyboard::ModifiersState,
    event: WindowEvent,
) -> Option<WindowEvent> {
    let Some(mut surface) = native_surface(app, native) else {
        return Some(event);
    };
    match event {
        WindowEvent::KeyboardInput { event, .. } => {
            dispatch_keyboard_input(&mut surface, current_modifiers, event);
        }
        WindowEvent::Ime(ime_event) => dispatch_ime_event(&mut surface, ime_event),
        WindowEvent::Focused(false) if native.active_drag.is_none() => {
            cancel_surface_input(&mut surface);
        }
        WindowEvent::PointerLeft { .. } if native.active_drag.is_none() => {
            surface.cancel_gesture_unless_pressed();
        }
        event => return Some(event),
    }
    None
}

fn present_native_frame_owed_while_hidden(native: &mut NativeWindowSurface, event: &WindowEvent) {
    if let WindowEvent::Occluded(occluded) = event
        && occlusion_leaves_a_frame_owed(*occluded)
    {
        native.surface_dirty = true;
        native.window.request_redraw();
    }
}

fn present_primary_frame_owed_while_hidden(
    event: &WindowEvent,
    window: &Arc<dyn Window>,
    surface_dirty: &mut bool,
    redraw_pending: &mut bool,
) {
    if let WindowEvent::Occluded(occluded) = event
        && occlusion_leaves_a_frame_owed(*occluded)
    {
        *surface_dirty = true;
        request_redraw_once(window, redraw_pending);
    }
}

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
    cranpose_services::publish_host_surface_size(
        cranpose_services::host_surface::HostSurfaceSize {
            width: logical_width,
            height: logical_height,
            scale: if logical_width > 0.0 {
                width as f32 / logical_width
            } else {
                1.0
            },
        },
    );
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

#[allow(clippy::too_many_arguments)]
fn apply_primary_surface_resize(
    app: &mut AppShell<WgpuRenderer>,
    surface: &wgpu::Surface<'static>,
    surface_config: &mut wgpu::SurfaceConfiguration,
    window: &Arc<dyn Window>,
    primary_viewport_override: Option<(f32, f32)>,
    width: u32,
    height: u32,
    primary_surface_dirty: &mut bool,
    primary_redraw_pending: &mut bool,
) {
    if width == 0 || height == 0 {
        return;
    }
    let viewport = viewport_for_surface_size(
        primary_viewport_override,
        width,
        height,
        window.scale_factor(),
    );
    configure_app_surface_size(app, surface, surface_config, width, height, viewport);
    *primary_surface_dirty = true;
    request_redraw_once(window, primary_redraw_pending);
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
    surface: &mut SurfaceMut<'_, WgpuRenderer>,
    platform: &DesktopWinitPlatform,
    current_modifiers: winit::keyboard::ModifiersState,
    cursor_position: Option<(f32, f32)>,
    delta: winit::event::MouseScrollDelta,
) -> bool {
    let cursor_dirty = if let Some((x, y)) = cursor_position {
        surface.set_cursor(x, y)
    } else {
        false
    };

    let wheel = crate::winit_wheel::wheel_scroll_from_winit(
        platform.scroll_delta(delta),
        current_modifiers,
        wheel_uptime_millis(),
    );
    let scroll_dirty = surface.wheel_scrolled(wheel);
    cursor_dirty || scroll_dirty
}

fn wheel_uptime_millis() -> u64 {
    use std::sync::OnceLock;
    static EPOCH: OnceLock<web_time::Instant> = OnceLock::new();
    EPOCH
        .get_or_init(web_time::Instant::now)
        .elapsed()
        .as_millis() as u64
}

fn dispatch_middle_click_paste(
    surface: &mut SurfaceMut<'_, WgpuRenderer>,
    cursor_position: Option<(f32, f32)>,
) {
    if let Some((x, y)) = cursor_position {
        surface.set_cursor(x, y);
    }
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    if let Some(text) = surface.shell().get_primary_selection() {
        surface.on_paste(&text);
    }
}

fn cancel_app_input(app: &mut AppShell<WgpuRenderer>) {
    cancel_surface_input(&mut app.primary());
}

fn cancel_surface_input(surface: &mut SurfaceMut<'_, WgpuRenderer>) {
    surface.cancel_gesture();
    let _ = surface.on_ime_preedit("", None);
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

fn tell_the_application_the_window_frame(state: Option<WindowState>, window: &Arc<dyn Window>) {
    let Some(state) = state else {
        return;
    };
    let scale = window.scale_factor() as f32;
    if scale <= 0.0 {
        return;
    }
    let outer = window.outer_size();
    state.set_frame_size(cranpose_ui::Size::new(
        outer.width as f32 / scale,
        outer.height as f32 / scale,
    ));
}

fn tell_the_application_the_window_moved(
    events: &NativeWindowEvents,
    state: Option<WindowState>,
    x: f32,
    y: f32,
) {
    let already_knows = state
        .and_then(WindowState::position_non_reactive)
        .is_some_and(|known| (known.x - x).abs() <= 0.5 && (known.y - y).abs() <= 0.5);
    if already_knows {
        return;
    }
    notify_native_window_moved(events, x, y);
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

fn dispatch_ime_event(surface: &mut SurfaceMut<'_, WgpuRenderer>, ime_event: winit::event::Ime) {
    use winit::event::Ime;

    match ime_event {
        Ime::Preedit(text, cursor) => {
            surface.on_ime_preedit(&text, cursor);
        }
        Ime::Commit(text) => {
            let _ = surface.on_ime_preedit("", None);
            surface.on_paste(&text);
        }
        Ime::Enabled => {}
        Ime::Disabled => {
            surface.on_ime_preedit("", None);
        }
        Ime::DeleteSurrounding {
            before_bytes,
            after_bytes,
        } => {
            let _ = surface.on_ime_delete_surrounding(before_bytes, after_bytes);
        }
    }
}

struct DesktopTextInput {
    window: std::sync::Weak<dyn Window>,
}

impl DesktopTextInput {
    fn install(surface: &mut SurfaceMut<'_, WgpuRenderer>, window: &Arc<dyn Window>) {
        surface.set_platform_text_input(Rc::new(DesktopTextInput {
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
        let Some(enable) = ImeEnableRequest::new(ImeCapabilities::new(), ImeRequestData::default())
        else {
            return;
        };
        match window.request_ime_update(ImeRequest::Enable(enable)) {
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
        if self.exiting {
            return;
        }
        #[cfg(feature = "robot")]
        if self
            .robot_controller
            .as_mut()
            .is_some_and(RobotController::stage_pending_command)
        {
            self.about_to_wait(event_loop);
            return;
        }
        self.handle_primary_frame_requested(event_loop);
    }

    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let initial_width = self.settings.initial_width;
        let initial_height = self.settings.initial_height;
        let headless = self.settings.headless;

        let window: Arc<dyn Window> = match event_loop.create_window(
            WindowAttributes::default()
                .with_title(self.settings.window_title.clone())
                .with_surface_size(LogicalSize::new(
                    initial_width as f64,
                    initial_height as f64,
                ))
                .with_visible(false),
        ) {
            Ok(window) => window.into(),
            Err(error) => {
                self.abort_launch(event_loop, LaunchError::WindowCreate(error));
                return;
            }
        };

        let (instance, surface, adapter) =
            match crate::wgpu_surface::create_wgpu_surface_and_adapter(&window) {
                Ok(triple) => triple,
                Err(error) => {
                    self.abort_launch(event_loop, error);
                    return;
                }
            };
        let adapter_info = adapter.get_info();
        self.vsync_interval = monitor_refresh_interval(&window);

        let (device, queue) =
            match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("Main Device"),
                required_features: cranpose_render_wgpu::optional_device_features(&adapter),
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

        let present_mode = desktop_present_mode(&surface_caps, self.frame_pacing_mode());
        log_desktop_present_mode(self.frame_pacing_mode(), present_mode, &surface_caps);
        let surface_config = match surface_config_for_window(
            &surface_caps,
            surface_format,
            size.width.max(1),
            size.height.max(1),
            present_mode,
            false,
            desired_frame_latency(self.frame_pacing_mode(), self.vsync_interval),
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
        present_initial_placeholder_frame(
            &surface,
            &device,
            &queue,
            surface_format,
            "primary window initial present",
        );

        let text_system = WgpuTextSystem::from_font_set(self.settings.resolve_font_set());
        let initial_scale = window.scale_factor();
        let renderer = wgpu_renderer_for_surface(
            text_system.clone(),
            Arc::clone(&device),
            Arc::clone(&queue),
            crate::surface_format::display_surface_view_format(surface_format),
            adapter_info.backend,
            adapter.get_downlevel_capabilities().flags,
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
        app.set_semantics_enabled(true);
        crate::accessibility::install_inspector(&mut app, self.settings.developer_inspector);

        let mut accessibility = crate::desktop_accessibility::DesktopAccessibilityBridge::new(
            window.as_ref(),
            self.event_proxy.clone(),
            self.robot_drives(),
        );
        accessibility.sync(&mut app);
        self.primary_wrap_size = wrap_primary_window_to_content(
            window.as_ref(),
            &mut app,
            self.settings.primary_wraps_content,
            self.primary_wrap_size,
        )
        .or(self.primary_wrap_size);
        show_primary_when_it_has_content(&self.primary_shown, &mut app, window.as_ref(), headless);

        let mut dev_options = self.settings.dev_options.clone();
        dev_options.frame_pacing_mode = self.frame_pacing_mode();
        app.set_dev_options(dev_options);

        DesktopTextInput::install(&mut app.primary(), &window);
        app.set_screen_origin(native_window_surface_origin(
            &self.native_window_platform_probe,
            &window,
        ));

        let frame_waker_window = window.clone();
        let frame_waker_event_proxy = self.event_proxy.clone();
        let frame_waker_shown = Arc::clone(&self.primary_shown);
        app.set_frame_waker(move || {
            let shown = frame_waker_shown.load(std::sync::atomic::Ordering::Relaxed);
            if primary_frame_waker_uses_event_proxy(shown, headless) {
                frame_waker_event_proxy.wake_up();
            } else {
                frame_waker_window.request_redraw();
            }
        });

        let mut platform = DesktopWinitPlatform::default();
        platform.set_scale_factor(initial_scale);
        let request_initial_redraw =
            primary_launch_requires_initial_redraw(self.primary_visible(), self.settings.headless);

        if let Some(theme) = window.theme() {
            self.platform_env
                .set_system_theme(system_theme_from_winit(theme));
        }

        self.window = Some(window);
        self.surface = Some(surface);
        self.surface_config = Some(surface_config);
        self.surface_caps = Some(surface_caps);
        self.app = Some(app);
        self.accessibility = Some(accessibility);
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
        if request_initial_redraw && let Some(window) = self.window.clone() {
            request_redraw_once(&window, &mut self.primary_redraw_pending);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: WinitWindowId,
        event: WindowEvent,
    ) {
        self.sync_frame_pacing();
        if self.primary_window_id() != Some(window_id) {
            self.dispatch_native_window_event(event_loop, window_id, event);
            return;
        }
        self.relay_primary_held_press(event_loop, &event);
        let Some(window) = &self.window else {
            return;
        };
        if let Some(accessibility) = &mut self.accessibility {
            accessibility.process_event(window.as_ref(), &event);
        }

        let frame_interval = self.frame_interval();
        let last_frame_start_time = self.last_frame_start_time;
        let frame_cap_deadline = last_frame_start_time
            .and_then(|started_at| frame_interval.map(|interval| started_at + interval));

        let primary_viewport_override = headless_requested_viewport(&self.settings);

        let registry = Rc::clone(&self.native_window_registry);
        let Some(app) = &mut self.app else { return };
        if let Some(accessibility) = &mut self.accessibility {
            for (x, y) in accessibility.drain_clicks() {
                app.accessibility_activate_at(x, y);
            }
            accessibility.run_custom_actions(app);
            accessibility.run_value_requests(app);
            accessibility.run_scroll_requests(app);
            accessibility.run_focus_requests(app);
        }
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
                if let Some(recorder) = self.recorder.take()
                    && let Err(e) = recorder.finish()
                {
                    eprintln!("[Recorder] Error saving recording: {}", e);
                }
                self.exiting = true;
                event_loop.exit();
            }
            WindowEvent::DragDropped { ref paths, .. } => {
                for path in paths {
                    crate::desktop_incoming::publish_file(path);
                }
            }
            WindowEvent::SurfaceResized(new_size) => {
                apply_primary_surface_resize(
                    app,
                    surface,
                    surface_config,
                    window,
                    primary_viewport_override,
                    new_size.width,
                    new_size.height,
                    &mut self.primary_surface_dirty,
                    &mut self.primary_redraw_pending,
                );
            }
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                mut surface_size_writer,
            } => {
                update_app_scale_factor(app, platform, scale_factor);

                let new_size = window.surface_size();
                let _ = surface_size_writer.request_surface_size(new_size);
                apply_primary_surface_resize(
                    app,
                    surface,
                    surface_config,
                    window,
                    primary_viewport_override,
                    new_size.width,
                    new_size.height,
                    &mut self.primary_surface_dirty,
                    &mut self.primary_redraw_pending,
                );
            }
            WindowEvent::Moved(_) => {
                self.vsync_interval = monitor_refresh_interval(window);
                app.set_screen_origin(native_window_surface_origin(
                    &self.native_window_platform_probe,
                    window,
                ));
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
                let event_time = app.realtime_pointer_event_time(None);
                if app.set_cursor_at_event_time(logical.x, logical.y, event_time) {
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
                self.current_modifiers = modifiers.state();
                app.set_modifiers(app_modifiers(self.current_modifiers));
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
                    &mut app.primary(),
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
                position,
                button,
                ..
            } if is_primary_pointer_button(&button) => {
                let source = pointer_source_from_button(&button);
                app.set_pointer_source(source);
                let logical = platform.pointer_position(position);
                self.last_cursor_position = Some((logical.x, logical.y));
                let event_time = app.realtime_pointer_event_time(None);
                log::trace!(
                    target: "cranpose::input",
                    "desktop pointer button {:?} at ({:.2},{:.2})",
                    state,
                    logical.x,
                    logical.y
                );
                if app.set_cursor_at_event_time(logical.x, logical.y, event_time) {
                    request_redraw_once(window, &mut self.primary_redraw_pending);
                }
                match state {
                    ElementState::Pressed => {
                        let request = pointer_button_frame_request(
                            app.pointer_pressed_at_event_time(event_time),
                        );
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
                        let released = if source.is_touch_like() {
                            app.pointer_released_at_position_event_time(
                                logical.x, logical.y, event_time,
                            )
                        } else {
                            app.pointer_released_at_event_time(event_time)
                        };
                        let request = pointer_button_frame_request(released);
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
                dispatch_middle_click_paste(&mut app.primary(), self.last_cursor_position);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                dispatch_keyboard_input(&mut app.primary(), self.current_modifiers, event);
            }
            WindowEvent::Focused(focused) => {
                primary_window_focus_changed(app, focused);
            }
            WindowEvent::Ime(ime_event) => {
                dispatch_ime_event(&mut app.primary(), ime_event);
            }
            WindowEvent::PointerLeft { .. } => {
                app.cancel_gesture_unless_pressed();
            }
            WindowEvent::RedrawRequested => {
                record_pacing_event(|diag| &mut diag.redraw_events);
                self.primary_redraw_pending = false;
                if let Some(deadline) = frame_cap_deadline
                    && deadline > Instant::now()
                {
                    event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
                    return;
                }
                log::trace!(target: "cranpose::input", "desktop redraw requested");
                let frame_started_at = Instant::now();
                #[cfg(feature = "robot")]
                let robot_surface_dirty_before_update = self.robot_visible_surface_dirty;
                #[cfg(not(feature = "robot"))]
                let robot_surface_dirty_before_update = false;
                let primary_surface_dirty_before_update = self.primary_surface_dirty;
                app.set_density(window.scale_factor() as f32);
                #[cfg_attr(not(feature = "robot"), allow(unused_variables))]
                let update_result = update_app_with_native_window_registry(app, &registry);
                wrap_primary_window_for_frame(
                    app,
                    surface,
                    surface_config,
                    window,
                    &registry,
                    primary_viewport_override,
                    self.settings.primary_wraps_content,
                    &mut self.primary_wrap_size,
                );
                if let Some(accessibility) = &mut self.accessibility {
                    accessibility.sync(app);
                }
                #[cfg(feature = "robot")]
                if let Some(controller) = &mut self.robot_controller {
                    controller.record_idle_update_result(update_result);
                }
                let after_update = Instant::now();
                sync_native_windows_after_event = true;

                let frame_owed = app.take_frame_owed();
                let present_required = surface_present_required(
                    primary_surface_dirty_before_update || robot_surface_dirty_before_update,
                    frame_owed,
                    app.needs_redraw(),
                );
                pace_after_empty_redraw(
                    &mut self.last_frame_start_time,
                    frame_started_at,
                    present_required,
                );
                if present_required {
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
                            #[cfg(feature = "robot")]
                            {
                                self.unpresentable_frames_since_present =
                                    self.unpresentable_frames_since_present.saturating_add(1);
                            }
                            return;
                        }
                    };
                    let after_acquire = Instant::now();

                    let view = output.texture.create_view(&wgpu::TextureViewDescriptor {
                        format: Some(crate::surface_format::display_surface_view_format(
                            surface_config.format,
                        )),
                        ..Default::default()
                    });

                    if let Err(err) = app.renderer().render_surface_texture(
                        &output.texture,
                        &view,
                        surface_config.width,
                        surface_config.height,
                    ) {
                        log::error!("render failed: {err:?}");
                        return;
                    }
                    let after_render = Instant::now();

                    window.pre_present_notify();
                    output.present();
                    let after_present = Instant::now();
                    record_pacing_event(|diag| &mut diag.presents);
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
                    self.last_frame_start_time = Some(next_frame_anchor(
                        self.last_frame_start_time,
                        frame_started_at,
                        frame_interval,
                    ));
                    #[cfg(feature = "robot")]
                    {
                        self.presented_frame_generation =
                            self.presented_frame_generation.saturating_add(1);
                        self.unpresentable_frames_since_present = 0;
                        self.robot_visible_surface_dirty = false;
                    }
                    #[cfg(feature = "robot")]
                    let robot_driven = self.robot_controller.is_some();
                    #[cfg(not(feature = "robot"))]
                    let robot_driven = false;
                    if !robot_driven
                        && should_chain_no_vsync_redraw(
                            frame_interval,
                            app.frame_schedule().needs_frame,
                        )
                    {
                        request_redraw_once(window, &mut self.primary_redraw_pending);
                    }
                } else {
                    record_pacing_event(|diag| &mut diag.skipped_no_present);
                    #[cfg(feature = "robot")]
                    {
                        self.robot_visible_surface_dirty = app.needs_redraw();
                    }
                }
            }
            event => {
                restore_pointer_icon_the_window_system_drew_over(self.app.as_ref(), &event);
                present_primary_frame_owed_while_hidden(
                    &event,
                    window,
                    &mut self.primary_surface_dirty,
                    &mut self.primary_redraw_pending,
                );
            }
        }

        if sync_native_windows_after_event {
            self.sync_native_windows(event_loop);
        }
    }

    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        if cranpose_services::take_exit_request() {
            self.exiting = true;
            event_loop.exit();
            return;
        }
        if let Some((width, height)) = crate::desktop_host_surface::take_requested_size()
            && let Some(window) = self.window.as_ref()
        {
            let _ = window.request_surface_size(
                winit::dpi::LogicalSize::new(width as f64, height as f64).into(),
            );
        }
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

        self.sync_frame_pacing();
        self.sync_pointer_icons(event_loop);

        let last_frame_start_time = self.last_frame_start_time;
        let registry = Rc::clone(&self.native_window_registry);
        let primary_visible = self.primary_visible();
        let Some(app) = &mut self.app else { return };
        let Some(window) = self.window.clone() else {
            return;
        };
        if let Some(accessibility) = &mut self.accessibility {
            let mut activated = false;
            for (x, y) in accessibility.drain_clicks() {
                app.accessibility_activate_at(x, y);
                activated = true;
            }
            activated |= accessibility.run_custom_actions(app);
            activated |= accessibility.run_value_requests(app);
            activated |= accessibility.run_scroll_requests(app);
            activated |= accessibility.run_focus_requests(app);
            if activated {
                request_redraw_once(&window, &mut self.primary_redraw_pending);
            }
        }

        if initial_present_redraw_needed(
            self.primary_initial_present_pending,
            self.primary_redraw_pending,
        ) {
            request_redraw_once(&window, &mut self.primary_redraw_pending);
        }

        #[cfg(feature = "robot")]
        if let Some(controller) = &mut self.robot_controller {
            let mut robot_visual_dirty = false;
            while let Some(cmd) = controller.next_command() {
                match cmd {
                    RobotCommand::Click { x, y } => {
                        app.set_pointer_source(PointerSource::Mouse);
                        let event_time = app.realtime_pointer_event_time(None);
                        let cursor_dirty = app.set_cursor_at_event_time(x, y, event_time);
                        controller.begin_synthetic_primary_gesture();
                        let press_dirty = app.pointer_pressed_at_event_time(event_time);
                        let release_dirty = app.pointer_released_at_event_time(event_time);
                        controller.end_synthetic_primary_gesture();
                        robot_visual_dirty |= cursor_dirty || press_dirty || release_dirty;
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::MoveTo { x, y } => {
                        let event_time = app.realtime_pointer_event_time(None);
                        robot_visual_dirty |= app.set_cursor_at_event_time(x, y, event_time);
                        if let Some(recorder) = &mut self.recorder {
                            recorder.record_mouse_move(x, y);
                        }
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::MouseDown => {
                        let event_time = app.realtime_pointer_event_time(None);
                        controller.begin_synthetic_primary_gesture();
                        robot_visual_dirty |= app.pointer_pressed_at_event_time(event_time);
                        if let Some(recorder) = &mut self.recorder {
                            recorder.record_mouse_down();
                        }
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::MouseUp => {
                        let event_time = app.realtime_pointer_event_time(None);
                        robot_visual_dirty |= app.pointer_released_at_event_time(event_time);
                        controller.end_synthetic_primary_gesture();
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
                        let Some(target) = robot_scroll_present_target(
                            app,
                            delta_x,
                            delta_y,
                            primary_visible,
                            self.settings.headless,
                            self.presented_frame_generation,
                        ) else {
                            let _ = controller.tx.send(RobotResponse::Ok);
                            continue;
                        };
                        robot_visual_dirty |= target.is_some();
                        robot_present_or_update(
                            controller,
                            &window,
                            app,
                            &registry,
                            &mut self.last_frame_start_time,
                            &mut self.primary_redraw_pending,
                            &mut self.robot_visible_surface_dirty,
                            target,
                        );
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
                        let Some(target) = robot_scroll_present_target(
                            app,
                            delta_x,
                            delta_y,
                            primary_visible,
                            self.settings.headless,
                            self.presented_frame_generation,
                        ) else {
                            let _ = controller.tx.send(RobotResponse::Ok);
                            continue;
                        };
                        if target.is_some() {
                            controller.scroll_sequence = Some(RobotScrollSequence {
                                delta_x,
                                delta_y,
                                remaining: count.saturating_sub(1),
                            });
                            robot_visual_dirty = true;
                        }
                        robot_present_or_update(
                            controller,
                            &window,
                            app,
                            &registry,
                            &mut self.last_frame_start_time,
                            &mut self.primary_redraw_pending,
                            &mut self.robot_visible_surface_dirty,
                            target,
                        );
                    }

                    RobotCommand::TouchDown { x, y, source } => {
                        app.set_pointer_source(source);
                        let event_time = app.realtime_pointer_event_time(None);
                        let cursor_dirty = app.set_cursor_at_event_time(x, y, event_time);
                        controller.begin_synthetic_primary_gesture();
                        let press_dirty = app.pointer_pressed_at_event_time(event_time);
                        robot_visual_dirty |= cursor_dirty || press_dirty;
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::TouchMove { x, y, source } => {
                        app.set_pointer_source(source);
                        let event_time = app.realtime_pointer_event_time(None);
                        robot_visual_dirty |= app.set_cursor_at_event_time(x, y, event_time);
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::TouchMoveAndWaitForFrame { x, y, source } => {
                        app.set_pointer_source(source);
                        let event_time = app.realtime_pointer_event_time(None);
                        let visual_dirty = app.set_cursor_at_event_time(x, y, event_time);
                        if visual_dirty {
                            robot_visual_dirty = true;
                        }
                        let present_target = visual_dirty
                            .then(|| {
                                robot_visible_pump_present_target(
                                    primary_visible,
                                    self.settings.headless,
                                    1,
                                    self.presented_frame_generation,
                                )
                            })
                            .flatten();
                        robot_present_or_update(
                            controller,
                            &window,
                            app,
                            &registry,
                            &mut self.last_frame_start_time,
                            &mut self.primary_redraw_pending,
                            &mut self.robot_visible_surface_dirty,
                            present_target,
                        );
                    }
                    RobotCommand::TouchUp { x, y, source } => {
                        app.set_pointer_source(source);
                        let event_time = app.realtime_pointer_event_time(None);
                        let cursor_dirty = app.set_cursor_at_event_time(x, y, event_time);
                        let release_dirty = app.pointer_released_at_event_time(event_time);
                        controller.end_synthetic_primary_gesture();
                        robot_visual_dirty |= cursor_dirty || release_dirty;
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    command @ (RobotCommand::GetSemantics
                    | RobotCommand::GetSpokenTree
                    | RobotCommand::GetInspectorState
                    | RobotCommand::AuditAccessibility) => {
                        let update_result = pump_robot_frame(app, &registry);
                        robot_visual_dirty |=
                            robot_query_visual_dirty(update_result, app.needs_redraw());
                        let _ = controller.tx.send(robot_tree_response(app, &command));
                    }
                    RobotCommand::FindText { text, match_kind } => {
                        let update_result = pump_robot_frame(app, &registry);
                        robot_visual_dirty |=
                            robot_query_visual_dirty(update_result, app.needs_redraw());
                        let result = find_text_in_app(app, &text, match_kind);
                        let _ = controller.tx.send(RobotResponse::SemanticQuery(result));
                    }
                    RobotCommand::FindButton { text, match_kind } => {
                        let update_result = pump_robot_frame(app, &registry);
                        robot_visual_dirty |=
                            robot_query_visual_dirty(update_result, app.needs_redraw());
                        let result = find_button_in_app(app, &text, match_kind);
                        let _ = controller.tx.send(RobotResponse::SemanticQuery(result));
                    }
                    RobotCommand::GetScreenshot => {
                        let update_result = pump_robot_frame(app, &registry);
                        robot_visual_dirty |=
                            robot_query_visual_dirty(update_result, app.needs_redraw());
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
                        let update_result = pump_robot_frame(app, &registry);
                        robot_visual_dirty |=
                            robot_query_visual_dirty(update_result, app.needs_redraw());
                        match capture_screenshot_with_scale(app, scale) {
                            Ok(screenshot) => {
                                let _ = controller.tx.send(RobotResponse::Screenshot(screenshot));
                            }
                            Err(err) => {
                                let _ = controller.tx.send(RobotResponse::Error(err));
                            }
                        }
                    }
                    RobotCommand::CaptureKeyframes { scale, steps } => {
                        let mut shots = Vec::new();
                        let mut capture_err = None;
                        for (advance_ms, capture) in steps {
                            native_window::with_native_window_registry(&registry, || {
                                app.update_after_exact_interval(Duration::from_secs_f64(
                                    f64::from(advance_ms.max(0.0)) / 1000.0,
                                ))
                            });
                            if capture {
                                match capture_screenshot_with_scale(app, scale) {
                                    Ok(shot) => shots.push(shot),
                                    Err(err) => {
                                        capture_err = Some(err);
                                        break;
                                    }
                                }
                            }
                        }
                        let _ = controller.tx.send(match capture_err {
                            Some(err) => RobotResponse::Error(err),
                            None => RobotResponse::Screenshots(shots),
                        });
                    }
                    RobotCommand::CaptureInteractionKeyframes { scale, steps } => {
                        let mut shots = Vec::new();
                        let mut capture_err = None;
                        for step in steps {
                            native_window::with_native_window_registry(&registry, || {
                                app.update_after_exact_interval(Duration::from_secs_f64(
                                    f64::from(step.advance_ms.max(0.0)) / 1000.0,
                                ))
                            });
                            for action in step.actions {
                                let action_result = match action {
                                    RobotTimelineAction::MoveTo { x, y } => {
                                        let event_time = app.exact_pointer_event_time(None);
                                        app.set_cursor_at_event_time(x, y, event_time);
                                        Ok(())
                                    }
                                    RobotTimelineAction::MouseDown => {
                                        let event_time = app.exact_pointer_event_time(None);
                                        controller.begin_synthetic_primary_gesture();
                                        app.pointer_pressed_at_event_time(event_time);
                                        Ok(())
                                    }
                                    RobotTimelineAction::MouseUp => {
                                        let event_time = app.exact_pointer_event_time(None);
                                        app.pointer_released_at_event_time(event_time);
                                        controller.end_synthetic_primary_gesture();
                                        Ok(())
                                    }
                                    RobotTimelineAction::InvokeAppHook { name, argument } => {
                                        self.robot_app_hook.as_mut().map_or_else(
                                            || Err("robot app hook not configured".to_string()),
                                            |hook| {
                                                app.debug_enter_app_context(|| hook(name, argument))
                                                    .map(|_| ())
                                            },
                                        )
                                    }
                                };
                                if let Err(err) = action_result {
                                    capture_err = Some(err);
                                    break;
                                }
                            }
                            if capture_err.is_some() {
                                break;
                            }
                            native_window::with_native_window_registry(&registry, || {
                                app.update_after_exact_interval(Duration::ZERO)
                            });
                            if step.capture {
                                match capture_screenshot_with_scale(app, scale) {
                                    Ok(shot) => shots.push(shot),
                                    Err(err) => {
                                        capture_err = Some(err);
                                        break;
                                    }
                                }
                            }
                        }
                        robot_visual_dirty = true;
                        let _ = controller.tx.send(match capture_err {
                            Some(err) => RobotResponse::Error(err),
                            None => RobotResponse::Screenshots(shots),
                        });
                    }
                    RobotCommand::GetRenderStats
                    | RobotCommand::GetFpsStats
                    | RobotCommand::GetPresentationInfo => {
                        let response = robot_frame_diagnostics(
                            &cmd,
                            app,
                            self.surface_config.as_ref(),
                            self.surface_caps.as_ref(),
                            self.vsync_interval,
                        );
                        let _ = controller.tx.send(response);
                    }
                    RobotCommand::GetPacingControlCenter(mode) => {
                        let center = app.dev_overlay_control_center(mode);
                        let _ = controller
                            .tx
                            .send(RobotResponse::PacingControlCenter(center));
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
                    RobotCommand::GetLiveUiTaskLabels => {
                        let _ = controller.tx.send(RobotResponse::LiveUiTaskLabels(
                            app.runtime_handle().live_ui_task_labels(),
                        ));
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
                            Some(hook) => app
                                .debug_enter_app_context(|| hook(name, argument))
                                .map(RobotResponse::AppHookResult),
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

                        for ch in text.chars() {
                            let key_code = char_to_key_code(ch);
                            let key_event = KeyEvent::new(
                                key_code,
                                ch.to_string(),
                                Modifiers::NONE,
                                KeyEventType::KeyDown,
                            );
                            app.on_key_event(&key_event);
                        }
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
                            primary_visible,
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
                                    primary_visible,
                                    self.settings.headless,
                                    count,
                                    self.presented_frame_generation,
                                )
                            })
                            .flatten();
                        robot_present_or_ack(
                            controller,
                            &window,
                            &mut self.last_frame_start_time,
                            &mut self.primary_redraw_pending,
                            present_target,
                        );
                    }
                    RobotCommand::WaitForPresentFrame => {
                        let mut visual_frame_pending =
                            self.robot_visible_surface_dirty || app.needs_redraw();
                        if !visual_frame_pending {
                            let update_result =
                                update_app_with_native_window_registry(app, &registry);
                            visual_frame_pending =
                                app.needs_redraw() || update_result.visual_changed;
                            self.robot_visible_surface_dirty = visual_frame_pending;
                        }
                        let present_target = visual_frame_pending
                            .then(|| {
                                robot_visible_pump_present_target(
                                    primary_visible,
                                    self.settings.headless,
                                    1,
                                    self.presented_frame_generation,
                                )
                            })
                            .flatten();
                        robot_present_or_ack(
                            controller,
                            &window,
                            &mut self.last_frame_start_time,
                            &mut self.primary_redraw_pending,
                            present_target,
                        );
                    }
                    RobotCommand::Exit => {
                        let _ = controller.tx.send(RobotResponse::Ok);
                        self.exiting = true;
                        event_loop.exit();
                        return;
                    }
                }
            }
            if robot_visual_dirty {
                self.robot_visible_surface_dirty = true;
                if primary_surface_redraw_drives_app(primary_visible, self.settings.headless) {
                    self.last_frame_start_time = None;
                    request_redraw_once(&window, &mut self.primary_redraw_pending);
                }
            }

            if let Some(target_generation) = controller.waiting_for_pump_present_generation {
                if self.presented_frame_generation >= target_generation {
                    if let Some(mut sequence) = controller.scroll_sequence.take() {
                        if sequence.remaining == 0 {
                            controller.finish_pump_present_wait();
                            self.robot_visible_surface_dirty = app.needs_redraw();
                            let _ = controller.tx.send(RobotResponse::Ok);
                        } else if app.pointer_scrolled(sequence.delta_x, sequence.delta_y) {
                            sequence.remaining = sequence.remaining.saturating_sub(1);
                            controller.scroll_sequence = Some(sequence);
                            controller.begin_pump_present_wait(
                                self.presented_frame_generation.saturating_add(1),
                            );
                            self.robot_visible_surface_dirty = true;
                            self.last_frame_start_time = None;
                            request_redraw_once(&window, &mut self.primary_redraw_pending);
                        } else {
                            controller.finish_pump_present_wait();
                            self.robot_visible_surface_dirty = app.needs_redraw();
                            let _ = controller.tx.send(RobotResponse::Ok);
                        }
                    } else {
                        controller.finish_pump_present_wait();
                        self.robot_visible_surface_dirty = app.needs_redraw();
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                } else if controller.pump_present_wait_timed_out(now) {
                    controller.finish_pump_present_wait();
                    controller.scroll_sequence = None;
                    self.robot_visible_surface_dirty = app.needs_redraw();
                    let _ = controller.tx.send(RobotResponse::Error(format!(
                        "present wait: the window surface refused {} consecutive frames over {:?} \
                         and never reached generation {target_generation} (currently {}); the \
                         window is occluded, off-screen, or on a display that is not compositing",
                        self.unpresentable_frames_since_present,
                        ROBOT_PRESENT_WAIT_TIMEOUT,
                        self.presented_frame_generation,
                    )));
                } else {
                    request_redraw_once(&window, &mut self.primary_redraw_pending);
                }
            }

            if controller.waiting_for_idle {
                let frame_schedule = app.frame_schedule();
                let needs_update = frame_schedule.needs_update;
                let needs_frame = frame_schedule.needs_frame;
                let has_active_animations = app.has_active_animations();
                let has_transient_frame_callbacks = app.has_transient_frame_callbacks();
                let visible_redraw_pending = self.robot_visible_surface_dirty || app.needs_redraw();
                if controller.waiting_for_present_generation.is_none()
                    && self.primary_redraw_pending
                    && visible_redraw_pending
                {
                    controller.waiting_for_present_generation = robot_visible_present_target(
                        primary_visible,
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
                let frame_only = needs_frame
                    && !needs_update
                    && !has_transient_frame_callbacks
                    && !waiting_for_present;
                let animation_loop_only = robot_wait_for_idle_animation_loop_only(
                    has_active_animations,
                    has_transient_frame_callbacks,
                    waiting_for_present,
                    controller.idle_iterations,
                    controller.idle_structure_clean_frames,
                );
                if frame_only || animation_loop_only {
                    controller.finish_idle_wait();
                    self.robot_visible_surface_dirty = false;
                    let _ = controller.tx.send(RobotResponse::Ok);
                } else if !needs_frame && !has_transient_frame_callbacks && !waiting_for_present {
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
                                primary_visible,
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
                            primary_visible,
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

                    let present_is_sole_blocker = waiting_for_present
                        && !needs_update
                        && !has_active_animations
                        && !has_transient_frame_callbacks;
                    let timeout_check_at = Instant::now();
                    controller
                        .observe_idle_present_block(present_is_sole_blocker, timeout_check_at);
                    if let Some(timeout) = controller.idle_wait_timeout(timeout_check_at) {
                        let iterations = controller.idle_iterations;
                        let target_generation = controller.waiting_for_present_generation;
                        controller.finish_idle_wait();
                        let message = match timeout {
                            IdleWaitTimeout::AppNotConverging => format!(
                                "wait_for_idle: timed out after {ROBOT_IDLE_TIMEOUT:?} ({iterations} iterations); needs_update={needs_update}, needs_redraw={}, has_animations={has_active_animations}, waiting_for_present={waiting_for_present}",
                                app.needs_redraw(),
                            ),
                            IdleWaitTimeout::SurfaceNotPresenting => format!(
                                "wait_for_idle: the window surface refused {} consecutive frames over {:?} and never reached generation {target_generation:?} (currently {}); composition had already settled, so this is the compositor and not the application -- the window is occluded, off-screen, or on a display that is not compositing; needs_update={needs_update}, needs_redraw={}, has_animations={has_active_animations}, waiting_for_present={waiting_for_present}",
                                self.unpresentable_frames_since_present,
                                ROBOT_IDLE_PRESENT_TIMEOUT,
                                self.presented_frame_generation,
                                app.needs_redraw(),
                            ),
                            IdleWaitTimeout::HostStarved => format!(
                                "wait_for_idle: host starvation -- only {iterations} iterations ran in {:?} (need {ROBOT_IDLE_MIN_ITERATIONS} to trust the elapsed budget); the process was not scheduled enough to tell whether the application is converging, not an application defect; needs_update={needs_update}, needs_redraw={}, has_animations={has_active_animations}, waiting_for_present={waiting_for_present}",
                                ROBOT_IDLE_STARVATION_CEILING,
                                app.needs_redraw(),
                            ),
                        };
                        let _ = controller.tx.send(RobotResponse::Error(message));
                    }
                }
            }
        }

        let frame_interval = frame_interval_for_mode(app.frame_pacing_mode(), self.vsync_interval);
        let frame_schedule = app.frame_schedule();
        let has_active_animations = app.has_active_animations();
        let needs_update = frame_schedule.needs_update;
        let needs_redraw = frame_schedule.needs_frame || app.frame_owed();
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
            primary_visible,
            self.settings.headless,
            needs_redraw,
            waiting_for_frame_cap,
        );
        if needs_update && !needs_redraw && !waiting_for_frame_cap {
            record_pacing_event(|diag| &mut diag.updates);
            update_app_with_native_window_registry(app, &registry);
            if app.frame_owed() || app.needs_redraw() {
                request_redraw_once(&window, &mut self.primary_redraw_pending);
            }
        } else if needs_redraw && !waiting_for_frame_cap {
            if direct_declaration_update {
                trace_native_window!(
                    "primary declaration host direct update visible={} headless={}",
                    primary_visible,
                    self.settings.headless
                );
                record_pacing_event(|diag| &mut diag.direct_updates);
                update_declaration_host_frame(
                    app,
                    &registry,
                    &mut self.last_frame_start_time,
                    frame_interval,
                );
            } else {
                request_redraw_once(&window, &mut self.primary_redraw_pending);
            }
        }
        let primary_next_event_time = frame_schedule.next_deadline;
        self.sync_primary_size();
        self.sync_primary_visibility();
        if direct_declaration_update {
            self.sync_native_windows(event_loop);
        }

        let Some(app) = self.app.as_mut() else { return };
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
        let pacing_mode = app.frame_pacing_mode();
        for native in self.native_windows.values_mut() {
            if !native.options.visible {
                continue;
            }
            let Some(surface) = native_surface(app, native) else {
                continue;
            };

            let frame_schedule = surface.frame_schedule();
            let needs_redraw = native_surface_needs_frame(
                frame_schedule.needs_frame,
                surface.frame_owed(),
                surface.needs_redraw(),
                native.surface_dirty,
            );
            let next_frame_time = native.last_frame_start_time.and_then(|started_at| {
                native
                    .frame_interval(pacing_mode)
                    .map(|interval| started_at + interval)
            });
            let waiting_for_frame_cap =
                needs_redraw && next_frame_time.is_some_and(|deadline| deadline > now);

            if needs_redraw && !waiting_for_frame_cap {
                native.window.request_redraw();
            }
            if let Some(next_poll_at) = native.active_drag.and_then(|active_drag| {
                native_window_drag_poll_deadline(
                    active_drag.next_poll_at,
                    NATIVE_WINDOW_GLOBAL_POINTER_POLLED,
                )
            }) {
                native_drag_deadline = Some(
                    native_drag_deadline
                        .map(|current| current.min(next_poll_at))
                        .unwrap_or(next_poll_at),
                );
            }
            if waiting_for_frame_cap && let Some(deadline) = next_frame_time {
                native_frame_cap_deadline = Some(
                    native_frame_cap_deadline
                        .map(|current| current.min(deadline))
                        .unwrap_or(deadline),
                );
            }
            if let Some(next_time) = frame_schedule.next_deadline {
                native_next_event_time = Some(
                    native_next_event_time
                        .map(|current| current.min(next_time))
                        .unwrap_or(next_time),
                );
            }
        }

        #[cfg(feature = "robot")]
        let robot_needs_poll = self
            .robot_controller
            .as_mut()
            .is_some_and(RobotController::awaiting_progress);

        #[cfg(not(feature = "robot"))]
        let robot_needs_poll = false;

        let control_flow = event_loop_control_flow(
            now,
            LoopControlInputs {
                robot_needs_poll,
                free_running: free_running_frame(
                    frame_interval,
                    needs_redraw,
                    self.primary_redraw_pending,
                ),
                primary_pointer_polled,
                drag_poll_deadline: native_drag_deadline,
                position_poll_deadline: native_position_poll_deadline,
                frame_cap_deadline: [
                    next_frame_time.filter(|_| waiting_for_frame_cap),
                    native_frame_cap_deadline,
                ]
                .into_iter()
                .flatten()
                .min(),
                has_active_animations,
                next_event_time: [primary_next_event_time, native_next_event_time]
                    .into_iter()
                    .flatten()
                    .min(),
            },
        );

        #[cfg(feature = "robot")]
        let control_flow = bound_park_for_robot(control_flow, self.robot_controller.is_some(), now);

        record_pacing_event(match control_flow {
            ControlFlow::Poll => |diag: &mut PacingDiag| &mut diag.control_flow_poll,
            ControlFlow::WaitUntil(_) => |diag: &mut PacingDiag| &mut diag.control_flow_wait_until,
            ControlFlow::Wait => |diag: &mut PacingDiag| &mut diag.control_flow_wait,
        });
        event_loop.set_control_flow(control_flow);
    }
}

#[cfg(feature = "robot")]
const ROBOT_PARKED_COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(8);

#[cfg(feature = "robot")]
fn bound_park_for_robot(
    control_flow: ControlFlow,
    robot_attached: bool,
    now: Instant,
) -> ControlFlow {
    if !robot_attached {
        return control_flow;
    }
    let bound = now + ROBOT_PARKED_COMMAND_POLL_INTERVAL;
    match control_flow {
        ControlFlow::Poll => ControlFlow::Poll,
        ControlFlow::Wait => ControlFlow::WaitUntil(bound),
        ControlFlow::WaitUntil(deadline) => ControlFlow::WaitUntil(deadline.min(bound)),
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
    register_application_id(settings.application_id.as_deref());

    let event_loop = EventLoop::builder()
        .build()
        .map_err(LaunchError::EventLoopCreate)?;
    let event_proxy = event_loop.create_proxy();
    let launch_error = Rc::new(RefCell::new(None));

    #[cfg(feature = "robot")]
    let robot_controller = if let Some(driver) = settings.test_driver.take() {
        let wake_proxy = event_proxy.clone();
        let (controller, robot) = RobotController::new(move || wake_proxy.wake_up());
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

    let surface_wake = event_proxy.clone();
    crate::desktop_host_surface::install(move || surface_wake.wake_up());

    #[cfg(feature = "media")]
    cranpose_media::install();

    #[cfg(all(feature = "camera-desktop", target_os = "macos"))]
    crate::apple_camera::register();

    crate::desktop_power::register();

    crate::desktop_bundled_assets::register();

    cranpose_services::set_platform_app_updater(Arc::new(
        cranpose_services::GitHubAppUpdater::new(),
    ));

    crate::desktop_incoming::publish_launch_documents();

    #[cfg(unix)]
    crate::process_info::install();

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

fn register_application_id(configured: Option<&str>) {
    let derived;
    let application_id = match configured {
        Some(application_id) => application_id,
        None => {
            derived = std::env::current_exe()
                .ok()
                .and_then(|path| {
                    path.file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned())
                })
                .unwrap_or_else(|| "cranpose-app".to_string());
            derived.as_str()
        }
    };
    if let Err(error) = cranpose_services::set_application_id(application_id) {
        log::warn!("cranpose: `{application_id}` is not a usable application id: {error}");
        return;
    }
    crate::pipeline_cache_file::publish();
}

/// Runs a desktop application and exits the process on success.
///
/// Use [`try_run`] when the caller needs to handle launch failures explicitly.
#[allow(unused_mut)]
pub fn run(settings: AppSettings, content: impl FnMut() + 'static) -> ! {
    try_run(settings, content).unwrap_or_else(|error| {
        crate::app_launcher::exit_after_launch_error("desktop launch failed", error)
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
    resolve_robot_screenshot_params_with_scale(
        buffer_size,
        fallback_logical_size,
        robot_capture_scale_from_env(),
    )
}

#[cfg(any(test, feature = "robot"))]
fn robot_capture_scale_from_env() -> f32 {
    parse_robot_capture_scale(
        std::env::var("CRANPOSE_ROBOT_CAPTURE_SCALE")
            .ok()
            .as_deref(),
    )
}

#[cfg(any(test, feature = "robot"))]
fn parse_robot_capture_scale(value: Option<&str>) -> f32 {
    value
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|scale| scale.is_finite() && *scale >= 0.5 && *scale <= 4.0)
        .unwrap_or(1.0)
}

#[cfg(any(test, feature = "robot"))]
fn resolve_robot_screenshot_params_with_scale(
    buffer_size: (u32, u32),
    fallback_logical_size: Option<(f32, f32)>,
    capture_scale: f32,
) -> (u32, u32, f32) {
    if let Some((logical_width, logical_height)) = fallback_logical_size {
        let width = (logical_width * capture_scale).ceil().max(1.0) as u32;
        let height = (logical_height * capture_scale).ceil().max(1.0) as u32;
        return (width, height, capture_scale);
    }

    let (buffer_width, buffer_height) = buffer_size;
    (buffer_width.max(1), buffer_height.max(1), 1.0)
}

#[cfg(test)]
#[path = "tests/desktop_tests.rs"]
mod tests;
