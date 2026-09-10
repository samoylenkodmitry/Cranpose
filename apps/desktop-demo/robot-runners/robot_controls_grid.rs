mod named_semantics;
mod robot_exit;

use std::time::Duration;

use cranpose::{AppLauncher, Robot};
use cranpose_testing::changed_pixel_count_in_region;
use desktop_app::app;
use named_semantics::{expect_reading, named_control};

const WINDOW_WIDTH: u32 = 760;
const WINDOW_HEIGHT: u32 = 900;
/// How much of the stage a press has to move for the dip to count as drawn.
const MIN_PRESSED_PIXELS: usize = 400;

fn card(robot: &Robot, title: &str) -> (String, named_semantics::Bounds) {
    named_control(robot, title)
}

fn click_stage(robot: &Robot, title: &str) {
    let (_, (x, y, width, height)) = card(robot, title);
    robot
        .click(x + width * 0.5, y + height * 0.5)
        .expect("click the control stage");
    std::thread::sleep(Duration::from_millis(220));
    let _ = robot.wait_for_idle();
}

fn main() {
    let _ = env_logger::try_init();

    AppLauncher::new()
        .with_title("Controls Grid Contract")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(600));
            let _ = robot.wait_for_idle();

            expect_reading(
                &robot,
                "Checkmark",
                "Checked",
                "the grid starts at its initial state",
            );
            expect_reading(
                &robot,
                "Toggle",
                "Off",
                "the grid starts at its initial state",
            );
            expect_reading(
                &robot,
                "Push button",
                "0 presses",
                "the grid starts at its initial state",
            );
            expect_reading(
                &robot,
                "Lever switch",
                "Off",
                "the grid starts at its initial state",
            );

            click_stage(&robot, "Checkmark");
            click_stage(&robot, "Toggle");
            click_stage(&robot, "Push button");
            click_stage(&robot, "Lever switch");

            expect_reading(
                &robot,
                "Checkmark",
                "Unchecked",
                "the first grid row takes a click",
            );
            expect_reading(
                &robot,
                "Toggle",
                "On",
                "a card in the middle grid row takes a click; a two-column window put the \
                 toggle and the button there and neither answered one",
            );
            expect_reading(
                &robot,
                "Push button",
                "1 press",
                "the other middle-row card takes a click",
            );
            expect_reading(
                &robot,
                "Lever switch",
                "On",
                "the last grid row takes a click",
            );

            expect_reading(
                &robot,
                "Slider",
                "35 %",
                "clicking other cards leaves this one alone",
            );
            expect_reading(
                &robot,
                "Volume dial",
                "40 %",
                "clicking other cards leaves this one alone",
            );

            let (_, stage) = card(&robot, "Toggle");
            let idle = robot.screenshot().expect("idle screenshot");
            robot
                .mouse_move(stage.0 + stage.2 * 0.25, stage.1 + stage.3 * 0.3)
                .expect("move onto the middle-row stage away from its centre");
            std::thread::sleep(Duration::from_millis(260));
            let _ = robot.wait_for_idle();
            let hovered = robot.screenshot().expect("hover screenshot");
            robot.mouse_down().expect("press the middle-row stage");
            std::thread::sleep(Duration::from_millis(140));
            let _ = robot.wait_for_idle();
            let pressed = robot.screenshot().expect("press screenshot");
            robot.mouse_up().expect("release the middle-row stage");
            std::thread::sleep(Duration::from_millis(360));
            let _ = robot.wait_for_idle();

            let hover_pixels = changed_pixel_count_in_region(&idle, &hovered, stage, 6);
            let press_pixels = changed_pixel_count_in_region(&hovered, &pressed, stage, 6);
            println!("hover_pixels={hover_pixels} press_pixels={press_pixels}");
            if hover_pixels < MIN_PRESSED_PIXELS {
                robot_exit::fail_without_shutdown(&format!(
                    "hovering a middle-row stage moved {hover_pixels} pixels: its gesture \
                     handler never ran"
                ));
            }
            if press_pixels < MIN_PRESSED_PIXELS {
                robot_exit::fail_without_shutdown(&format!(
                    "pressing a middle-row stage moved {press_pixels} pixels: the press dip never \
                     reached the shader"
                ));
            }

            println!("PASS: every row of the controls grid takes a click and shows its press");
            robot.exit().expect("exit");
        })
        .run(app::ControlsUiRobotApp);
}
