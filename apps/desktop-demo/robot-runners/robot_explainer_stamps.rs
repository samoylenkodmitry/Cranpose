// The Explainer tab's demo gallery, a sheet of postage stamps: the title
// page's button scrolls to it, a stamp under the pointer curls up off the
// sheet, a drag over the sheet's title cancels the words with a red
// postmark, and a click on a stamp opens its demo tab.

use std::path::Path;

use crate::{
    explainer_robot::{
        button_bounds, changed_pixels, click, drag, move_to, pixels, save, settle, shot,
        text_bounds,
    },
    output_paths, robot_exit, robot_explainer_paint::BUTTON, robot_launch, robot_shot,
};

use cranpose::Robot;

const TITLE: &str = "Some demos in this app";
// The stamp the pointer lifts, and the stamp a click opens.
const LIFTED: &str = "Controls UI";
const OPENED: (&str, &str) = ("Counter App", "counter");
const WIDTH: f32 = 1200.0;
const HEIGHT: f32 = 800.0;
// How many pixels round a stamp change at least as it curls up.
const CURLED_PIXELS: usize = 1500;
// How many more pixels show the postmark's red once the title is
// selected.
const POSTMARK_PIXELS: usize = 300;

pub(crate) fn main() {
    let _ = env_logger::try_init();
    println!("=== Robot Explainer Stamps ===");
    robot_exit::arm_timeout(120);
    let shots = output_paths::diagnostic_path("cranpose-robot-explainer-stamps");
    if let Err(err) = std::fs::create_dir_all(&shots) {
        robot_exit::fail_without_shutdown(&format!("{}: {err}", shots.display()));
    }
    robot_launch::launch("Robot Explainer Stamps", WIDTH as u32, HEIGHT as u32)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_robot_app_hook(robot_launch::set_tab_hook)
        .with_test_driver(move |robot| {
            robot_launch::switch_tab(&robot, "explainer");
            settle(&robot);
            let (x, y, width, height) = button_bounds(&robot, BUTTON);
            click(&robot, x + width * 0.5, y + height * 0.5);
            settle(&robot);
            move_to(&robot, 4.0, HEIGHT * 0.5);
            settle(&robot);
            save(&robot, &shots, "stamps_rest.png");
            curl_stamp(&robot, &shots);
            postmark_title(&robot, &shots);
            open_stamp(&robot, &shots);
            println!("PASS: stamps shots in {}", shots.display());
            robot.exit().ok();
        })
        .run(robot_launch::counter_demo);
}

// The middle of the stamp of the tab named `name`. The tab bar shows the
// same name, so the stamp is found by the name a reader gives it.
fn stamp_of(robot: &Robot, name: &str) -> (f32, f32) {
    let (x, y, width, height) = button_bounds(robot, &format!("{name} stamp"));
    (x + width * 0.5, y + height * 0.5)
}

// Rests the pointer on a stamp: it tears free and curls up from its far
// corner, showing the ground and a shadow round it.
fn curl_stamp(robot: &Robot, shots: &Path) {
    let (x, y) = stamp_of(robot, LIFTED);
    let area = (x - 150.0, y - 110.0, 330.0, 260.0);
    let flat = shot(robot);
    move_to(robot, x, y);
    settle(robot);
    let curled = shot(robot);
    robot_shot::save(&curled, shots, "stamps_curl.png");
    let changed = changed_pixels(&flat, &curled, WIDTH, area);
    println!("  the lifted stamp changed {changed} pixels round it");
    if changed < CURLED_PIXELS {
        robot_exit::fail(robot, "a stamp under the pointer did not curl up");
    }
    move_to(robot, 4.0, HEIGHT * 0.5);
    settle(robot);
}

// Drags over the sheet's title: the selected words are cancelled with a
// red postmark.
fn postmark_title(robot: &Robot, shots: &Path) {
    let (x, y, width, height) = text_bounds(robot, TITLE);
    let area = (x, y, width, height);
    let before = pixels(robot, WIDTH, area, is_postmark);
    drag(robot, (x + 4.0, y + height * 0.5), x + width * 0.6);
    settle(robot);
    save(robot, shots, "stamps_postmark.png");
    let after = pixels(robot, WIDTH, area, is_postmark);
    println!("  postmark pixels on the title: {before} before the drag, {after} after");
    if after < before + POSTMARK_PIXELS {
        robot_exit::fail(robot, "a drag over the sheet's title left no postmark");
    }
}

// Clicks a stamp: the demo shows the stamp's tab.
fn open_stamp(robot: &Robot, shots: &Path) {
    let (name, slug) = OPENED;
    let (x, y) = stamp_of(robot, name);
    click(robot, x, y);
    settle(robot);
    save(robot, shots, "stamps_opened.png");
    let shown = robot.invoke_app_hook(robot_launch::ACTIVE_TAB, "");
    println!("  after a click on the {name} stamp the demo shows {shown:?}");
    if shown.ok().flatten().as_deref() != Some(slug) {
        robot_exit::fail(robot, "a click on a stamp did not open its tab");
    }
}

fn is_postmark([red, green, blue]: [u8; 3]) -> bool {
    red > 120 && green < 60 && blue < 60
}
