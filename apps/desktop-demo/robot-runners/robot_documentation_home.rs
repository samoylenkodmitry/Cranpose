use cranpose::{AppLauncher, Robot};
use cranpose_testing::find_button_exact_in_semantics;
use desktop_app::app;

use crate::{output_paths, robot_shot};

fn click_button(robot: &Robot, label: &str) {
    let (x, y, width, height) =
        find_button_exact_in_semantics(robot, label).expect("documentation button");
    robot.click(x + width * 0.5, y + height * 0.5).expect("click documentation control");
    robot.wait_for_idle().expect("documentation settles");
}

fn capture(robot: &Robot, width: u32, stage: &str) {
    let filename = format!("documentation-{width}-{stage}.png");
    robot_shot::save_checked(
        &output_paths::diagnostic_path(&filename),
        &robot.screenshot().expect("documentation screenshot"),
    ).expect("save documentation screenshot");
}

pub(crate) fn main() {
    let (width, height) = match std::env::var("CRANPOSE_DOCS_VIEWPORT").as_deref() {
        Ok("compact") => (390, 780),
        Ok("wide") => (1440, 1000),
        Ok("desktop") | Err(std::env::VarError::NotPresent) => (1100, 820),
        other => panic!("invalid documentation viewport: {other:?}"),
    };
    let compact = width < 780;
    AppLauncher::new()
        .with_title("Cranpose documentation")
        .with_size(width, height)
        .with_headless(true)
        .with_test_driver(move |robot| {
            robot.wait_for_idle().expect("documentation startup");
            robot.validate_content("Build native and browser interfaces in Rust.")
                .expect("offline documentation opens first");
            capture(&robot, width, "home");
            if compact {
                click_button(&robot, "Sections");
                capture(&robot, width, "sections");
            }
            click_button(&robot, "Get started");
            robot.validate_content("Create an application").expect("chapter changed");
            capture(&robot, width, "get-started");
            let section_bounds = (!compact).then(|| {
                find_button_exact_in_semantics(&robot, "Get started").expect("selected section")
            });
            robot.move_to(width as f32 * 0.72, height as f32 * 0.64).expect("hover document");
            robot.mouse_scroll_sequence_and_wait_for_frames(0.0, -70.0, 3)
                .expect("scroll guide through presented frames");
            capture(&robot, width, "scrolling");
            robot.wait_for_idle().expect("electric edge settles at rest");
            capture(&robot, width, "settled");
            if let Some((x, y, _, _)) = section_bounds {
                let (current_x, current_y, _, _) =
                    find_button_exact_in_semantics(&robot, "Get started").expect("section stays visible");
                assert!((x - current_x).abs() < 0.5 && (y - current_y).abs() < 0.5,
                    "reading scroll must leave the section wheel in place");
            }
            for _ in 0..12 {
                if robot.validate_content("use cranpose::prelude::*;").is_ok() {
                    break;
                }
                robot.mouse_scroll_and_wait_for_frame(0.0, -120.0).expect("scroll to code");
            }
            robot.validate_content("use cranpose::prelude::*;").expect("code is readable");
            robot.wait_for_idle().expect("reading code settles");
            capture(&robot, width, "code");
            if !compact {
                click_button(&robot, "Counter App");
                robot.validate_content("Increment").expect("existing counter remains available");
                click_button(&robot, "Documentation");
                robot.validate_content("Build native and browser interfaces in Rust.")
                    .expect("return to documentation");
            }
            robot.exit().expect("exit documentation robot");
        })
        .run(app::DesktopApp);
}
