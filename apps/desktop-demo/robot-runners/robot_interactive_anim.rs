//! Robot test for the Interactive Anim tab.

use cranpose::{AppLauncher, Robot, RobotScreenshot};
use cranpose_testing::{
    changed_pixel_count_in_region, find_button_in_semantics, find_text_in_semantics,
};
use desktop_app::app;
use std::time::Duration;

fn fail(robot: &Robot, message: &str) -> ! {
    println!("FATAL: {message}");
    robot.exit().ok();
    std::process::exit(1);
}

fn capture(robot: &Robot, label: &str) -> RobotScreenshot {
    robot
        .screenshot()
        .unwrap_or_else(|err| fail(robot, &format!("{label} screenshot failed: {err}")))
}

fn changed_pixels(
    before: &RobotScreenshot,
    after: &RobotScreenshot,
    region: (f32, f32, f32, f32),
) -> usize {
    changed_pixel_count_in_region(before, after, region, 6)
}

fn main() {
    env_logger::init();
    println!("=== Robot Interactive Anim Test ===");

    AppLauncher::new()
        .with_title("Robot Interactive Anim Test")
        .with_size(1200, 760)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            robot
                .wait_for_idle()
                .unwrap_or_else(|err| fail(&robot, &format!("initial idle failed: {err}")));

            if find_text_in_semantics(&robot, "Interactive Anim").is_none() {
                fail(&robot, "Interactive Anim tab content not visible");
            }

            let Some((button_x, button_y, button_w, button_h)) =
                find_button_in_semantics(&robot, "Tap me")
            else {
                fail(&robot, "'Tap me' button not found");
            };
            let sample_region = (
                button_x - 18.0,
                button_y - 18.0,
                button_w + 36.0,
                button_h + 36.0,
            );
            let center_x = button_x + button_w / 2.0;
            let center_y = button_y + button_h / 2.0;

            robot
                .move_to(center_x, center_y)
                .unwrap_or_else(|err| fail(&robot, &format!("move failed: {err}")));
            robot
                .wait_for_idle()
                .unwrap_or_else(|err| fail(&robot, &format!("pre-press idle failed: {err}")));
            let baseline = capture(&robot, "baseline");

            robot
                .mouse_down()
                .unwrap_or_else(|err| fail(&robot, &format!("mouse down failed: {err}")));
            robot
                .pump_frames(1)
                .unwrap_or_else(|err| fail(&robot, &format!("down pump failed: {err}")));
            let down_first = capture(&robot, "first pressed");

            robot
                .pump_frames(8)
                .unwrap_or_else(|err| fail(&robot, &format!("pressed animation pump failed: {err}")));
            let down_later = capture(&robot, "later pressed");

            let press_delta = changed_pixels(&baseline, &down_first, sample_region);
            let held_delta = changed_pixels(&down_first, &down_later, sample_region);
            if press_delta < 120 {
                fail(
                    &robot,
                    &format!("press did not produce visible button pixels: {press_delta}"),
                );
            }
            if held_delta < 20 {
                fail(
                    &robot,
                    &format!("held press animation did not advance: {held_delta}"),
                );
            }

            robot
                .mouse_up()
                .unwrap_or_else(|err| fail(&robot, &format!("mouse up failed: {err}")));
            robot
                .pump_frames(6)
                .unwrap_or_else(|err| fail(&robot, &format!("release pump failed: {err}")));
            let released_first = capture(&robot, "first released");
            robot
                .pump_frames(14)
                .unwrap_or_else(|err| fail(&robot, &format!("slow release pump failed: {err}")));
            let released_later = capture(&robot, "later released");
            let release_delta = changed_pixels(&down_later, &released_first, sample_region);
            let slow_release_delta =
                changed_pixels(&released_first, &released_later, sample_region);
            if release_delta < 80 {
                fail(
                    &robot,
                    &format!("release did not animate button back up: {release_delta}"),
                );
            }
            if slow_release_delta < 20 {
                fail(
                    &robot,
                    &format!("release animation did not continue slowly: {slow_release_delta}"),
                );
            }

            if find_text_in_semantics(&robot, "1").is_none() {
                fail(&robot, "click count did not update after release");
            }

            println!(
                "PASS: interactive press changed pixels (press={press_delta}, held={held_delta}, release={release_delta}, slow_release={slow_release_delta})"
            );
            robot.exit().ok();
        })
        .run(|| app::combined_app_with_initial_tab(Some(app::DemoTab::InteractiveAnim)));
}
