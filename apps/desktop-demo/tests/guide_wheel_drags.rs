use cranpose_testing::robot::{RobotTestRule, TestRenderer};
use desktop_app::app::{combined_app_with_initial_tab, DemoTab};

const FRAME_NANOS: u64 = 16_666_667;

/// Advances one frame, then runs the idle prefetch passes the web page and
/// Android run between frames.
fn frame_with_idle_passes(robot: &mut RobotTestRule<TestRenderer>, time_ms: &mut i64) {
    robot.advance_time(FRAME_NANOS);
    *time_ms += 16;
    robot.shell_mut().run_idle_prefetch(None, |_| false);
}

fn drag(robot: &mut RobotTestRule<TestRenderer>, from: f32, to: f32, time_ms: &mut i64) {
    robot.shell_mut().set_cursor(200.0, from);
    robot.shell_mut().pointer_pressed_at_time(Some(*time_ms));
    for step in 1..=8 {
        frame_with_idle_passes(robot, time_ms);
        let y = from + (to - from) * step as f32 / 8.0;
        robot
            .shell_mut()
            .set_cursor_at_time(200.0, y, Some(*time_ms));
    }
    robot
        .shell_mut()
        .pointer_released_at_position_time(200.0, to, Some(*time_ms));
    for _ in 0..20 {
        frame_with_idle_passes(robot, time_ms);
    }
}

/// Turns the phone layout's guide wheel back and forth. Its reader drops the
/// rows of tables, which remember scroll states, in idle passes; this
/// panicked with "state cell missing" on the web page.
#[test]
fn dragging_the_compact_wheel_with_idle_passes() {
    cranpose_ui::set_idle_prefetch_passes(true);
    let mut robot = RobotTestRule::new(375, 812, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Guide));
    });
    robot.wait_for_idle();
    let mut time_ms = 0;
    for round in 0..12 {
        if round % 3 == 2 {
            drag(&mut robot, 150.0, 700.0, &mut time_ms);
        } else {
            drag(&mut robot, 650.0, 100.0, &mut time_ms);
        }
    }
}
