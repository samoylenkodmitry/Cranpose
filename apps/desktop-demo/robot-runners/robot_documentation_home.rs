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

pub(crate) fn main() {
    AppLauncher::new()
        .with_title("Cranpose documentation")
        .with_size(1000, 760)
        .with_headless(true)
        .with_test_driver(|robot| {
            robot.wait_for_idle().expect("documentation startup");
            robot.validate_content("Build native and browser interfaces in Rust.")
                .expect("offline documentation opens first");
            robot_shot::save_checked(
                &output_paths::diagnostic_path("documentation-home.png"),
                &robot.screenshot().expect("documentation screenshot"),
            ).expect("save documentation screenshot");
            click_button(&robot, "Get started");
            robot.validate_content("Create an application").expect("chapter changed");
            robot_shot::save_checked(
                &output_paths::diagnostic_path("documentation-get-started.png"),
                &robot.screenshot().expect("chapter screenshot"),
            ).expect("save chapter screenshot");
            robot.move_to(500.0, 500.0).expect("hover document");
            robot.mouse_scroll_and_wait_for_frame(0.0, -420.0).expect("scroll guide");
            robot.validate_content("use cranpose::prelude::*;").expect("code is readable");
            click_button(&robot, "Counter App");
            robot.validate_content("Increment").expect("existing counter remains available");
            click_button(&robot, "Documentation");
            robot.validate_content("Build native and browser interfaces in Rust.")
                .expect("return to documentation");
            robot.exit().expect("exit documentation robot");
        })
        .run(app::DesktopApp);
}
