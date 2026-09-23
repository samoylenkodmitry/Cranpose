use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use cranpose_core::Runtime;
use cranpose_runtime_std::StdScheduler;

#[test]
fn a_window_coming_up_alone_takes_focus_so_its_cursor_is_drawn() {
    assert!(
        super::a_new_window_comes_up_key(false, true, false, WindowFocus::WhenNoneFocused),
        "macOS reads cursor rectangles only for the key window, so the \
         first window up has to take focus or its cursor is never drawn"
    );
}

#[test]
fn a_window_coming_up_beside_a_focused_one_leaves_the_focus_alone() {
    assert!(
        !super::a_new_window_comes_up_key(false, true, true, WindowFocus::WhenNoneFocused),
        "a window the user is already working in keeps focus"
    );
}

#[test]
fn a_window_with_nothing_on_screen_is_left_alone() {
    assert!(!super::a_new_window_comes_up_key(
        false,
        false,
        false,
        WindowFocus::WhenNoneFocused
    ));
    assert!(!super::a_new_window_comes_up_key(
        true,
        true,
        false,
        WindowFocus::WhenNoneFocused
    ));
}

#[test]
fn a_window_told_to_always_take_focus_takes_it_beside_a_focused_one() {
    assert!(super::a_new_window_comes_up_key(
        false,
        true,
        true,
        WindowFocus::Always
    ));
}

#[test]
fn a_window_told_never_to_take_focus_leaves_it_even_alone() {
    assert!(!super::a_new_window_comes_up_key(
        false,
        true,
        false,
        WindowFocus::Never
    ));
}

fn quiet_loop() -> LoopControlInputs {
    LoopControlInputs {
        robot_needs_poll: false,
        free_running: false,
        primary_pointer_polled: false,
        drag_poll_deadline: None,
        position_poll_deadline: None,
        frame_cap_deadline: None,
        has_active_animations: false,
        next_event_time: None,
    }
}

#[test]
fn a_drag_poll_ahead_is_waited_for_and_one_that_is_due_is_run() {
    let now = Instant::now();
    let ahead = now + std::time::Duration::from_millis(16);
    assert_eq!(
        event_loop_control_flow(
            now,
            LoopControlInputs {
                drag_poll_deadline: Some(ahead),
                ..quiet_loop()
            }
        ),
        ControlFlow::WaitUntil(ahead),
        "a poll ahead is a deadline, not a reason to spin"
    );
    assert_eq!(
        event_loop_control_flow(
            now,
            LoopControlInputs {
                drag_poll_deadline: Some(now),
                ..quiet_loop()
            }
        ),
        ControlFlow::Poll
    );
    assert_eq!(
        event_loop_control_flow(now, quiet_loop()),
        ControlFlow::Wait
    );
}

#[test]
fn a_build_that_cannot_read_the_pointer_never_polls_a_drag() {
    let now = Instant::now();
    assert_eq!(native_window_drag_poll_deadline(now, true), Some(now));
    assert_eq!(native_window_drag_poll_deadline(now, false), None);
}

#[test]
fn a_redraw_that_presents_nothing_still_paces_the_next_one() {
    let attempt = Instant::now();
    let mut last_frame_start_time = None;
    pace_after_empty_redraw(&mut last_frame_start_time, attempt, false);
    assert_eq!(last_frame_start_time, Some(attempt));
    let mut presented = Some(attempt);
    pace_after_empty_redraw(
        &mut presented,
        attempt + std::time::Duration::from_millis(5),
        true,
    );
    assert_eq!(presented, Some(attempt), "a present keeps its own anchor");
}

#[test]
fn a_trace_that_is_off_evaluates_none_of_its_arguments() {
    let evaluated = std::cell::Cell::new(false);
    let sink = |_: std::fmt::Arguments<'_>| {};
    trace_when!(false, sink, "{}", {
        evaluated.set(true);
        1
    });
    assert!(!evaluated.get(), "a trace that is off must cost nothing");
    trace_when!(true, sink, "{}", {
        evaluated.set(true);
        1
    });
    assert!(evaluated.get());
}

fn window_options() -> crate::native_window::NativeWindowOptions {
    crate::native_window::NativeWindowOptions {
        title: "w".to_string(),
        width: 320.0,
        height: 240.0,
        x: None,
        y: None,
        position_origin: crate::native_window::NativeWindowPositionOrigin::Screen,
        decorations: false,
        transparent: false,
        shadow: true,
        resizable: true,
        visible: true,
        always_on_top: false,
        min_width: None,
        min_height: None,
        max_width: None,
        max_height: None,
        focus: WindowFocus::WhenNoneFocused,
    }
}

#[test]
fn a_window_that_must_not_take_focus_comes_up_inactive() {
    assert!(
        !super::native_window_attributes(&window_options(), false, false, None).active,
        "a window that comes up key steals the press the focused window is \
         holding, and the gesture it was in the middle of is cancelled"
    );
}

#[test]
fn a_window_that_may_take_focus_comes_up_active() {
    assert!(super::native_window_attributes(&window_options(), false, true, None).active);
}

fn opaque_icon() -> cranpose_ui::ImageBitmap {
    cranpose_ui::ImageBitmap::from_rgba8(2, 2, vec![200; 16]).expect("a 2x2 bitmap")
}

#[test]
fn a_window_icon_becomes_a_winit_icon() {
    assert!(super::winit_window_icon(&opaque_icon()).is_some());
}

#[test]
fn every_native_window_carries_the_application_window_icon() {
    let attributes =
        super::native_window_attributes(&window_options(), false, true, Some(&opaque_icon()));

    assert!(
        attributes.window_icon.is_some(),
        "a torn-out window with no icon shows the platform's blank one in the taskbar"
    );
}

fn icon_pixels(attributes: &winit::window::WindowAttributes) -> *const u8 {
    attributes
        .window_icon
        .as_ref()
        .and_then(|icon| icon.cast_ref::<winit::icon::RgbaIcon>())
        .expect("the window carries an RGBA icon")
        .buffer()
        .as_ptr()
}

#[test]
fn no_two_windows_share_the_pixels_of_one_icon() {
    let icon = opaque_icon();
    let first = super::native_window_attributes(&window_options(), false, true, Some(&icon));
    let second = super::native_window_attributes(&window_options(), false, true, Some(&icon));

    assert_ne!(
        icon_pixels(&first),
        icon_pixels(&second),
        "winit's Windows backend swaps an icon's red and blue in place when it makes the \
         HICON, so a buffer used twice comes out with its colours reversed the second time"
    );
}

#[test]
fn the_primary_window_carries_the_application_icon_in_every_place_the_platform_draws_one() {
    let attributes = super::with_application_icon(
        winit::window::WindowAttributes::default(),
        Some(&opaque_icon()),
    );

    assert!(attributes.window_icon.is_some());
    #[cfg(target_os = "windows")]
    assert!(
        attributes.platform.is_some(),
        "Windows draws the taskbar and Alt-Tab from ICON_BIG, which only the platform \
         attributes set"
    );
}

#[test]
fn a_native_window_has_no_icon_when_the_application_names_none() {
    assert!(
        super::native_window_attributes(&window_options(), false, true, None)
            .window_icon
            .is_none()
    );
}

#[test]
fn a_drag_with_no_pointer_to_poll_and_no_anchor_leaves_the_move_to_the_platform() {
    assert!(
        super::native_window_polling_drag_pointer(None, None).is_none(),
        "polling reads the pointer every frame, so without one the window \
         never moves and the platform drag has to take the gesture"
    );
}

#[test]
fn a_drag_anchored_at_a_handed_over_press_polls_without_a_platform_pointer() {
    assert_eq!(
        super::native_window_polling_drag_pointer(
            None,
            Some(winit::dpi::PhysicalPosition::new(7.0, 9.0)),
        ),
        Some(winit::dpi::PhysicalPosition::new(7.0, 9.0)),
        "the window holding the button relays its moves, so the session has \
         a pointer to follow from the anchor on"
    );
}

#[test]
fn a_drag_polls_from_the_anchor_the_press_recorded() {
    let global = super::NativeWindowPointerState {
        position: winit::dpi::PhysicalPosition::new(1.0, 2.0),
        primary_down: true,
    };
    assert_eq!(
        super::native_window_polling_drag_pointer(
            Some(global),
            Some(winit::dpi::PhysicalPosition::new(7.0, 9.0)),
        ),
        Some(winit::dpi::PhysicalPosition::new(7.0, 9.0))
    );
    assert_eq!(
        super::native_window_polling_drag_pointer(Some(global), None),
        Some(winit::dpi::PhysicalPosition::new(1.0, 2.0))
    );
}

#[cfg(feature = "robot")]
use cranpose_app_shell::FrameUpdateResult;
use winit::dpi::{PhysicalPosition, PhysicalSize};

use super::{
    App, ControlFlow, DesktopRect, FramePacingMode, HandedPress, HeldPressStep, LoopControlInputs,
    NativeWindowOptions, NativeWindowPointerState, NativeWindowPollingDragSession,
    NativeWindowPositionObservation, NativeWindowPositionOrigin, PendingNativeWindowPositions,
    PressBelongsHere, PressToHandOver, PrimaryPointerGesturePollAction, RootId, WindowFocus,
    WinitWindowId, clamp_rect_to_monitor_delta, desired_frame_latency, event_loop_control_flow,
    frame_interval_for_mode, free_running_frame, held_press_after_step, held_press_step,
    held_press_to_hand_over, initial_present_redraw_needed, native_window_drag_poll_deadline,
    native_window_options_change_is_position_only, native_window_position_poll_needed,
    native_window_redraw_held_while_hidden, nearest_monitor_to_rect, next_frame_anchor,
    occlusion_leaves_a_frame_owed, pace_after_empty_redraw, physical_outer_origin_from_surface,
    physical_surface_local_pointer, physical_surface_origin_from_outer,
    physical_surface_rect_contains_pointer, pointer_button_frame_request, press_belongs_here,
    press_to_hand_over, primary_declaration_host_needs_direct_update, primary_frame_waker,
    primary_frame_waker_uses_event_proxy, primary_launch_requires_initial_redraw,
    primary_pointer_gesture_poll_action, primary_pointer_move_should_recover_press,
    primary_surface_redraw_drives_app, primary_viewport_for_surface_size,
    primary_window_should_show, recovered_native_window_drag_start_pointer, scroll_frame_request,
    should_chain_no_vsync_redraw, surface_reconfigure_requires_redraw,
};
#[cfg(feature = "robot")]
use super::{
    IdleWaitTimeout, ROBOT_IDLE_MIN_ITERATIONS, ROBOT_IDLE_PRESENT_TIMEOUT,
    ROBOT_IDLE_STARVATION_CEILING, ROBOT_IDLE_TIMEOUT, ROBOT_PARKED_COMMAND_POLL_INTERVAL,
    ROBOT_PRESENT_WAIT_TIMEOUT, bound_park_for_robot,
};
#[cfg(feature = "robot")]
use super::{
    RobotController, parse_robot_capture_scale, resolve_robot_screenshot_params,
    resolve_robot_screenshot_params_with_scale, robot_query_visual_dirty,
    robot_visible_present_target, robot_visible_pump_present_target,
};
use crate::{app_launcher::AppSettings, wgpu_surface::surface_present_required};

#[test]
fn native_window_screen_position_is_declarative() {
    let options = NativeWindowOptions::new("child", 100.0, 50.0).with_position(10.0, 20.0);
    assert!(App::native_window_options_have_screen_position(&options));
}

#[test]
fn decorated_window_surface_coordinates_apply_frame_offset_once() {
    let outer = PhysicalPosition::new(100, 200);
    let surface_offset = PhysicalPosition::new(2, 32);
    let surface = physical_surface_origin_from_outer(outer, surface_offset);

    assert_eq!(surface, PhysicalPosition::new(102, 232));
    assert_eq!(
        physical_outer_origin_from_surface(surface, surface_offset),
        outer
    );
    assert_eq!(
        physical_surface_local_pointer(surface, PhysicalPosition::new(250.0, 808.0)),
        PhysicalPosition::new(148.0, 576.0)
    );
}

#[test]
fn desktop_wheel_dispatch_does_not_short_circuit_after_cursor_update() {
    let source = include_str!("../desktop.rs");
    let short_circuit = ["cursor_dirty ", "|| app.pointer_scrolled"].concat();

    assert!(
        !source.contains(&short_circuit),
        "desktop wheel dispatch must always call pointer_scrolled after updating hover cursor"
    );
}

#[test]
fn desktop_input_prefers_winit_cursor_before_x11_global_probe() {
    let source = include_str!("../desktop.rs");

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
    let source = include_str!("../desktop.rs");

    assert!(
        source.contains("let needs_update = frame_schedule.needs_update;"),
        "robot wait_for_idle must read update-only scheduler state"
    );
    assert!(
        source
            .contains("if !needs_frame && !has_transient_frame_callbacks && !waiting_for_present"),
        "robot wait_for_idle must not finish while update-only work is pending"
    );
}

#[test]
fn desktop_robot_idle_wait_allows_frame_only_loops() {
    let source = include_str!("../desktop.rs");

    assert!(
        source.contains(
            "let frame_only = needs_frame\n                    && !needs_update\n                    && !has_transient_frame_callbacks\n                    && !waiting_for_present;",
        ),
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
    let moved = NativeWindowOptions::borderless("Winamp", 275.0, 116.0).with_position(42.0, 48.0);
    let resized = NativeWindowOptions::borderless("Winamp", 280.0, 116.0).with_position(42.0, 48.0);
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
    let source = include_str!("../desktop.rs");

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

#[cfg(feature = "robot")]
#[test]
fn desktop_robot_queries_schedule_presentation_for_drained_visual_updates() {
    assert!(robot_query_visual_dirty(
        FrameUpdateResult {
            visual_changed: true,
            structure_changed: false,
        },
        false,
    ));
    assert!(robot_query_visual_dirty(FrameUpdateResult::default(), true,));
    assert!(!robot_query_visual_dirty(
        FrameUpdateResult {
            visual_changed: false,
            structure_changed: true,
        },
        false,
    ));
}

#[test]
fn a_mode_that_outruns_the_display_gets_a_deeper_drawable_queue() {
    let sixty = std::time::Duration::from_nanos(16_666_667);
    assert_eq!(desired_frame_latency(FramePacingMode::Hard120, sixty), 2);
    assert_eq!(desired_frame_latency(FramePacingMode::NoVsync, sixty), 2);
}

#[test]
fn a_mode_the_display_can_keep_up_with_keeps_the_shallow_queue() {
    let sixty = std::time::Duration::from_nanos(16_666_667);
    let one_twenty = std::time::Duration::from_nanos(8_333_333);
    assert_eq!(desired_frame_latency(FramePacingMode::Hard60, sixty), 1);
    assert_eq!(
        desired_frame_latency(FramePacingMode::Hard60, one_twenty),
        1
    );
    assert_eq!(
        desired_frame_latency(FramePacingMode::Hard120, one_twenty),
        1,
        "120fps on a 120Hz display never outruns the display"
    );
}

#[test]
fn vsync_keeps_its_queue_depth_on_every_display() {
    for interval in [
        std::time::Duration::from_nanos(16_666_667),
        std::time::Duration::from_nanos(8_333_333),
    ] {
        assert_eq!(desired_frame_latency(FramePacingMode::Vsync, interval), 2);
    }
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
fn free_running_loop_never_parks_on_a_frame_it_already_asked_for() {
    let vsync_interval = std::time::Duration::from_nanos(16_666_667);
    let no_vsync = frame_interval_for_mode(FramePacingMode::NoVsync, vsync_interval);

    assert!(free_running_frame(no_vsync, false, true));
    assert!(free_running_frame(no_vsync, true, false));
    assert!(!free_running_frame(no_vsync, false, false));

    assert!(!free_running_frame(
        frame_interval_for_mode(FramePacingMode::Vsync, vsync_interval),
        true,
        true
    ));
    assert!(!free_running_frame(
        frame_interval_for_mode(FramePacingMode::Hard120, vsync_interval),
        true,
        true
    ));
}

#[cfg(feature = "robot")]
#[test]
fn a_driven_run_never_parks_longer_than_its_command_poll() {
    let now = Instant::now();
    let far = now + std::time::Duration::from_secs(5);
    let soon = now + std::time::Duration::from_millis(1);
    let bound = now + ROBOT_PARKED_COMMAND_POLL_INTERVAL;

    assert_eq!(
        bound_park_for_robot(ControlFlow::Wait, true, now),
        ControlFlow::WaitUntil(bound)
    );
    assert_eq!(
        bound_park_for_robot(ControlFlow::WaitUntil(far), true, now),
        ControlFlow::WaitUntil(bound)
    );
    assert_eq!(
        bound_park_for_robot(ControlFlow::WaitUntil(soon), true, now),
        ControlFlow::WaitUntil(soon)
    );
    assert_eq!(
        bound_park_for_robot(ControlFlow::Poll, true, now),
        ControlFlow::Poll
    );
    assert_eq!(
        bound_park_for_robot(ControlFlow::Wait, false, now),
        ControlFlow::Wait
    );
    assert_eq!(
        bound_park_for_robot(ControlFlow::WaitUntil(far), false, now),
        ControlFlow::WaitUntil(far)
    );
}

#[cfg(feature = "robot")]
#[test]
fn an_idle_robot_lets_the_loop_park() {
    let (mut controller, robot) = RobotController::new(|| {});

    assert!(!controller.awaiting_progress());

    robot
        .command_sender()
        .send(crate::robot::RobotCommand::PumpFrames { count: 1 })
        .expect("robot command channel should remain open");
    assert!(controller.awaiting_progress());
    assert!(matches!(
        controller.next_command(),
        Some(crate::robot::RobotCommand::PumpFrames { count: 1 })
    ));
    assert!(!controller.awaiting_progress());

    controller.start_idle_wait();
    assert!(controller.awaiting_progress());
    controller.finish_idle_wait();
    assert!(!controller.awaiting_progress());

    controller.begin_pump_present_wait(7);
    assert!(controller.awaiting_progress());
    controller.finish_pump_present_wait();
    assert!(!controller.awaiting_progress());

    controller.begin_synthetic_primary_gesture();
    assert!(controller.awaiting_progress());
    controller.end_synthetic_primary_gesture();
    assert!(!controller.awaiting_progress());
}

#[cfg(feature = "robot")]
#[test]
fn robot_idle_wait_times_out_once_the_app_keeps_the_loop_busy() {
    let (mut controller, _robot) = RobotController::new(|| {});
    let started_at = Instant::now();

    controller.start_idle_wait_at(started_at);
    controller.idle_iterations = ROBOT_IDLE_MIN_ITERATIONS;

    assert_eq!(
        controller.idle_wait_timeout(started_at + ROBOT_IDLE_TIMEOUT / 2),
        None
    );
    assert_eq!(
        controller.idle_wait_timeout(started_at + ROBOT_IDLE_TIMEOUT),
        Some(IdleWaitTimeout::AppNotConverging)
    );
    controller.finish_idle_wait();
    assert_eq!(
        controller.idle_wait_timeout(started_at + ROBOT_IDLE_TIMEOUT * 2),
        None
    );
}

#[cfg(feature = "robot")]
#[test]
fn robot_idle_wait_does_not_blame_the_app_for_a_starved_host() {
    let (mut controller, _robot) = RobotController::new(|| {});
    let started_at = Instant::now();

    controller.start_idle_wait_at(started_at);
    controller.idle_iterations = 1;

    assert_eq!(
        controller.idle_wait_timeout(started_at + ROBOT_IDLE_TIMEOUT),
        None
    );
    assert_eq!(
        controller.idle_wait_timeout(
            started_at + ROBOT_IDLE_STARVATION_CEILING - std::time::Duration::from_millis(1)
        ),
        None
    );
}

#[cfg(feature = "robot")]
#[test]
fn robot_idle_wait_still_gives_up_on_a_permanently_wedged_host() {
    let (mut controller, _robot) = RobotController::new(|| {});
    let started_at = Instant::now();

    controller.start_idle_wait_at(started_at);
    controller.idle_iterations = 1;

    assert_eq!(
        controller.idle_wait_timeout(started_at + ROBOT_IDLE_STARVATION_CEILING),
        Some(IdleWaitTimeout::HostStarved)
    );
}

#[cfg(feature = "robot")]
#[test]
fn robot_idle_wait_does_not_blame_the_app_for_a_slow_present() {
    let (mut controller, _robot) = RobotController::new(|| {});
    let started_at = Instant::now();

    controller.start_idle_wait_at(started_at);
    controller.idle_iterations = 1_280_577;
    controller.observe_idle_present_block(true, started_at);

    assert_eq!(
        controller.idle_wait_timeout(started_at + ROBOT_IDLE_TIMEOUT),
        None
    );
    assert_eq!(
        controller.idle_wait_timeout(
            started_at + ROBOT_IDLE_PRESENT_TIMEOUT - std::time::Duration::from_millis(1)
        ),
        None
    );
}

#[cfg(feature = "robot")]
#[test]
fn robot_idle_wait_blames_the_surface_when_a_present_never_lands() {
    let (mut controller, _robot) = RobotController::new(|| {});
    let started_at = Instant::now();

    controller.start_idle_wait_at(started_at);
    controller.idle_iterations = 1_280_577;
    controller.observe_idle_present_block(true, started_at);

    assert_eq!(
        controller.idle_wait_timeout(started_at + ROBOT_IDLE_PRESENT_TIMEOUT),
        Some(IdleWaitTimeout::SurfaceNotPresenting)
    );
}

#[cfg(feature = "robot")]
#[test]
fn robot_idle_wait_still_blames_an_app_that_churns_while_a_present_is_outstanding() {
    let (mut controller, _robot) = RobotController::new(|| {});
    let started_at = Instant::now();

    controller.start_idle_wait_at(started_at);
    controller.idle_iterations = ROBOT_IDLE_MIN_ITERATIONS;
    controller.observe_idle_present_block(true, started_at);
    controller.observe_idle_present_block(false, started_at);

    assert_eq!(
        controller.idle_wait_timeout(started_at + ROBOT_IDLE_TIMEOUT),
        Some(IdleWaitTimeout::AppNotConverging)
    );
}

#[cfg(feature = "robot")]
#[test]
fn a_present_wait_gives_up_when_the_surface_never_presents() {
    let (mut controller, _robot) = RobotController::new(|| {});
    let started_at = Instant::now();

    controller.begin_pump_present_wait_at(11, started_at);
    assert!(!controller.pump_present_wait_timed_out(started_at));
    assert!(!controller.pump_present_wait_timed_out(started_at + ROBOT_PRESENT_WAIT_TIMEOUT / 2));
    assert!(controller.pump_present_wait_timed_out(started_at + ROBOT_PRESENT_WAIT_TIMEOUT));
}

#[cfg(feature = "robot")]
#[test]
fn a_present_that_arrives_clears_the_wait_deadline() {
    let (mut controller, _robot) = RobotController::new(|| {});
    let started_at = Instant::now();

    controller.begin_pump_present_wait_at(11, started_at);
    controller.finish_pump_present_wait();

    assert!(controller.waiting_for_pump_present_generation.is_none());
    assert!(
        !controller.pump_present_wait_timed_out(started_at + ROBOT_PRESENT_WAIT_TIMEOUT * 4),
        "a finished wait must not report a timeout on the next loop turn"
    );
}

#[cfg(feature = "robot")]
#[test]
fn each_presented_scroll_step_restarts_the_wait_deadline() {
    let (mut controller, _robot) = RobotController::new(|| {});
    let started_at = Instant::now();

    controller.begin_pump_present_wait_at(11, started_at);
    let next_step = started_at + ROBOT_PRESENT_WAIT_TIMEOUT - std::time::Duration::from_millis(1);
    controller.begin_pump_present_wait_at(12, next_step);

    assert!(
        !controller.pump_present_wait_timed_out(next_step + std::time::Duration::from_millis(2))
    );
    assert!(controller.pump_present_wait_timed_out(next_step + ROBOT_PRESENT_WAIT_TIMEOUT));
}

#[test]
fn frame_anchor_holds_cadence_despite_late_wakeups() {
    let interval = std::time::Duration::from_nanos(16_666_667);
    let start = Instant::now();
    let mut anchor = start;

    for step in 1..=240u32 {
        let observed = anchor + interval + std::time::Duration::from_micros(750);
        anchor = next_frame_anchor(Some(anchor), observed, Some(interval));
        assert_eq!(anchor, start + interval * step);
    }
}

#[test]
fn frame_anchor_reanchors_after_a_full_interval_overrun() {
    let interval = std::time::Duration::from_nanos(16_666_667);
    let previous = Instant::now();
    let overrun = previous + interval * 4;

    assert_eq!(
        next_frame_anchor(Some(previous), overrun, Some(interval)),
        overrun
    );
}

#[test]
fn frame_anchor_falls_back_to_the_observed_start_without_a_cadence() {
    let interval = std::time::Duration::from_nanos(16_666_667);
    let now = Instant::now();

    assert_eq!(next_frame_anchor(None, now, Some(interval)), now);
    assert_eq!(next_frame_anchor(Some(now), now, None), now);
    assert_eq!(
        next_frame_anchor(Some(now), now, Some(std::time::Duration::ZERO)),
        now
    );
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
    assert!(!primary_frame_waker_uses_event_proxy(true, true, false));
    assert!(primary_frame_waker_uses_event_proxy(true, false, false));
    assert!(primary_frame_waker_uses_event_proxy(true, true, true));
}

#[test]
fn off_thread_primary_frame_waker_always_uses_event_loop_proxy() {
    assert!(primary_frame_waker_uses_event_proxy(false, true, false));
    assert!(primary_frame_waker_uses_event_proxy(false, false, false));
    assert!(primary_frame_waker_uses_event_proxy(false, true, true));
}

fn counting(counter: &Arc<AtomicUsize>) -> impl Fn() + Send + Sync + 'static {
    let counter = Arc::clone(counter);
    move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn primary_frame_waker_on_the_event_loop_thread_redraws_a_shown_window_directly() {
    let redraws = Arc::new(AtomicUsize::new(0));
    let wake_ups = Arc::new(AtomicUsize::new(0));
    let shown = Arc::new(AtomicBool::new(true));
    let waker = primary_frame_waker(
        std::thread::current().id(),
        Arc::clone(&shown),
        false,
        counting(&redraws),
        counting(&wake_ups),
    );

    waker();
    assert_eq!(redraws.load(Ordering::SeqCst), 1);
    assert_eq!(wake_ups.load(Ordering::SeqCst), 0);

    shown.store(false, Ordering::SeqCst);
    waker();
    assert_eq!(redraws.load(Ordering::SeqCst), 1);
    assert_eq!(wake_ups.load(Ordering::SeqCst), 1);
}

#[test]
fn background_ui_post_returns_without_the_event_loop_thread() {
    let (main_queue, main_queue_jobs) = mpsc::channel::<mpsc::SyncSender<()>>();
    let wake_ups = Arc::new(AtomicUsize::new(0));
    let scheduler = Arc::new(StdScheduler::new());
    scheduler.set_frame_waker(primary_frame_waker(
        std::thread::current().id(),
        Arc::new(AtomicBool::new(true)),
        false,
        move || {
            let (serviced, wait_for_main_thread) = mpsc::sync_channel(0);
            if main_queue.send(serviced).is_ok() {
                let _ = wait_for_main_thread.recv();
            }
        },
        counting(&wake_ups),
    ));
    let runtime = Runtime::new(scheduler.clone());
    let dispatcher = runtime.handle().dispatcher();
    let (posted, post_returned) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        dispatcher.post(|| {});
        let _ = posted.send(());
    });

    let returned = post_returned.recv_timeout(Duration::from_secs(5)).is_ok();
    let mut redraws_waiting_on_main_thread = 0;
    for serviced in main_queue_jobs.try_iter() {
        redraws_waiting_on_main_thread += 1;
        let _ = serviced.send(());
    }
    let worker_finished = worker.join().is_ok();

    assert!(
        returned,
        "a background post waited for the event loop thread"
    );
    assert!(worker_finished);
    assert_eq!(redraws_waiting_on_main_thread, 0);
    assert_eq!(wake_ups.load(Ordering::SeqCst), 1);
    assert!(scheduler.has_frame_request());
    assert!(runtime.handle().has_pending_ui());
}

#[test]
fn the_primary_window_shows_only_content_of_its_own_and_never_headless() {
    assert!(primary_window_should_show(false, true));
    assert!(!primary_window_should_show(false, false));
    assert!(!primary_window_should_show(true, true));
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
fn a_window_back_on_screen_is_owed_the_frame_it_could_not_present() {
    assert!(occlusion_leaves_a_frame_owed(false));
    assert!(!occlusion_leaves_a_frame_owed(true));
    assert!(
        surface_present_required(occlusion_leaves_a_frame_owed(false), false, false),
        "a reappearing window presents even when nothing new was drawn"
    );
}

#[test]
fn a_hidden_native_window_holds_its_redraw_until_it_shows() {
    assert!(native_window_redraw_held_while_hidden(false));
    assert!(!native_window_redraw_held_while_hidden(true));
}

#[test]
fn initial_present_self_heals_when_redraw_request_is_dropped() {
    assert!(initial_present_redraw_needed(true, false));
    assert!(!initial_present_redraw_needed(true, true));
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
fn a_new_window_takes_over_a_held_press_when_the_node_that_took_it_moved_there() {
    let down = NativeWindowPointerState {
        position: PhysicalPosition::new(40.0, 30.0),
        primary_down: true,
    };
    let up = NativeWindowPointerState {
        primary_down: false,
        ..down
    };
    assert_eq!(
        held_press_to_hand_over(Some(down), true, PressBelongsHere::ItsNodeMovedHere, |_| {
            panic!("the window the node moved to does not have to be under the pointer")
        }),
        Some(down)
    );
    assert_eq!(
        held_press_to_hand_over(Some(down), true, PressBelongsHere::AskTheRectangle, |_| {
            true
        }),
        Some(down),
        "a press carried by a node that stayed where it was goes to the window under the pointer"
    );
    assert_eq!(
        held_press_to_hand_over(
            Some(down),
            true,
            PressBelongsHere::AskTheRectangle,
            |position| position.x > 100.0
        ),
        None,
        "and to no window elsewhere"
    );
    assert_eq!(
        held_press_to_hand_over(
            Some(down),
            false,
            PressBelongsHere::ItsNodeMovedHere,
            |_| true
        ),
        None,
        "a hidden window, or one appearing during a window drag, takes nothing"
    );
    assert_eq!(
        held_press_to_hand_over(Some(up), true, PressBelongsHere::ItsNodeMovedHere, |_| true),
        None,
        "a released button is no press to hand over"
    );
    assert_eq!(
        held_press_to_hand_over(None, true, PressBelongsHere::ItsNodeMovedHere, |_| true),
        None
    );
}

#[test]
fn a_press_belongs_to_the_window_drawing_the_node_that_took_it() {
    assert_eq!(
        press_belongs_here(Some(RootId::Window(7)), RootId::Window(7)),
        PressBelongsHere::ItsNodeMovedHere
    );
    assert_eq!(
        press_belongs_here(Some(RootId::Primary), RootId::Window(7)),
        PressBelongsHere::AskTheRectangle,
        "a gesture whose node stayed behind is still a gesture this window may be taking over"
    );
    assert_eq!(
        press_belongs_here(None, RootId::Window(7)),
        PressBelongsHere::AskTheRectangle
    );
}

#[test]
fn a_press_the_platform_reports_is_handed_over_without_a_relay() {
    let platform = NativeWindowPointerState {
        position: PhysicalPosition::new(40.0, 30.0),
        primary_down: true,
    };
    let held = NativeWindowPointerState {
        position: PhysicalPosition::new(1.0, 2.0),
        primary_down: true,
    };
    assert_eq!(
        press_to_hand_over(
            Some(platform),
            Some((WinitWindowId::from_raw(1), held)),
            true,
            PressBelongsHere::AskTheRectangle,
            |_| true
        ),
        Some(PressToHandOver {
            pointer: held,
            relayed_by: None,
        }),
        "a platform that reports the pointer polls the drag itself, but it reports where the \
         pointer is now, and the window was placed for where the pointer was when the frame \
         asked for it; handing the press on at the newer reading drops it past the grip the \
         frame put under it, so the press goes on at the reading the frame had"
    );
    assert_eq!(
        press_to_hand_over(
            Some(platform),
            None,
            true,
            PressBelongsHere::AskTheRectangle,
            |_| true
        ),
        Some(PressToHandOver {
            pointer: platform,
            relayed_by: None,
        }),
        "with no window holding the press the platform reading is all there is"
    );
}

#[test]
fn a_press_a_window_holds_is_handed_over_and_relayed_when_the_platform_reports_nothing() {
    let holder = WinitWindowId::from_raw(1);
    let held = NativeWindowPointerState {
        position: PhysicalPosition::new(40.0, 30.0),
        primary_down: true,
    };
    assert_eq!(
        press_to_hand_over(
            None,
            Some((holder, held)),
            true,
            PressBelongsHere::AskTheRectangle,
            |_| true
        ),
        Some(PressToHandOver {
            pointer: held,
            relayed_by: Some(holder),
        }),
        "the window that got the button keeps its events, so it relays them"
    );
    assert_eq!(
        press_to_hand_over(
            None,
            Some((holder, held)),
            true,
            PressBelongsHere::AskTheRectangle,
            |position| position.x > 100.0
        ),
        None,
        "the new window still has to be under the press"
    );
    assert_eq!(
        press_to_hand_over(None, None, true, PressBelongsHere::AskTheRectangle, |_| {
            true
        }),
        None
    );
}

#[test]
fn a_held_press_follows_the_button_on_the_window_that_got_it() {
    let down = PhysicalPosition::new(10.0, 20.0);
    let moved = PhysicalPosition::new(30.0, 40.0);
    assert_eq!(
        held_press_after_step(None, HeldPressStep::Pressed(down)),
        Some(down)
    );
    assert_eq!(
        held_press_after_step(Some(down), HeldPressStep::Moved(moved)),
        Some(moved)
    );
    assert_eq!(
        held_press_after_step(None, HeldPressStep::Moved(moved)),
        None,
        "a move with the button up holds nothing"
    );
    assert_eq!(
        held_press_after_step(Some(moved), HeldPressStep::Released(moved)),
        None
    );
}

#[test]
fn only_the_primary_button_steps_a_held_press() {
    use winit::event::{ButtonSource, ElementState, MouseButton, PointerSource, WindowEvent};
    let position = PhysicalPosition::new(3.0, 4.0);
    let button = |state, button| WindowEvent::PointerButton {
        device_id: None,
        state,
        position,
        primary: true,
        button,
        is_macos_activation_click: false,
    };
    assert_eq!(
        held_press_step(&button(
            ElementState::Pressed,
            ButtonSource::Mouse(MouseButton::Left)
        )),
        Some(HeldPressStep::Pressed(position))
    );
    assert_eq!(
        held_press_step(&button(
            ElementState::Released,
            ButtonSource::Mouse(MouseButton::Left)
        )),
        Some(HeldPressStep::Released(position))
    );
    assert_eq!(
        held_press_step(&button(
            ElementState::Pressed,
            ButtonSource::Mouse(MouseButton::Right)
        )),
        None,
        "a secondary button is no press to hand over"
    );
    assert_eq!(
        held_press_step(&WindowEvent::PointerMoved {
            device_id: None,
            position,
            primary: true,
            source: PointerSource::Mouse,
        }),
        Some(HeldPressStep::Moved(position))
    );
    assert_eq!(held_press_step(&WindowEvent::Focused(true)), None);
}

#[test]
fn a_handed_press_relays_only_the_holders_events() {
    let handed = HandedPress {
        holder: WinitWindowId::from_raw(1),
        taker: WinitWindowId::from_raw(2),
    };
    assert_eq!(
        handed.taker_for(WinitWindowId::from_raw(1)),
        Some(WinitWindowId::from_raw(2))
    );
    assert_eq!(
        handed.taker_for(WinitWindowId::from_raw(2)),
        None,
        "the taker gets no events of its own to relay"
    );
    assert_eq!(handed.taker_for(WinitWindowId::from_raw(3)), None);
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
fn primary_pointer_poll_synchronizes_global_position_before_releasing() {
    let position = PhysicalPosition::new(112.0, 57.0);

    assert_eq!(
        primary_pointer_gesture_poll_action(
            true,
            false,
            Some(NativeWindowPointerState {
                position,
                primary_down: false,
            }),
        ),
        PrimaryPointerGesturePollAction::ReleaseAt(position)
    );
}

#[test]
fn primary_pointer_poll_preserves_pressed_recovery_state() {
    let pointer = NativeWindowPointerState {
        position: PhysicalPosition::new(112.0, 57.0),
        primary_down: true,
    };

    assert_eq!(
        primary_pointer_gesture_poll_action(true, false, Some(pointer)),
        PrimaryPointerGesturePollAction::Pressed(pointer)
    );
}

#[test]
fn primary_pointer_poll_ignores_inactive_synthetic_and_unavailable_input() {
    let pointer = Some(NativeWindowPointerState {
        position: PhysicalPosition::new(112.0, 57.0),
        primary_down: false,
    });

    assert_eq!(
        primary_pointer_gesture_poll_action(false, false, pointer),
        PrimaryPointerGesturePollAction::Inactive
    );
    assert_eq!(
        primary_pointer_gesture_poll_action(true, true, pointer),
        PrimaryPointerGesturePollAction::Inactive
    );
    assert_eq!(
        primary_pointer_gesture_poll_action(true, false, None),
        PrimaryPointerGesturePollAction::Inactive
    );
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
    let (mut controller, _robot) = RobotController::new(|| {});

    assert!(!controller.synthetic_primary_down());
    controller.begin_synthetic_primary_gesture();
    assert!(controller.synthetic_primary_down());
    controller.end_synthetic_primary_gesture();
    assert!(!controller.synthetic_primary_down());
}

#[cfg(feature = "robot")]
#[test]
fn robot_controller_stages_commands_during_continuous_frame_wakes() {
    let (mut controller, robot) = RobotController::new(|| {});
    robot
        .command_sender()
        .send(crate::robot::RobotCommand::PumpFrames { count: 2 })
        .expect("robot command channel should remain open");

    assert!(controller.stage_pending_command());
    assert!(matches!(
        controller.next_command(),
        Some(crate::robot::RobotCommand::PumpFrames { count: 2 })
    ));
    assert!(!controller.stage_pending_command());
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

#[cfg(feature = "robot")]
#[test]
fn robot_screenshot_honors_capture_scale() {
    let resolved = resolve_robot_screenshot_params_with_scale((0, 0), Some((100.0, 50.0)), 2.0);
    assert_eq!(resolved, (200, 100, 2.0));

    assert_eq!(parse_robot_capture_scale(Some("2")), 2.0);
    assert_eq!(
        parse_robot_capture_scale(Some("999")),
        1.0,
        "out-of-range scale is ignored"
    );
    assert_eq!(parse_robot_capture_scale(Some("junk")), 1.0);
    assert_eq!(parse_robot_capture_scale(None), 1.0);
}

#[test]
fn a_primary_that_wraps_its_content_asks_for_each_new_content_size_once() {
    let content = |width, height| Some(cranpose_ui::Size::new(width, height));
    assert_eq!(
        super::primary_wrap_request(None, None),
        None,
        "nothing laid out, nothing asked"
    );
    assert_eq!(
        super::primary_wrap_request(content(0.0, 40.0), None),
        None,
        "content without area is nothing to wrap"
    );
    assert_eq!(
        super::primary_wrap_request(content(275.0, 463.5), None),
        Some((275, 464)),
        "the window is whole pixels around the content"
    );
    assert_eq!(
        super::primary_wrap_request(content(275.0, 463.5), Some((275, 464))),
        None,
        "the size already asked for is not asked for again"
    );
    assert_eq!(
        super::primary_wrap_request(content(275.0, 348.0), Some((275, 464))),
        Some((275, 348)),
        "a pane gone shrinks the window"
    );
}

#[test]
fn a_hidden_primarys_update_is_paced_while_an_animation_still_needs_frames() {
    let started = Instant::now();
    let interval = Some(std::time::Duration::from_millis(16));
    assert_eq!(
        super::declaration_host_frame_anchor(None, started, interval, false, false),
        None,
        "nothing presented and nothing running: no cap"
    );
    assert_eq!(
        super::declaration_host_frame_anchor(None, started, interval, true, false),
        Some(started),
        "a presented frame anchors the next"
    );
    assert_eq!(
        super::declaration_host_frame_anchor(None, started, interval, false, true),
        Some(started),
        "an animation that only dirties a peer's scene is paced like a presented frame"
    );
    let previous = Some(started - std::time::Duration::from_millis(1));
    assert_eq!(
        super::declaration_host_frame_anchor(previous, started, interval, false, false),
        previous,
        "an idle update leaves the anchor where it was"
    );
}

#[test]
fn a_peer_window_redraws_when_its_scene_is_dirty_even_with_no_frame_owed() {
    assert!(!super::native_surface_needs_frame(
        false, false, false, false
    ));
    assert!(super::native_surface_needs_frame(true, false, false, false));
    assert!(super::native_surface_needs_frame(false, true, false, false));
    assert!(
        super::native_surface_needs_frame(false, false, true, false),
        "an animated layer in a peer dirties the scene without owing a frame"
    );
    assert!(
        super::native_surface_needs_frame(false, false, false, true),
        "a window whose present was skipped, because the desktop had it covered, still owes \
         the frame it drew; nothing else asks for it once the scene is clean again"
    );
}

#[test]
fn a_transparent_window_takes_the_alpha_mode_the_platform_composites_with() {
    use wgpu::CompositeAlphaMode::{Opaque, PostMultiplied, PreMultiplied};
    assert_eq!(
        super::transparent_alpha_mode(&[Opaque, PreMultiplied, PostMultiplied]),
        Some(PreMultiplied),
        "the frame is premultiplied, so that mode comes first"
    );
    assert_eq!(
        super::transparent_alpha_mode(&[Opaque, PostMultiplied]),
        Some(PostMultiplied),
        "Metal offers no premultiplied mode and composites its non-opaque layer as one; \
         the opaque fallback painted every transparent window black"
    );
    assert_eq!(super::transparent_alpha_mode(&[Opaque]), Some(Opaque));
    assert_eq!(super::transparent_alpha_mode(&[]), None);
}

#[test]
fn a_window_shown_again_is_new_to_the_press_that_is_still_down() {
    assert!(
        super::window_shown_again_takes_a_held_press(true, true),
        "a pane docked and torn out again reaches the desktop as a window it already has, \
         hidden; the press that tore it has to go to it the way it goes to a window that \
         was made on the spot, or the window stands still where it appeared"
    );
    assert!(!super::window_shown_again_takes_a_held_press(false, true));
    assert!(!super::window_shown_again_takes_a_held_press(true, false));
    assert!(!super::window_shown_again_takes_a_held_press(false, false));
}
