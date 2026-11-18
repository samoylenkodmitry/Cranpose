// The Explainer tab's "How it works" in a window of a phone's size: at the
// last step the board with the flat copy stands on the table in front of
// the wall, in the window's light, as it does in a wide window.

use std::time::Duration;

use crate::{
    explainer_robot::{move_to, pixels, save, text_bounds, wait, Area, Walk},
    output_paths, robot_exit, robot_launch,
};

use cranpose::Robot;

const WIDTH: f32 = 393.0;
const HEIGHT: f32 = 760.0;
// The section's height, in view heights.
const HOW_PAGES: f32 = 2.6;
// The least share of the flat copy's headline that shows paper, and of the
// stage round it that shows the window's light.
const PAPER: f32 = 0.3;
const LIGHT: f32 = 0.1;

pub(crate) fn main() {
    let _ = env_logger::try_init();
    println!("=== Robot Explainer How Phone ===");
    robot_exit::arm_timeout(120);
    let shots = output_paths::diagnostic_path("cranpose-robot-explainer-how-phone");
    if let Err(err) = std::fs::create_dir_all(&shots) {
        robot_exit::fail_without_shutdown(&format!("{}: {err}", shots.display()));
    }
    robot_launch::launch("Robot Explainer How Phone", WIDTH as u32, HEIGHT as u32)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_robot_app_hook(robot_launch::set_tab_hook)
        .with_test_driver(move |robot| {
            robot_launch::switch_tab(&robot, "explainer");
            wait(&robot, Duration::from_millis(300));
            let walk = Walk::in_window(&robot, "how", HOW_PAGES, HEIGHT);
            move_to(&robot, WIDTH * 0.5, HEIGHT * 0.5);
            walk.walk_to(&robot, 0.92);
            wait(&robot, Duration::from_millis(500));
            save(&robot, &shots, "how_phone.png");
            flat_copy_lit(&robot);
            println!("PASS: how phone shots in {}", shots.display());
            robot.exit().ok();
        })
        .run(robot_launch::counter_demo);
}

// Fails unless the flat copy shows where its words are, on paper, with the
// window's light round it. A board pushed back past where the table turns
// up into the wall hides behind the wall, and the light that follows it
// falls on the wall.
fn flat_copy_lit(robot: &Robot) {
    let (x, y, width, height) = text_bounds(robot, "One paragraph");
    let words: Area = (x, y, width, height);
    let round: Area = (x - height * 4.0, y - height * 4.0, width + height * 8.0, height * 10.0);
    let paper = share(robot, words, is_paper);
    let light = share(robot, round, is_window_light);
    println!("  flat copy at {words:?}: paper {paper:.2}, window light round it {light:.2}");
    if paper < PAPER {
        robot_exit::fail(robot, "the flat copy does not show where its words are");
    }
    if light < LIGHT {
        robot_exit::fail(robot, "the window's light falls far from the flat copy");
    }
}

// The share of the pixels in `area` that pass `test`.
fn share(robot: &Robot, area: Area, test: fn([u8; 3]) -> bool) -> f32 {
    let all = pixels(robot, WIDTH, area, |_| true).max(1);
    pixels(robot, WIDTH, area, test) as f32 / all as f32
}

fn is_paper([red, green, blue]: [u8; 3]) -> bool {
    red > 200 && green > 200 && blue > 180
}

// The green paper where the window's light falls on it.
fn is_window_light([red, green, blue]: [u8; 3]) -> bool {
    let (red, green, blue) = (u16::from(red), u16::from(green), u16::from(blue));
    green > 140 && green > red + 60 && green > blue + 30
}
