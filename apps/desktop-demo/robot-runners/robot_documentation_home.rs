use cranpose::{AppLauncher, Robot};
use cranpose_testing::{find_button_exact_in_semantics, find_text_in_semantics, find_text_in_semantics_exact};
use desktop_app::app;

use crate::{output_paths, robot_shot};

fn click_button(robot: &Robot, label: &str) {
    let Some((x, y, width, height)) = find_button_exact_in_semantics(robot, label) else {
        robot_shot::save_checked(
            &output_paths::diagnostic_path("documentation-missing-control.png"),
            &robot.screenshot().expect("missing control screenshot"),
        ).expect("save missing control screenshot");
        panic!("documentation button {label:?}");
    };
    robot
        .click(x + width * 0.5, y + height * 0.5)
        .expect("click documentation control");
    robot.wait_for_idle().expect("documentation settles");
}

fn capture(robot: &Robot, width: u32, stage: &str) {
    let filename = format!("documentation-{width}-{stage}.png");
    robot_shot::save_checked(
        &output_paths::diagnostic_path(&filename),
        &robot.screenshot().expect("documentation screenshot"),
    )
    .expect("save documentation screenshot");
}

fn return_to_top(robot: &Robot, width: u32, height: u32) {
    let (_, y, _, _) = find_button_exact_in_semantics(robot, "Back to top")
        .expect("reader footer");
    if y < 80.0 {
        robot.move_to(width as f32 * 0.9, height as f32 * 0.6).expect("hover reader");
        robot.mouse_scroll_and_wait_for_frame(0.0, 120.0 - y).expect("reveal footer below the back bar");
        robot.wait_for_idle().expect("footer settles");
    }
    click_button(robot, "Back to top");
    robot.validate_content("Build native and browser interfaces in Rust.").expect("top of guide is visible");
}

pub(crate) fn main() {
    let (width, height) = match std::env::var("CRANPOSE_DOCS_VIEWPORT").as_deref() {
        Ok("compact") => (390, 780),
        Ok("short") => (800, 600),
        Ok("wide") => (1440, 1000),
        Ok("desktop") | Err(std::env::VarError::NotPresent) => (1100, 820),
        other => panic!("invalid documentation viewport: {other:?}"),
    };
    let compact = width < 780;
    AppLauncher::new()
        .with_title("Cranpose documentation")
        .with_size(width, height)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(true)
        .with_test_driver(move |robot| {
            robot.wait_for_idle().expect("documentation startup");
            if compact {
                robot.validate_content("Welcome").expect("compact wheel opens first");
                assert!(find_text_in_semantics(&robot, "Build native and browser interfaces in Rust.").is_none());
            } else {
                robot.validate_content("Build native and browser interfaces in Rust.").expect("offline reader");
            }
            capture(&robot, width, "home");
            let github_before = find_text_in_semantics_exact(&robot, "View on GitHub").expect("fixed repository link");
            assert!(github_before.0 < 100.0 && github_before.1 > height as f32 - 55.0);
            robot.move_to(160.0, height as f32 * 0.65).expect("hover wheel");
            robot.mouse_scroll_sequence_and_wait_for_frames(0.0, -150.0, 5).expect("rotate wheel independently");
            robot.wait_for_idle().expect("wheel settles");
            capture(&robot, width, "wheel-scrolling");
            if !compact {
                robot.validate_content("Create an application").expect("wheel scroll navigates the document");
            }
            click_button(&robot, "Get started");
            robot.validate_content("Create an application").expect("selected chapter is readable");
            capture(&robot, width, "get-started");
            if !compact {
                if let Some((_, y, _, height)) = find_text_in_semantics_exact(&robot, "Counter App") {
                    assert!(y + height <= 0.0, "the tabs must scroll completely out of view: y={y}, height={height}");
                }
            }
            let (_, title_y, _, _) = find_text_in_semantics(&robot, "Create an application").expect("article heading");
            robot.move_to(width as f32 * 0.90, height as f32 * 0.70).expect("hover document");
            robot.mouse_scroll_sequence_and_wait_for_frames(0.0, -90.0, 3).expect("scroll reader");
            robot.wait_for_idle().expect("reader settles");
            capture(&robot, width, "scrolling");
            let (_, scrolled_title_y, _, _) = find_text_in_semantics(&robot, "Create an application").expect("scrolled article heading");
            assert!(title_y - scrolled_title_y > 50.0, "the document must scroll");
            let github_after = find_text_in_semantics_exact(&robot, "View on GitHub").expect("fixed repository link after scrolling");
            assert!((github_after.1 - github_before.1).abs() < 1.0);
            for _ in 0..30 {
                let (_, y, _, _) = find_text_in_semantics(&robot, "use cranpose::prelude::*;")
                    .expect("code block");
                if y >= 0.0 && y < height as f32 * 0.65 {
                    break;
                }
                robot
                    .mouse_scroll_and_wait_for_frame(0.0, -90.0)
                    .expect("scroll to code");
            }
            capture(&robot, width, "code");
            if compact {
                click_button(&robot, "Back to wheel");
                capture(&robot, width, "back-to-wheel");
                assert!(find_text_in_semantics(&robot, "Create an application").is_none());
                click_button(&robot, "Get started");
                robot.validate_content("Create an application").expect("reopen the reader");
            }
            return_to_top(&robot, width, height);
            capture(&robot, width, "back-to-top");
            if !compact {
                click_button(&robot, "Counter App");
                robot.validate_content("Increment").expect("existing demo remains available");
                click_button(&robot, "Documentation");
                robot.validate_content("Build native and browser interfaces in Rust.").expect("return to documentation");
            } else {
                click_button(&robot, "Back to wheel");
            }
            robot.drag_and_wait_for_frames(120.0, 550.0, 120.0, 454.0, 8).expect("flick the wheel");
            let released = find_text_in_semantics_exact(&robot, "Get started").expect("wheel entry after release");
            robot.pump_frames(30).expect("wheel coast frames");
            let coasted = find_text_in_semantics_exact(&robot, "Get started").expect("wheel entry after coasting");
            assert!(coasted.1 < released.1 - 10.0, "wheel must continue rotating after release: {released:?} -> {coasted:?}");
            robot.touch_down(120.0, 550.0).expect("catch the spinning wheel");
            capture(&robot, width, "fling");
            robot.touch_up(120.0, 550.0).expect("release the stopped wheel");
            robot.exit().expect("exit documentation robot");
        })
        .run(app::DesktopApp);
}
