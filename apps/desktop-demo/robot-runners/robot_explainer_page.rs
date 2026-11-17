// The Explainer tab's page: the wheel brings the sheet into view; a click
// on each language's chip sets the sheet's words in that language, Arabic
// laid out from the right; the size slider, dragged to its end, sets the
// type larger. A picture after each step.

use std::{path::Path, time::Duration};

use cranpose::Robot;

use crate::{
    explainer_robot::{click_text, drag, save, scroll_to, text_bounds, wait},
    output_paths, robot_exit, robot_launch,
};

const WIDTH: f32 = 1200.0;
const HEIGHT: f32 = 800.0;
// The headline in each language, with the chip that switches to it.
const HEADLINES: [(&str, &str, &str); 4] = [
    ("Español", "Sigue siendo", "page_es.png"),
    ("日本語", "これは", "page_ja.png"),
    ("العربية", "ما زال", "page_ar.png"),
    ("English", "It is still real", "page_en.png"),
];
// How long new words take to be pressed in.
const PRESS: Duration = Duration::from_millis(1300);

pub(crate) fn main() {
    let _ = env_logger::try_init();
    println!("=== Robot Explainer Page ===");
    robot_exit::arm_timeout(150);
    let shots = output_paths::diagnostic_path("cranpose-robot-explainer-page");
    if let Err(err) = std::fs::create_dir_all(&shots) {
        robot_exit::fail_without_shutdown(&format!("{}: {err}", shots.display()));
    }
    robot_launch::launch("Robot Explainer Page", WIDTH as u32, HEIGHT as u32)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_robot_app_hook(robot_launch::set_tab_hook)
        .with_test_driver(move |robot| {
            robot_launch::switch_tab(&robot, "explainer");
            wait(&robot, Duration::from_millis(300));
            scroll_to(&robot, &shots, "It is still real", (WIDTH, HEIGHT));
            wait(&robot, Duration::from_millis(600));
            save(&robot, &shots, "page_start.png");
            for (chip, headline, picture) in HEADLINES {
                switch_language(&robot, &shots, (chip, headline, picture));
            }
            grow_type(&robot, &shots);
            println!("PASS: page shots in {}", shots.display());
            robot.exit().ok();
        })
        .run(robot_launch::counter_demo);
}

// Clicks `chip` and checks that the headline reads `headline`; Arabic's
// starts on the right.
fn switch_language(robot: &Robot, shots: &Path, (chip, headline, picture): (&str, &str, &str)) {
    click_text(robot, chip);
    wait(robot, PRESS);
    save(robot, shots, picture);
    let (x, _, width, _) = text_bounds(robot, headline);
    println!("  {chip}: the headline spans x {x:.0} to {:.0}", x + width);
    if headline == "ما زال" && x + width < WIDTH * 0.75 {
        robot_exit::fail(robot, "the Arabic headline is not laid out from the right");
    }
    if headline != "ما زال" && x > WIDTH * 0.3 {
        robot_exit::fail(
            robot,
            &format!("the {chip} headline is not laid out from the left"),
        );
    }
}

// Drags the size slider's thumb to the right end and checks that the
// headline grows.
fn grow_type(robot: &Robot, shots: &Path) {
    let (_, _, _, before) = text_bounds(robot, "It is still real");
    let (x, y, width, height) = text_bounds(robot, "Type size");
    let u = (WIDTH / 1600.0).min(HEIGHT / 1000.0);
    let track = x + width + 18.0 * u;
    let thumb = track + (236.0 * u - 36.0 * u) * 0.5 + 18.0 * u;
    drag(robot, (thumb, y + height * 0.5), track + 236.0 * u + 20.0);
    wait(robot, Duration::from_millis(400));
    save(robot, shots, "page_large.png");
    let (_, _, _, after) = text_bounds(robot, "It is still real");
    println!(
        "  the headline is {before:.0} px tall at the middle size, {after:.0} px at the largest"
    );
    if after < before * 1.12 {
        robot_exit::fail(
            robot,
            "dragging the size slider did not set the type larger",
        );
    }
}
