use cranpose_app_shell::FrameUpdateResult;

use super::*;

#[test]
fn desktop_robot_queries_schedule_presentation_for_drained_visual_updates() {
    assert!(robot_query_visual_dirty(
        FrameUpdateResult {
            visual_changed: true,
            ..FrameUpdateResult::default()
        },
        false,
    ));
    assert!(robot_query_visual_dirty(FrameUpdateResult::default(), true,));
    assert!(!robot_query_visual_dirty(
        FrameUpdateResult {
            structure_changed: true,
            ..FrameUpdateResult::default()
        },
        false,
    ));
}

#[test]
fn a_driven_run_never_parks_longer_than_its_command_poll() {
    let now = Instant::now();
    let far = now + Duration::from_secs(5);
    let soon = now + Duration::from_millis(1);
    let bound = now + ROBOT_PARKED_COMMAND_POLL_INTERVAL;

    assert_eq!(
        bound_robot_park(ControlFlow::Wait, now),
        ControlFlow::WaitUntil(bound)
    );
    assert_eq!(
        bound_robot_park(ControlFlow::WaitUntil(far), now),
        ControlFlow::WaitUntil(bound)
    );
    assert_eq!(
        bound_robot_park(ControlFlow::WaitUntil(soon), now),
        ControlFlow::WaitUntil(soon)
    );
    assert_eq!(bound_robot_park(ControlFlow::Poll, now), ControlFlow::Poll);
}

#[test]
fn an_idle_robot_lets_the_loop_park() {
    let (mut controller, robot) = RobotController::new(|| {});

    assert!(!controller.awaiting_progress());

    robot
        .command_sender()
        .send(RobotCommand::PumpFrames { count: 1 })
        .expect("robot command channel should remain open");
    assert!(controller.awaiting_progress());
    assert!(matches!(
        controller.next_command(),
        Some(RobotCommand::PumpFrames { count: 1 })
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
            started_at + ROBOT_IDLE_STARVATION_CEILING - Duration::from_millis(1)
        ),
        None
    );
}

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
        controller
            .idle_wait_timeout(started_at + ROBOT_IDLE_PRESENT_TIMEOUT - Duration::from_millis(1)),
        None
    );
}

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

#[test]
fn a_present_wait_gives_up_when_the_surface_never_presents() {
    let (mut controller, _robot) = RobotController::new(|| {});
    let started_at = Instant::now();

    controller.begin_pump_present_wait_at(11, started_at);
    assert!(!controller.pump_present_wait_timed_out(started_at));
    assert!(!controller.pump_present_wait_timed_out(started_at + ROBOT_PRESENT_WAIT_TIMEOUT / 2));
    assert!(controller.pump_present_wait_timed_out(started_at + ROBOT_PRESENT_WAIT_TIMEOUT));
}

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

#[test]
fn each_presented_scroll_step_restarts_the_wait_deadline() {
    let (mut controller, _robot) = RobotController::new(|| {});
    let started_at = Instant::now();

    controller.begin_pump_present_wait_at(11, started_at);
    let next_step = started_at + ROBOT_PRESENT_WAIT_TIMEOUT - Duration::from_millis(1);
    controller.begin_pump_present_wait_at(12, next_step);

    assert!(!controller.pump_present_wait_timed_out(next_step + Duration::from_millis(2)));
    assert!(controller.pump_present_wait_timed_out(next_step + ROBOT_PRESENT_WAIT_TIMEOUT));
}

#[test]
fn visible_robot_wait_requires_present_after_visual_mutation() {
    assert_eq!(robot_visible_present_target(true, false, true, 7), Some(8));
    assert_eq!(robot_visible_present_target(true, false, false, 7), None);
    assert_eq!(robot_visible_present_target(true, true, true, 7), None);
    assert_eq!(robot_visible_present_target(false, false, true, 7), None);
}

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
fn robot_controller_tracks_synthetic_primary_button_lifetime() {
    let (mut controller, _robot) = RobotController::new(|| {});

    assert!(!controller.synthetic_primary_down());
    controller.begin_synthetic_primary_gesture();
    assert!(controller.synthetic_primary_down());
    controller.end_synthetic_primary_gesture();
    assert!(!controller.synthetic_primary_down());
}

#[test]
fn robot_controller_stages_commands_during_continuous_frame_wakes() {
    let (mut controller, robot) = RobotController::new(|| {});
    robot
        .command_sender()
        .send(RobotCommand::PumpFrames { count: 2 })
        .expect("robot command channel should remain open");

    assert!(controller.stage_pending_command());
    assert!(matches!(
        controller.next_command(),
        Some(RobotCommand::PumpFrames { count: 2 })
    ));
    assert!(!controller.stage_pending_command());
}

#[test]
fn robot_screenshot_prefers_logical_viewport_size() {
    let resolved = resolve_robot_screenshot_params((1600, 1200), Some((800.0, 600.0)));
    assert_eq!(resolved, (800, 600, 1.0));
}

#[test]
fn robot_screenshot_uses_ceil_on_fractional_logical_size() {
    let resolved = resolve_robot_screenshot_params((0, 0), Some((801.2, 601.3)));
    assert_eq!(resolved, (802, 602, 1.0));
}

#[test]
fn robot_screenshot_falls_back_to_physical_buffer_when_layout_is_missing() {
    let resolved = resolve_robot_screenshot_params((1600, 1200), None);
    assert_eq!(resolved, (1600, 1200, 1.0));
}

#[test]
fn robot_screenshot_clamps_to_non_zero_target() {
    let resolved = resolve_robot_screenshot_params((0, 0), Some((10.0, 20.0)));
    assert_eq!(resolved, (10, 20, 1.0));
}

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
