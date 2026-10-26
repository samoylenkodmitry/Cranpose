use std::{
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use cranpose_app_shell::{AppShell, FrameUpdateResult, PointerSource};
use cranpose_render_common::Renderer;
use cranpose_render_wgpu::WgpuRenderer;
use winit::{
    event_loop::{ActiveEventLoop, EventLoopProxy},
    window::Window,
};

use super::{
    IdleWaitTimeout, ROBOT_IDLE_MIN_ITERATIONS, ROBOT_IDLE_PRESENT_TIMEOUT,
    ROBOT_IDLE_STARVATION_CEILING, ROBOT_IDLE_TIMEOUT, ROBOT_PRESENT_WAIT_TIMEOUT,
    ROBOT_PUMP_FRAME_INTERVAL, RobotController, RobotScrollSequence, capture_screenshot,
    capture_screenshot_with_scale, primary_surface_redraw_drives_app, pump_robot_frame,
    robot_app_response, robot_frame_diagnostics, robot_mouse_button, robot_query_visual_dirty,
    robot_scroll_present_target, robot_tree_response, robot_visible_present_target,
    robot_visible_pump_present_target, update_app_with_native_window_registry,
};
use crate::{
    app_launcher::LaunchError,
    desktop::event_loop::{App, device_loss, request_redraw_once},
    native_window,
    robot::{
        RobotCommand, RobotIdlePending, RobotResponse, RobotScreenshot, RobotTimelineAction,
        char_to_key_code, find_button_in_app, find_text_in_app,
        robot_wait_for_idle_animation_loop_only,
    },
};

// What ends the event loop's turn of robot commands.
enum RobotStop {
    Exit,
    DriverPanicked(String),
}

// The event loop state a turn of robot commands reads and writes.
pub(in crate::desktop::event_loop) struct RobotTurn<'a> {
    app: &'a mut AppShell<WgpuRenderer>,
    window: &'a Arc<dyn Window>,
    registry: &'a Rc<native_window::NativeWindowRegistry>,
    controller: &'a mut RobotController,
    recorder: &'a mut Option<crate::recorder::InputRecorder>,
    app_hook: &'a mut Option<Box<crate::RobotAppHook>>,
    device_loss: &'a mut device_loss::DeviceLoss,
    event_proxy: &'a EventLoopProxy,
    surface_config: &'a Option<wgpu::SurfaceConfiguration>,
    surface_caps: &'a Option<wgpu::SurfaceCapabilities>,
    last_frame_start_time: &'a mut Option<Instant>,
    primary_redraw_pending: &'a mut bool,
    visible_surface_dirty: &'a mut bool,
    presented_frame_generation: u64,
    unpresentable_frames: u32,
    vsync_interval: Duration,
    primary_visible: bool,
    headless: bool,
    visual_dirty: bool,
    now: Instant,
}

impl RobotTurn<'_> {
    pub(in crate::desktop::event_loop) fn serve(
        app: &mut App,
        event_loop: &dyn ActiveEventLoop,
        now: Instant,
    ) -> std::ops::ControlFlow<()> {
        let registry = Rc::clone(&app.native_window_registry);
        let primary_visible = app.primary_visible();
        let Some(window) = app.window.clone() else {
            return std::ops::ControlFlow::Continue(());
        };
        let (Some(shell), Some(controller)) = (&mut app.app, &mut app.robot.controller) else {
            return std::ops::ControlFlow::Continue(());
        };
        let mut turn = RobotTurn {
            app: shell,
            window: &window,
            registry: &registry,
            controller,
            recorder: &mut app.recorder,
            app_hook: &mut app.robot.app_hook,
            device_loss: &mut app.device_loss,
            event_proxy: &app.event_proxy,
            surface_config: &app.surface_config,
            surface_caps: &app.surface_caps,
            last_frame_start_time: &mut app.last_frame_start_time,
            primary_redraw_pending: &mut app.primary_redraw_pending,
            visible_surface_dirty: &mut app.robot.visible_surface_dirty,
            presented_frame_generation: app.robot.presented_frame_generation,
            unpresentable_frames: app.robot.unpresentable_frames,
            vsync_interval: app.vsync_interval,
            primary_visible,
            headless: app.settings.headless,
            visual_dirty: false,
            now,
        };
        while let Some(command) = turn.controller.next_command() {
            match turn.run(command) {
                std::ops::ControlFlow::Continue(()) => {}
                std::ops::ControlFlow::Break(RobotStop::Exit) => {
                    app.exiting = true;
                    event_loop.exit();
                    return std::ops::ControlFlow::Break(());
                }
                std::ops::ControlFlow::Break(RobotStop::DriverPanicked(message)) => {
                    app.abort_launch(event_loop, LaunchError::TestDriverPanic(message));
                    return std::ops::ControlFlow::Break(());
                }
            }
        }
        turn.after_commands();
        std::ops::ControlFlow::Continue(())
    }

    fn run(&mut self, command: RobotCommand) -> std::ops::ControlFlow<RobotStop> {
        match command {
            RobotCommand::DriverPanicked(message) => {
                return std::ops::ControlFlow::Break(RobotStop::DriverPanicked(message));
            }
            RobotCommand::Exit => {
                let _ = self.controller.tx.send(RobotResponse::Ok);
                return std::ops::ControlFlow::Break(RobotStop::Exit);
            }
            command @ (RobotCommand::Click { .. }
            | RobotCommand::MoveTo { .. }
            | RobotCommand::MouseDown(..)
            | RobotCommand::MouseUp(..)
            | RobotCommand::MouseScroll { .. }
            | RobotCommand::TouchDown { .. }
            | RobotCommand::TouchMove { .. }
            | RobotCommand::TouchUp { .. }) => self.pointer(&command),
            command @ (RobotCommand::GetSemantics
            | RobotCommand::GetSpokenTree
            | RobotCommand::GetInspectorState
            | RobotCommand::AuditAccessibility
            | RobotCommand::GetRuntimeLeakDebugStats
            | RobotCommand::GetLiveUiTaskLabels
            | RobotCommand::MeasureText { .. }
            | RobotCommand::HasFocusedTextField
            | RobotCommand::SetSemanticsEnabled(..)
            | RobotCommand::ChooseMenuItem { .. }
            | RobotCommand::OpenUrls(..)
            | RobotCommand::FindText { .. }
            | RobotCommand::FindButton { .. }
            | RobotCommand::GetRenderStats
            | RobotCommand::GetFpsStats
            | RobotCommand::GetPresentationInfo
            | RobotCommand::GetPacingControlCenter(..)
            | RobotCommand::ResetFpsStats
            | RobotCommand::GetLastFlingVelocity
            | RobotCommand::ResetLastFlingVelocity
            | RobotCommand::GetRenderCpuAllocationStats) => self.query(command),
            command @ (RobotCommand::GetScreenshot
            | RobotCommand::GetScreenshotWithScale(..)
            | RobotCommand::CaptureKeyframes { .. }) => self.capture(command),
            command @ (RobotCommand::LoseGpuDevice { .. } | RobotCommand::InvokeAppHook { .. }) => {
                self.session(command);
            }
            command @ (RobotCommand::TypeText(..) | RobotCommand::SendKey { .. }) => {
                self.keys(command);
            }
            command @ (RobotCommand::WaitForIdle
            | RobotCommand::PumpFrames { .. }
            | RobotCommand::WaitForPresentFrame) => self.frames(&command),
            command @ RobotCommand::MouseScrollAndWaitForFrame { .. } => {
                self.scroll_and_wait(&command);
            }
            command @ RobotCommand::MouseScrollSequenceAndWaitForFrames { .. } => {
                self.scroll_sequence_and_wait(&command);
            }
            command @ RobotCommand::TouchMoveAndWaitForFrame { .. } => {
                self.touch_move_and_wait(&command);
            }
            command @ RobotCommand::CaptureInteractionKeyframes { .. } => {
                self.capture_interaction(command);
            }
        }
        std::ops::ControlFlow::Continue(())
    }

    fn pointer(&mut self, command: &RobotCommand) {
        match *command {
            RobotCommand::Click { x, y } => {
                self.app.set_pointer_source(PointerSource::Mouse);
                let event_time = self.app.realtime_pointer_event_time(None);
                let cursor_dirty = self.app.set_cursor_at_event_time(x, y, event_time);
                self.controller.begin_synthetic_primary_gesture();
                let press_dirty = self.app.pointer_pressed_at_event_time(event_time);
                let release_dirty = self.app.pointer_released_at_event_time(event_time);
                self.controller.end_synthetic_primary_gesture();
                self.visual_dirty |= cursor_dirty || press_dirty || release_dirty;
                let _ = self.controller.tx.send(RobotResponse::Ok);
            }
            RobotCommand::MoveTo { x, y } => {
                let event_time = self.app.realtime_pointer_event_time(None);
                self.visual_dirty |= self.app.set_cursor_at_event_time(x, y, event_time);
                if let Some(recorder) = &mut *self.recorder {
                    recorder.record_mouse_move(x, y);
                }
                let _ = self.controller.tx.send(RobotResponse::Ok);
            }
            RobotCommand::MouseDown(button) => {
                self.visual_dirty |= robot_mouse_button(
                    self.app,
                    self.controller,
                    self.recorder.as_mut(),
                    (button, true),
                );
                let _ = self.controller.tx.send(RobotResponse::Ok);
            }
            RobotCommand::MouseUp(button) => {
                self.visual_dirty |= robot_mouse_button(
                    self.app,
                    self.controller,
                    self.recorder.as_mut(),
                    (button, false),
                );
                let _ = self.controller.tx.send(RobotResponse::Ok);
            }
            RobotCommand::MouseScroll { delta_x, delta_y } => {
                if self.app.pointer_scrolled(delta_x, delta_y) {
                    self.visual_dirty = true;
                }
                let _ = self.controller.tx.send(RobotResponse::Ok);
            }
            RobotCommand::TouchDown { x, y, source } => {
                self.app.set_pointer_source(source);
                let event_time = self.app.realtime_pointer_event_time(None);
                let cursor_dirty = self.app.set_cursor_at_event_time(x, y, event_time);
                self.controller.begin_synthetic_primary_gesture();
                let press_dirty = self.app.pointer_pressed_at_event_time(event_time);
                self.visual_dirty |= cursor_dirty || press_dirty;
                let _ = self.controller.tx.send(RobotResponse::Ok);
            }
            RobotCommand::TouchMove { x, y, source } => {
                self.app.set_pointer_source(source);
                let event_time = self.app.realtime_pointer_event_time(None);
                self.visual_dirty |= self.app.set_cursor_at_event_time(x, y, event_time);
                let _ = self.controller.tx.send(RobotResponse::Ok);
            }
            RobotCommand::TouchUp { x, y, source } => {
                self.app.set_pointer_source(source);
                let event_time = self.app.realtime_pointer_event_time(None);
                let cursor_dirty = self.app.set_cursor_at_event_time(x, y, event_time);
                let release_dirty = self.app.pointer_released_at_event_time(event_time);
                self.controller.end_synthetic_primary_gesture();
                self.visual_dirty |= cursor_dirty || release_dirty;
                let _ = self.controller.tx.send(RobotResponse::Ok);
            }
            _ => {}
        }
    }

    fn query(&mut self, command: RobotCommand) {
        match command {
            command @ (RobotCommand::GetSemantics
            | RobotCommand::GetSpokenTree
            | RobotCommand::GetInspectorState
            | RobotCommand::AuditAccessibility) => {
                let update_result = pump_robot_frame(self.app, self.registry);
                self.visual_dirty |=
                    robot_query_visual_dirty(update_result, self.app.needs_redraw());
                let _ = self
                    .controller
                    .tx
                    .send(robot_tree_response(self.app, &command));
            }
            command @ (RobotCommand::GetRuntimeLeakDebugStats
            | RobotCommand::GetLiveUiTaskLabels
            | RobotCommand::MeasureText { .. }
            | RobotCommand::HasFocusedTextField
            | RobotCommand::SetSemanticsEnabled(_)
            | RobotCommand::ChooseMenuItem { .. }
            | RobotCommand::OpenUrls(_)) => {
                let _ = self
                    .controller
                    .tx
                    .send(robot_app_response(self.app, command));
            }
            RobotCommand::FindText { text, match_kind } => {
                let update_result = pump_robot_frame(self.app, self.registry);
                self.visual_dirty |=
                    robot_query_visual_dirty(update_result, self.app.needs_redraw());
                let result = find_text_in_app(self.app, &text, match_kind);
                let _ = self
                    .controller
                    .tx
                    .send(RobotResponse::SemanticQuery(result));
            }
            RobotCommand::FindButton { text, match_kind } => {
                let update_result = pump_robot_frame(self.app, self.registry);
                self.visual_dirty |=
                    robot_query_visual_dirty(update_result, self.app.needs_redraw());
                let result = find_button_in_app(self.app, &text, match_kind);
                let _ = self
                    .controller
                    .tx
                    .send(RobotResponse::SemanticQuery(result));
            }
            RobotCommand::GetRenderStats
            | RobotCommand::GetFpsStats
            | RobotCommand::GetPresentationInfo => {
                let response = robot_frame_diagnostics(
                    &command,
                    self.app,
                    self.surface_config.as_ref(),
                    self.surface_caps.as_ref(),
                    self.vsync_interval,
                );
                let _ = self.controller.tx.send(response);
            }
            RobotCommand::GetPacingControlCenter(mode) => {
                let center = self.app.dev_overlay_control_center(mode);
                let _ = self
                    .controller
                    .tx
                    .send(RobotResponse::PacingControlCenter(center));
            }
            RobotCommand::ResetFpsStats => {
                self.app.reset_fps_stats();
                let _ = self.controller.tx.send(RobotResponse::Ok);
            }
            RobotCommand::GetLastFlingVelocity => {
                let velocity = self
                    .app
                    .debug_enter_app_context(cranpose_ui::debug_last_fling_velocity);
                let _ = self.controller.tx.send(RobotResponse::F32(velocity));
            }
            RobotCommand::ResetLastFlingVelocity => {
                self.app
                    .debug_enter_app_context(cranpose_ui::debug_reset_last_fling_velocity);
                let _ = self.controller.tx.send(RobotResponse::Ok);
            }
            RobotCommand::GetRenderCpuAllocationStats => {
                let _ = self
                    .controller
                    .tx
                    .send(RobotResponse::RenderCpuAllocationStats(Box::new(
                        self.app.renderer().debug_cpu_allocation_stats(),
                    )));
            }
            _ => {}
        }
    }

    fn capture(&mut self, command: RobotCommand) {
        match command {
            RobotCommand::GetScreenshot => self.screenshot(capture_screenshot),
            RobotCommand::GetScreenshotWithScale(scale) => {
                self.screenshot(|app| capture_screenshot_with_scale(app, scale));
            }
            RobotCommand::CaptureKeyframes { scale, steps } => {
                let mut shots = Vec::new();
                let mut capture_err = None;
                for (advance_ms, capture) in steps {
                    native_window::with_native_window_registry(self.registry, || {
                        self.app
                            .update_after_exact_interval(Duration::from_secs_f64(
                                f64::from(advance_ms.max(0.0)) / 1000.0,
                            ))
                    });
                    if capture {
                        match capture_screenshot_with_scale(self.app, scale) {
                            Ok(shot) => shots.push(shot),
                            Err(err) => {
                                capture_err = Some(err);
                                break;
                            }
                        }
                    }
                }
                let _ = self.controller.tx.send(match capture_err {
                    Some(err) => RobotResponse::Error(err),
                    None => RobotResponse::Screenshots(shots),
                });
            }
            _ => {}
        }
    }

    fn session(&mut self, command: RobotCommand) {
        match command {
            RobotCommand::LoseGpuDevice { times } => {
                // The pass before the next wait loses the devices.
                self.device_loss.lose_for_robot(times);
                self.event_proxy.wake_up();
            }
            RobotCommand::InvokeAppHook { name, argument } => {
                let response = match self.app_hook.as_mut() {
                    Some(hook) => self
                        .app
                        .debug_enter_app_context(|| hook(name, argument))
                        .map(RobotResponse::AppHookResult),
                    None => Err("robot app hook not configured".to_string()),
                };
                match response {
                    Ok(response) => {
                        let _ = self.controller.tx.send(response);
                    }
                    Err(err) => {
                        let _ = self.controller.tx.send(RobotResponse::Error(err));
                    }
                }
                self.visual_dirty = true;
            }
            _ => {}
        }
    }

    fn keys(&mut self, command: RobotCommand) {
        match command {
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
                    self.app.on_key_event(&key_event);
                }
                let update_result = update_app_with_native_window_registry(self.app, self.registry);
                self.visual_dirty |= update_result.visual_changed || self.app.needs_redraw();
                let _ = self.controller.tx.send(RobotResponse::Ok);
            }
            RobotCommand::SendKey {
                key,
                modifiers,
                event_type,
            } => {
                self.app
                    .on_key_event(&crate::robot::robot_key_event(&key, modifiers, event_type));
                let update_result = update_app_with_native_window_registry(self.app, self.registry);
                self.visual_dirty |= update_result.visual_changed || self.app.needs_redraw();
                let _ = self.controller.tx.send(RobotResponse::Ok);
            }
            _ => {}
        }
    }

    fn frames(&mut self, command: &RobotCommand) {
        match *command {
            RobotCommand::WaitForIdle => {
                self.controller.start_idle_wait();
                let visual_frame_pending = *self.visible_surface_dirty
                    || self.visual_dirty
                    || *self.primary_redraw_pending;
                self.controller.waiting_for_present_generation = robot_visible_present_target(
                    self.primary_visible,
                    self.headless,
                    visual_frame_pending,
                    self.presented_frame_generation,
                );
                if self.controller.waiting_for_present_generation.is_some() {
                    request_redraw_once(self.window, &mut *self.primary_redraw_pending);
                }
            }
            RobotCommand::PumpFrames { count } => {
                let update_result =
                    native_window::with_native_window_registry(self.registry, || {
                        let mut result = FrameUpdateResult::default();
                        for _ in 0..count {
                            let frame_result = self
                                .app
                                .update_after_frame_interval(ROBOT_PUMP_FRAME_INTERVAL);
                            result.visual_changed |= frame_result.visual_changed;
                            result.structure_changed |= frame_result.structure_changed;
                        }
                        result
                    });
                self.visual_dirty |= update_result.visual_changed || self.app.needs_redraw();
                let present_target = self
                    .visual_dirty
                    .then(|| {
                        robot_visible_pump_present_target(
                            self.primary_visible,
                            self.headless,
                            count,
                            self.presented_frame_generation,
                        )
                    })
                    .flatten();
                self.present_or_ack(present_target);
            }
            RobotCommand::WaitForPresentFrame => {
                let mut visual_frame_pending =
                    *self.visible_surface_dirty || self.app.needs_redraw();
                if !visual_frame_pending {
                    let update_result =
                        update_app_with_native_window_registry(self.app, self.registry);
                    visual_frame_pending = self.app.needs_redraw() || update_result.visual_changed;
                    *self.visible_surface_dirty = visual_frame_pending;
                }
                let present_target = visual_frame_pending
                    .then(|| {
                        robot_visible_pump_present_target(
                            self.primary_visible,
                            self.headless,
                            1,
                            self.presented_frame_generation,
                        )
                    })
                    .flatten();
                self.present_or_ack(present_target);
            }
            _ => {}
        }
    }

    fn scroll_and_wait(&mut self, command: &RobotCommand) {
        let RobotCommand::MouseScrollAndWaitForFrame { delta_x, delta_y } = *command else {
            return;
        };
        let Some(target) = self.scroll_target(delta_x, delta_y) else {
            return;
        };
        self.visual_dirty |= target.is_some();
        self.present_or_update(target);
    }

    fn scroll_sequence_and_wait(&mut self, command: &RobotCommand) {
        let RobotCommand::MouseScrollSequenceAndWaitForFrames {
            delta_x,
            delta_y,
            count,
        } = *command
        else {
            return;
        };
        if count == 0 {
            let _ = self.controller.tx.send(RobotResponse::Ok);
            return;
        }
        let Some(target) = self.scroll_target(delta_x, delta_y) else {
            return;
        };
        if target.is_some() {
            self.controller.scroll_sequence = Some(RobotScrollSequence {
                delta_x,
                delta_y,
                remaining: count.saturating_sub(1),
            });
            self.visual_dirty = true;
        }
        self.present_or_update(target);
    }

    fn touch_move_and_wait(&mut self, command: &RobotCommand) {
        let RobotCommand::TouchMoveAndWaitForFrame { x, y, source } = *command else {
            return;
        };
        self.app.set_pointer_source(source);
        let event_time = self.app.realtime_pointer_event_time(None);
        let visual_dirty = self.app.set_cursor_at_event_time(x, y, event_time);
        if visual_dirty {
            self.visual_dirty = true;
        }
        let present_target = visual_dirty
            .then(|| {
                robot_visible_pump_present_target(
                    self.primary_visible,
                    self.headless,
                    1,
                    self.presented_frame_generation,
                )
            })
            .flatten();
        self.present_or_update(present_target);
    }

    fn capture_interaction(&mut self, command: RobotCommand) {
        let RobotCommand::CaptureInteractionKeyframes { scale, steps } = command else {
            return;
        };
        let mut shots = Vec::new();
        let mut capture_err = None;
        for step in steps {
            native_window::with_native_window_registry(self.registry, || {
                self.app
                    .update_after_exact_interval(Duration::from_secs_f64(
                        f64::from(step.advance_ms.max(0.0)) / 1000.0,
                    ))
            });
            for action in step.actions {
                let action_result = match action {
                    RobotTimelineAction::MoveTo { x, y } => {
                        let event_time = self.app.exact_pointer_event_time(None);
                        self.app.set_cursor_at_event_time(x, y, event_time);
                        Ok(())
                    }
                    RobotTimelineAction::MouseDown => {
                        let event_time = self.app.exact_pointer_event_time(None);
                        self.controller.begin_synthetic_primary_gesture();
                        self.app.pointer_pressed_at_event_time(event_time);
                        Ok(())
                    }
                    RobotTimelineAction::MouseUp => {
                        let event_time = self.app.exact_pointer_event_time(None);
                        self.app.pointer_released_at_event_time(event_time);
                        self.controller.end_synthetic_primary_gesture();
                        Ok(())
                    }
                    RobotTimelineAction::InvokeAppHook { name, argument } => {
                        self.app_hook.as_mut().map_or_else(
                            || Err("robot app hook not configured".to_string()),
                            |hook| {
                                self.app
                                    .debug_enter_app_context(|| hook(name, argument))
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
            native_window::with_native_window_registry(self.registry, || {
                self.app.update_after_exact_interval(Duration::ZERO)
            });
            if step.capture {
                match capture_screenshot_with_scale(self.app, scale) {
                    Ok(shot) => shots.push(shot),
                    Err(err) => {
                        capture_err = Some(err);
                        break;
                    }
                }
            }
        }
        self.visual_dirty = true;
        let _ = self.controller.tx.send(match capture_err {
            Some(err) => RobotResponse::Error(err),
            None => RobotResponse::Screenshots(shots),
        });
    }

    fn begin_pump_present_wait(&mut self, target: u64) {
        self.controller.begin_pump_present_wait(target);
        *self.last_frame_start_time = None;
        request_redraw_once(self.window, &mut *self.primary_redraw_pending);
    }

    fn present_or_update(&mut self, target: Option<u64>) {
        match target {
            Some(target) => self.begin_pump_present_wait(target),
            None => {
                let update_result = update_app_with_native_window_registry(self.app, self.registry);
                *self.visible_surface_dirty =
                    self.app.needs_redraw() || update_result.visual_changed;
                let _ = self.controller.tx.send(RobotResponse::Ok);
            }
        }
    }

    fn present_or_ack(&mut self, target: Option<u64>) {
        match target {
            Some(target) => self.begin_pump_present_wait(target),
            None => {
                let _ = self.controller.tx.send(RobotResponse::Ok);
            }
        }
    }

    // The present a scroll waits for; None after it acknowledged a scroll
    // nothing took.
    fn scroll_target(&mut self, delta_x: f32, delta_y: f32) -> Option<Option<u64>> {
        let target = robot_scroll_present_target(
            self.app,
            delta_x,
            delta_y,
            self.primary_visible,
            self.headless,
            self.presented_frame_generation,
        );
        if target.is_none() {
            let _ = self.controller.tx.send(RobotResponse::Ok);
        }
        target
    }

    fn screenshot(
        &mut self,
        capture: impl FnOnce(&mut AppShell<WgpuRenderer>) -> Result<RobotScreenshot, String>,
    ) {
        let update_result = pump_robot_frame(self.app, self.registry);
        self.visual_dirty |= robot_query_visual_dirty(update_result, self.app.needs_redraw());
        let response = match capture(self.app) {
            Ok(screenshot) => RobotResponse::Screenshot(screenshot),
            Err(err) => RobotResponse::Error(err),
        };
        let _ = self.controller.tx.send(response);
    }

    fn after_commands(&mut self) {
        if self.visual_dirty {
            *self.visible_surface_dirty = true;
            if primary_surface_redraw_drives_app(self.primary_visible, self.headless) {
                request_redraw_once(self.window, &mut *self.primary_redraw_pending);
            }
        }
        if let Some(target_generation) = self.controller.waiting_for_pump_present_generation {
            self.serve_pump_present_wait(target_generation);
        }
        if self.controller.waiting_for_idle {
            self.serve_idle_wait();
        }
    }

    fn serve_pump_present_wait(&mut self, target_generation: u64) {
        if self.presented_frame_generation >= target_generation {
            self.end_or_step_scroll_sequence();
        } else if self.controller.pump_present_wait_timed_out(self.now) {
            self.controller.finish_pump_present_wait();
            self.controller.scroll_sequence = None;
            *self.visible_surface_dirty = self.app.needs_redraw();
            let _ = self.controller.tx.send(RobotResponse::Error(format!(
                "present wait: the window surface refused {} consecutive frames over {:?} \
                 and never reached generation {target_generation} (currently {}); the \
                 window is occluded, off-screen, or on a display that is not compositing",
                self.unpresentable_frames,
                ROBOT_PRESENT_WAIT_TIMEOUT,
                self.presented_frame_generation,
            )));
        } else {
            request_redraw_once(self.window, &mut *self.primary_redraw_pending);
        }
    }

    // The present the pump waited for came: a scroll sequence takes its next
    // step, or the wait ends.
    fn end_or_step_scroll_sequence(&mut self) {
        if let Some(mut sequence) = self.controller.scroll_sequence.take() {
            if sequence.remaining == 0 {
                self.controller.finish_pump_present_wait();
                *self.visible_surface_dirty = self.app.needs_redraw();
                let _ = self.controller.tx.send(RobotResponse::Ok);
            } else if self
                .app
                .pointer_scrolled(sequence.delta_x, sequence.delta_y)
            {
                sequence.remaining = sequence.remaining.saturating_sub(1);
                self.controller.scroll_sequence = Some(sequence);
                self.controller
                    .begin_pump_present_wait(self.presented_frame_generation.saturating_add(1));
                *self.visible_surface_dirty = true;
                *self.last_frame_start_time = None;
                request_redraw_once(self.window, &mut *self.primary_redraw_pending);
            } else {
                self.controller.finish_pump_present_wait();
                *self.visible_surface_dirty = self.app.needs_redraw();
                let _ = self.controller.tx.send(RobotResponse::Ok);
            }
        } else {
            self.controller.finish_pump_present_wait();
            *self.visible_surface_dirty = self.app.needs_redraw();
            let _ = self.controller.tx.send(RobotResponse::Ok);
        }
    }

    fn serve_idle_wait(&mut self) {
        let frame_schedule = self.app.frame_schedule();
        let needs_update = frame_schedule.needs_update;
        let needs_frame = frame_schedule.needs_frame;
        let has_active_animations = self.app.has_active_animations();
        let has_transient_frame_callbacks = self.app.has_transient_frame_callbacks();
        let visible_redraw_pending = *self.visible_surface_dirty || self.app.needs_redraw();
        if self.controller.waiting_for_present_generation.is_none()
            && *self.primary_redraw_pending
            && visible_redraw_pending
        {
            self.controller.waiting_for_present_generation = robot_visible_present_target(
                self.primary_visible,
                self.headless,
                true,
                self.presented_frame_generation,
            );
        }
        let present_still_required = visible_redraw_pending;
        let waiting_for_present = present_still_required
            && self
                .controller
                .waiting_for_present_generation
                .is_some_and(|target| self.presented_frame_generation < target);
        let pending = RobotIdlePending {
            needs_frame,
            needs_update,
            transient_frame_callbacks: has_transient_frame_callbacks,
            waiting_for_present,
            awaits_pipelines: self.app.renderer().awaits_pipelines(),
        };
        let animation_loop_only = robot_wait_for_idle_animation_loop_only(
            has_active_animations,
            has_transient_frame_callbacks,
            waiting_for_present,
            self.controller.idle_iterations,
            self.controller.idle_structure_clean_frames,
        );
        if pending.frame_only() || animation_loop_only {
            self.controller.finish_idle_wait();
            *self.visible_surface_dirty = false;
            let _ = self.controller.tx.send(RobotResponse::Ok);
        } else if pending.quiet() {
            self.idle_wait_quiet(needs_update);
        } else {
            self.idle_wait_busy(pending, has_active_animations);
        }
    }

    fn idle_wait_quiet(&mut self, needs_update: bool) {
        let mut finish_idle = true;
        if needs_update {
            let update_result = update_app_with_native_window_registry(self.app, self.registry);
            self.controller.record_idle_update_result(update_result);
            *self.visible_surface_dirty = self.app.needs_redraw() || update_result.visual_changed;
            let follow_up_schedule = self.app.frame_schedule();
            if follow_up_schedule.needs_frame || update_result.structure_changed {
                finish_idle = false;
                if primary_surface_redraw_drives_app(self.primary_visible, self.headless) {
                    request_redraw_once(self.window, &mut *self.primary_redraw_pending);
                }
                self.controller.idle_iterations += 1;
            }
        }
        if finish_idle {
            self.controller.finish_idle_wait();
            *self.visible_surface_dirty = false;
            let _ = self.controller.tx.send(RobotResponse::Ok);
        }
    }

    fn idle_wait_busy(&mut self, pending: RobotIdlePending, has_active_animations: bool) {
        let needs_frame = pending.needs_frame;
        let needs_update = pending.needs_update;
        let has_transient_frame_callbacks = pending.transient_frame_callbacks;
        let waiting_for_present = pending.waiting_for_present;
        if needs_frame && primary_surface_redraw_drives_app(self.primary_visible, self.headless) {
            request_redraw_once(self.window, &mut *self.primary_redraw_pending);
        } else {
            let update_result = update_app_with_native_window_registry(self.app, self.registry);
            self.controller.record_idle_update_result(update_result);
            *self.visible_surface_dirty = self.app.needs_redraw() || update_result.visual_changed;
        }
        self.controller.idle_iterations += 1;

        if self.controller.idle_iterations.is_multiple_of(50) {
            log::debug!(
                "wait_for_idle iteration {}: needs_update={}, needs_redraw={}, has_animations={}, waiting_for_present={}, presented_frame_generation={}, target_present_generation={:?}",
                self.controller.idle_iterations,
                needs_update,
                self.app.needs_redraw(),
                has_active_animations,
                waiting_for_present,
                self.presented_frame_generation,
                self.controller.waiting_for_present_generation
            );
        }

        let present_is_sole_blocker = waiting_for_present
            && !needs_update
            && !has_active_animations
            && !has_transient_frame_callbacks;
        let timeout_check_at = Instant::now();
        self.controller
            .observe_idle_present_block(present_is_sole_blocker, timeout_check_at);
        if let Some(timeout) = self.controller.idle_wait_timeout(timeout_check_at) {
            let iterations = self.controller.idle_iterations;
            let target_generation = self.controller.waiting_for_present_generation;
            self.controller.finish_idle_wait();
            let message = self.idle_timeout_message(
                timeout,
                iterations,
                target_generation,
                pending,
                has_active_animations,
            );
            let _ = self.controller.tx.send(RobotResponse::Error(message));
        }
    }

    fn idle_timeout_message(
        &self,
        timeout: IdleWaitTimeout,
        iterations: u32,
        target_generation: Option<u64>,
        pending: RobotIdlePending,
        has_active_animations: bool,
    ) -> String {
        let needs_update = pending.needs_update;
        let waiting_for_present = pending.waiting_for_present;
        match timeout {
            IdleWaitTimeout::AppNotConverging => format!(
                "wait_for_idle: timed out after {ROBOT_IDLE_TIMEOUT:?} ({iterations} iterations); needs_update={needs_update}, needs_redraw={}, has_animations={has_active_animations}, waiting_for_present={waiting_for_present}",
                self.app.needs_redraw(),
            ),
            IdleWaitTimeout::SurfaceNotPresenting => format!(
                "wait_for_idle: the window surface refused {} consecutive frames over {:?} and never reached generation {target_generation:?} (currently {}); composition had already settled, so this is the compositor and not the application -- the window is occluded, off-screen, or on a display that is not compositing; needs_update={needs_update}, needs_redraw={}, has_animations={has_active_animations}, waiting_for_present={waiting_for_present}",
                self.unpresentable_frames,
                ROBOT_IDLE_PRESENT_TIMEOUT,
                self.presented_frame_generation,
                self.app.needs_redraw(),
            ),
            IdleWaitTimeout::HostStarved => format!(
                "wait_for_idle: host starvation -- only {iterations} iterations ran in {:?} (need {ROBOT_IDLE_MIN_ITERATIONS} to trust the elapsed budget); the process was not scheduled enough to tell whether the application is converging, not an application defect; needs_update={needs_update}, needs_redraw={}, has_animations={has_active_animations}, waiting_for_present={waiting_for_present}",
                ROBOT_IDLE_STARVATION_CEILING,
                self.app.needs_redraw(),
            ),
        }
    }
}
