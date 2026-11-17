// The Explainer tab's title page, an oil painting of the page: the lamp
// leans after the pointer and each such frame only lights the paint, a
// drag over the small type lays a wet orange stroke over it, a drag over
// the headline paints its letters again, the button takes a fresh coat
// under the pointer, and a click on it scrolls the article to the next
// section.

use std::path::Path;

use crate::{
    explainer_robot::{
        button_bounds, changed_pixels, click, drag, move_to, pixels, render_frames, save, settle,
        shot, text_bounds,
    },
    output_paths, robot_exit, robot_launch, robot_shot,
};

use cranpose::{Robot, RobotScreenshot};

const HEADLINE: &str = "Live composables";
const SIDE: &str = "Its API follows";
pub(crate) const BUTTON: &str = "See it in action";
const WIDTH: f32 = 1200.0;
const HEIGHT: f32 = 800.0;
// How many more pixels show wet orange paint once words are selected.
const WET_PIXELS: usize = 400;
// How many pixels of the stage a lamp move must relight at least.
const RELIT_PIXELS: usize = 20_000;

pub(crate) fn main() {
    let _ = env_logger::try_init();
    println!("=== Robot Explainer Paint ===");
    robot_exit::arm_timeout(120);
    let shots = output_paths::diagnostic_path("cranpose-robot-explainer-paint");
    if let Err(err) = std::fs::create_dir_all(&shots) {
        robot_exit::fail_without_shutdown(&format!("{}: {err}", shots.display()));
    }
    robot_launch::launch("Robot Explainer Paint", WIDTH as u32, HEIGHT as u32)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_robot_app_hook(robot_launch::set_tab_hook)
        .with_test_driver(move |robot| {
            robot_launch::switch_tab(&robot, "explainer");
            settle(&robot);
            let rest = shot(&robot);
            robot_shot::save(&rest, &shots, "paint_rest.png");
            lean_lamp(&robot, &shots, &rest);
            paint_over_small_type(&robot, &shots);
            repaint_headline(&robot, &shots);
            coat_button(&robot, &shots);
            go_on(&robot, &shots);
            println!("PASS: paint shots in {}", shots.display());
            robot.exit().ok();
        })
        .run(robot_launch::counter_demo);
}

// Moves the pointer across the stage: the lamp leans after it, the light
// changes, and the frames of the move run the light alone, never the
// strokes. A robot without a window draws a frame for each screenshot, so
// each frame measured is a screenshot's.
fn lean_lamp(robot: &Robot, shots: &Path, rest: &RobotScreenshot) {
    let stage_pixels = u64::from(rest.width * rest.height);
    // The renderer keeps the painting once it has held still for a frame
    // or two.
    for _ in 0..2 {
        shot(robot);
    }
    // None of the places is over the button, which a pointer paints anew.
    // The last leans the lamp farthest from where it rests.
    for (x, y) in [(0.7, 0.3), (0.3, 0.85), (0.8, 0.2), (0.9, 0.6)] {
        move_to(robot, WIDTH * x, HEIGHT * y);
        render_frames(robot, 2);
        shot(robot);
        let stats = match robot.get_render_stats() {
            Ok(Some(stats)) => stats,
            Ok(None) => robot_exit::fail(robot, "the renderer reports no frame statistics"),
            Err(err) => robot_exit::fail(robot, &format!("render stats: {err}")),
        };
        println!(
            "  lamp frame: shader pixels {} of {stage_pixels}, isolated layers {}, effects {}",
            stats.shader_pixels, stats.isolated_layer_renders, stats.effect_applies
        );
        if stats.shader_pixels > stage_pixels * 3 / 2 || stats.isolated_layer_renders > 0 {
            robot_exit::fail(
                robot,
                "a frame in which only the lamp moved painted the strokes again",
            );
        }
    }
    settle(robot);
    let leaned = shot(robot);
    robot_shot::save(&leaned, shots, "paint_lamp.png");
    let relit = changed_pixels(rest, &leaned, WIDTH, (0.0, 0.0, WIDTH, HEIGHT));
    println!("  the lamp's lean relit {relit} pixels");
    if relit < RELIT_PIXELS {
        robot_exit::fail(robot, "the lamp did not lean after the pointer");
    }
}

// Drags over the side column's first line: a wet orange stroke comes up
// over the selected words.
fn paint_over_small_type(robot: &Robot, shots: &Path) {
    let (x, y, width, height) = text_bounds(robot, SIDE);
    let area = (x - 20.0, y - 10.0, width + 40.0, height * 0.4);
    let before = pixels(robot, WIDTH, area, is_wet_orange);
    drag(robot, (x + 2.0, y + 14.0), x + width * 0.7);
    settle(robot);
    save(robot, shots, "paint_selection.png");
    let after = pixels(robot, WIDTH, area, is_wet_orange);
    println!("  wet orange pixels over the small type: {before} before the drag, {after} after");
    if after < before + WET_PIXELS {
        robot_exit::fail(robot, "a drag over the small type laid no wet stroke over it");
    }
}

// Drags over the headline's first word: its letters are painted again
// dark over a wet orange stroke.
fn repaint_headline(robot: &Robot, shots: &Path) {
    let (x, y, width, height) = text_bounds(robot, HEADLINE);
    let area = (x, y, width * 0.5, height * 0.5);
    let before = pixels(robot, WIDTH, area, is_wet_orange);
    drag(robot, (x + 4.0, y + height * 0.25), x + width * 0.3);
    settle(robot);
    save(robot, shots, "paint_headline.png");
    let after = pixels(robot, WIDTH, area, is_wet_orange);
    println!("  wet orange pixels on the headline: {before} before the drag, {after} after");
    if after < before + WET_PIXELS {
        robot_exit::fail(robot, "a drag over the headline painted none of it again");
    }
}

// Rests the pointer on the button: the slab takes a fresh, lighter coat.
fn coat_button(robot: &Robot, shots: &Path) {
    let (x, y, width, height) = button_bounds(robot, BUTTON);
    let area = (x - 20.0, y - 10.0, width + 40.0, height + 20.0);
    move_to(robot, x - 60.0, y + height * 2.0);
    settle(robot);
    let away = shot(robot);
    move_to(robot, x + width * 0.5, y + height * 0.5);
    settle(robot);
    let over = shot(robot);
    robot_shot::save(&over, shots, "paint_button_hover.png");
    let changed = changed_pixels(&away, &over, WIDTH, area);
    println!("  the slab changed {changed} pixels under the pointer");
    if changed < 500 {
        robot_exit::fail(robot, "the button took no fresh coat under the pointer");
    }
}

// Clicks the button: the article scrolls on by a window height.
fn go_on(robot: &Robot, shots: &Path) {
    let (_, before, _, _) = text_bounds(robot, HEADLINE);
    let (x, y, width, height) = button_bounds(robot, BUTTON);
    click(robot, x + width * 0.5, y + height * 0.5);
    settle(robot);
    save(robot, shots, "paint_next.png");
    let after = robot
        .find_text_by_prefix(HEADLINE)
        .ok()
        .flatten()
        .map_or(f32::NEG_INFINITY, |(_, y, _, _, _)| y);
    println!("  the headline stood at {before:.0}, after the click at {after:.0}");
    if after > before - HEIGHT * 0.8 {
        robot_exit::fail(robot, "the button did not scroll the article on");
    }
}

fn is_wet_orange([red, green, blue]: [u8; 3]) -> bool {
    red > 190 && (50..190).contains(&green) && blue < 110
}
