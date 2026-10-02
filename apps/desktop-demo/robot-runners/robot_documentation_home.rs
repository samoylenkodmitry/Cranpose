use cranpose::{AppLauncher, Robot};
use cranpose_testing::{find_button_exact_in_semantics, find_text_in_semantics, find_text_in_semantics_exact};
use desktop_app::app;

use crate::{output_paths, robot_shot};

fn click_button(robot: &Robot, label: &str) {
    let (x, y, width, height) =
        find_button_exact_in_semantics(robot, label).expect("documentation button");
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
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(true)
        .with_test_driver(move |robot| {
            robot.wait_for_idle().expect("documentation startup");
            robot
                .validate_content("Build native and browser interfaces in Rust.")
                .expect("offline documentation opens first");
            capture(&robot, width, "home");
            if compact {
                click_button(&robot, "Sections");
                capture(&robot, width, "sections");
            }
            click_button(&robot, "Get started");
            robot
                .validate_content("Create an application")
                .expect("chapter changed");
            capture(&robot, width, "get-started");
            let tabs_before = (!compact)
                .then(|| find_text_in_semantics_exact(&robot, "Counter App").expect("tab row"));
            let (_, title_y, _, _) =
                find_text_in_semantics(&robot, "Create an application").expect("article heading");
            robot
                .move_to(width as f32 * 0.90, height as f32 * 0.70)
                .expect("hover document");
            robot
                .mouse_scroll_sequence_and_wait_for_frames(0.0, -90.0, 3)
                .expect("scroll the whole guide");
            robot.wait_for_idle().expect("page settles");
            capture(&robot, width, "scrolling");
            let (_, scrolled_title_y, _, _) =
                find_text_in_semantics(&robot, "Create an application")
                    .expect("scrolled article heading");
            assert!(title_y - scrolled_title_y > 50.0, "the page must scroll: {title_y} -> {scrolled_title_y}");
            if let Some((_, tabs_y, _, _)) = tabs_before {
                let (_, scrolled_tabs_y, _, _) =
                    find_text_in_semantics_exact(&robot, "Counter App")
                        .expect("scrolled tab row");
                assert!(
                    (tabs_y - scrolled_tabs_y - (title_y - scrolled_title_y)).abs() < 1.0,
                    "tabs and article share the same page scroll"
                );
                assert!(scrolled_tabs_y < 0.0, "tabs scroll out of view");
            }
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
            robot
                .mouse_scroll_sequence_and_wait_for_frames(0.0, -12000.0, 25)
                .expect("scroll to the end");
            robot.wait_for_idle().expect("end of page");
            capture(&robot, width, "end");
            click_button(&robot, "Back to top");
            robot
                .validate_content("Create an application")
                .expect("return to article top");
            if !compact {
                click_button(&robot, "Counter App");
                robot
                    .validate_content("Increment")
                    .expect("existing counter remains available");
                click_button(&robot, "Documentation");
                robot
                    .validate_content("Build native and browser interfaces in Rust.")
                    .expect("return to documentation");
            }
            robot.exit().expect("exit documentation robot");
        })
        .run(app::DesktopApp);
}
