use std::{
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant},
};

use cranpose_app_shell::{AppShell, FrameUpdateResult};
use cranpose_render_common::Renderer;
use cranpose_render_wgpu::WgpuRenderer;
use winit::event_loop::ControlFlow;

use super::{
    choose_menu_item, open_urls, primary_surface_redraw_drives_app,
    update_app_with_native_window_registry,
};
use crate::{
    native_window,
    robot::{Robot, RobotChannel, RobotCommand, RobotResponse, RobotScreenshot, extract_semantics},
};

const ROBOT_IDLE_TIMEOUT: Duration = Duration::from_secs(10);

const ROBOT_IDLE_MIN_ITERATIONS: u32 = 100;

const ROBOT_IDLE_PRESENT_TIMEOUT: Duration = Duration::from_secs(30);

const ROBOT_IDLE_STARVATION_CEILING: Duration = Duration::from_secs(60);

const ROBOT_PUMP_FRAME_INTERVAL: Duration = Duration::from_nanos(16_666_667);

const ROBOT_PRESENT_WAIT_TIMEOUT: Duration = Duration::from_secs(5);

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

/// The answer to a robot query that the app answers in its own context.
fn robot_app_response(app: &mut AppShell<WgpuRenderer>, command: RobotCommand) -> RobotResponse {
    match command {
        RobotCommand::GetRuntimeLeakDebugStats => {
            RobotResponse::RuntimeLeakDebugStats(Box::new(app.debug_runtime_leak_stats()))
        }
        RobotCommand::GetLiveUiTaskLabels => {
            RobotResponse::LiveUiTaskLabels(app.runtime_handle().live_ui_task_labels())
        }
        RobotCommand::MeasureText { text, style } => {
            RobotResponse::TextMetrics(app.debug_enter_app_context(|| {
                let text = cranpose_ui::text::AnnotatedString::from(text.as_str());
                cranpose_ui::measure_text(&text, &style)
            }))
        }
        RobotCommand::HasFocusedTextField => RobotResponse::Bool(
            app.debug_enter_app_context(cranpose_ui::text_field_focus::has_focused_field),
        ),
        RobotCommand::SetSemanticsEnabled(enabled) => {
            app.set_semantics_enabled(enabled);
            RobotResponse::Ok
        }
        RobotCommand::ChooseMenuItem { tray, labels } => choose_menu_item(tray, &labels)
            .map_or_else(RobotResponse::Error, |()| RobotResponse::Ok),
        RobotCommand::OpenUrls(urls) => {
            open_urls(&urls).map_or_else(RobotResponse::Error, |()| RobotResponse::Ok)
        }
        _ => RobotResponse::Error("not an app query".into()),
    }
}

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

fn robot_query_should_drain_frame(app: &AppShell<WgpuRenderer>) -> bool {
    app.needs_redraw()
}

fn robot_query_visual_dirty(update_result: FrameUpdateResult, needs_redraw: bool) -> bool {
    update_result.visual_changed || needs_redraw
}

/// Presses (`true`) or releases a mouse button for the robot. The primary
/// button also opens or closes the robot's synthetic gesture, and the
/// recorder writes it down.
fn robot_mouse_button(
    app: &mut AppShell<WgpuRenderer>,
    controller: &mut RobotController,
    recorder: Option<&mut crate::recorder::InputRecorder>,
    (button, pressed): (cranpose_ui::PointerButton, bool),
) -> bool {
    if button != cranpose_ui::PointerButton::Primary {
        let mut surface = app.primary();
        return if pressed {
            surface.mouse_button_pressed(button)
        } else {
            surface.mouse_button_released(button)
        };
    }
    let event_time = app.realtime_pointer_event_time(None);
    if pressed {
        controller.begin_synthetic_primary_gesture();
        let handled = app.pointer_pressed_at_event_time(event_time);
        if let Some(recorder) = recorder {
            recorder.record_mouse_down();
        }
        handled
    } else {
        let handled = app.pointer_released_at_event_time(event_time);
        controller.end_synthetic_primary_gesture();
        if let Some(recorder) = recorder {
            recorder.record_mouse_up();
        }
        handled
    }
}

pub(super) struct RobotController {
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

#[derive(Clone, Copy, Debug)]
struct RobotScrollSequence {
    delta_x: f32,
    delta_y: f32,
    remaining: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IdleWaitTimeout {
    AppNotConverging,
    SurfaceNotPresenting,
    HostStarved,
}

impl RobotController {
    // A driver that hung up has nothing left to hear.
    pub(super) fn respond(&self, response: RobotResponse) {
        let _ = self.tx.send(response);
    }

    pub(super) fn new(wake_event_loop: impl Fn() + Send + Sync + 'static) -> (Self, Robot) {
        let (channel, robot) = RobotChannel::new(wake_event_loop);
        let RobotChannel { rx, tx } = channel;

        let controller = Self {
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

    pub(super) fn synthetic_primary_down(&self) -> bool {
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

    pub(super) fn stage_pending_command(&mut self) -> bool {
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

    pub(super) fn awaiting_progress(&mut self) -> bool {
        self.waiting_for_idle
            || self.waiting_for_present_generation.is_some()
            || self.waiting_for_pump_present_generation.is_some()
            || self.scroll_sequence.is_some()
            || self.synthetic_primary_down
            || self.stage_pending_command()
    }

    pub(super) fn record_idle_update_result(&mut self, result: FrameUpdateResult) {
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

fn robot_visible_present_target(
    primary_window_visible: bool,
    headless: bool,
    surface_dirty: bool,
    presented_frame_generation: u64,
) -> Option<u64> {
    (surface_dirty && primary_surface_redraw_drives_app(primary_window_visible, headless))
        .then(|| presented_frame_generation.saturating_add(1))
}

fn robot_visible_pump_present_target(
    primary_window_visible: bool,
    headless: bool,
    frame_count: u32,
    presented_frame_generation: u64,
) -> Option<u64> {
    (frame_count > 0 && primary_surface_redraw_drives_app(primary_window_visible, headless))
        .then(|| presented_frame_generation.saturating_add(frame_count as u64))
}

const ROBOT_PARKED_COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(8);

pub(super) fn bound_robot_park(control_flow: ControlFlow, now: Instant) -> ControlFlow {
    let bound = now + ROBOT_PARKED_COMMAND_POLL_INTERVAL;
    match control_flow {
        ControlFlow::Poll => ControlFlow::Poll,
        ControlFlow::Wait => ControlFlow::WaitUntil(bound),
        ControlFlow::WaitUntil(deadline) => ControlFlow::WaitUntil(deadline.min(bound)),
    }
}

/// How long a robot screenshot waits for the pipelines its effects need.
const ROBOT_PIPELINE_WAIT: Duration = Duration::from_secs(30);

/// Captures the scene at `scale`, again until the capture draws no
/// placeholder and no shape through a stand-in: a robot screenshot is the
/// final picture, drawn once the pipelines its effects and shapes wait for
/// have landed.
fn capture_final_frame(
    app: &mut AppShell<WgpuRenderer>,
    width: u32,
    height: u32,
    scale: f32,
) -> Result<cranpose_render_wgpu::CapturedFrame, String> {
    let deadline = Instant::now() + ROBOT_PIPELINE_WAIT;
    loop {
        let captured = app
            .renderer()
            .capture_frame_with_scale(width, height, scale)
            .map_err(|err| format!("Failed to capture GPU screenshot: {err:?}"))?;
        let drew_stand_ins = app.renderer().last_frame_stats().is_some_and(|stats| {
            stats.placeholder_draws > 0
                || stats.shape_pipeline_fallback_draws > 0
                || stats.instant_draws > 0
        });
        if !drew_stand_ins || Instant::now() >= deadline {
            return Ok(captured);
        }
        while app.renderer().awaits_pipelines() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

fn capture_screenshot(app: &mut AppShell<WgpuRenderer>) -> Result<RobotScreenshot, String> {
    let logical_size = app.viewport_size();
    let (width, height, capture_scale) =
        resolve_robot_screenshot_params(app.buffer_size(), Some(logical_size));

    let captured = capture_final_frame(app, width, height, capture_scale)?;

    let (logical_width, logical_height) = logical_size;

    Ok(RobotScreenshot {
        width: captured.width,
        height: captured.height,
        logical_width,
        logical_height,
        pixels: captured.pixels,
    })
}

fn capture_screenshot_with_scale(
    app: &mut AppShell<WgpuRenderer>,
    scale: f32,
) -> Result<RobotScreenshot, String> {
    let (logical_width, logical_height) = app.viewport_size();
    let width = (logical_width * scale).ceil().max(1.0) as u32;
    let height = (logical_height * scale).ceil().max(1.0) as u32;

    let captured = capture_final_frame(app, width, height, scale)?;

    Ok(RobotScreenshot {
        width: captured.width,
        height: captured.height,
        logical_width,
        logical_height,
        pixels: captured.pixels,
    })
}

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

fn robot_capture_scale_from_env() -> f32 {
    parse_robot_capture_scale(
        std::env::var("CRANPOSE_ROBOT_CAPTURE_SCALE")
            .ok()
            .as_deref(),
    )
}

fn parse_robot_capture_scale(value: Option<&str>) -> f32 {
    value
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|scale| scale.is_finite() && *scale >= 0.5 && *scale <= 4.0)
        .unwrap_or(1.0)
}

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

#[path = "robot_turn.rs"]
mod robot_turn;

pub(super) use robot_turn::RobotTurn;

#[cfg(test)]
#[path = "tests/robot_driver_tests.rs"]
mod tests;
