// The Explainer tab's pool: the wheel brings the pool into view, a picture
// at each step. A click on the writing lane focuses its field, typing
// replaces its words, and the typed words show on the floor in ink tiles.
// A stroke of the pointer and a press on bare floor ring the water round
// them far more than elsewhere. A drag over the paragraph's first words,
// under the moving water, lays their tiles in gold.

use std::{path::Path, time::Duration};

use cranpose::{Robot, RobotScreenshot};
use cranpose_ui::Modifiers;

use crate::{
    explainer_robot::{drag, pixels, save, scroll_to, text_bounds, wait},
    output_paths, robot_exit, robot_launch,
};

const HEADLINE: &str = "You can type";
const WORDS: &str = "These words lie";
const FIRST_WORDS: &str = "Write something here";
const TYPED: &str = "Hello pool";
const WIDTH: f32 = 1200.0;
const HEIGHT: f32 = 800.0;
// How many more pixels show gold once words under the water are selected.
const GOLD_PIXELS: usize = 200;
// How many ink pixels the typed words show at least.
const INK_PIXELS: usize = 150;
// How much more the water round a press changes than the water elsewhere.
const RINGS_OVER_SWELL: f32 = 1.3;
// The side of the squares the water's change is read over.
const PATCH: f32 = 140.0;
// The span the water's change is read over: short beside the swell's
// period, long beside a ripple's.
const SPAN: Duration = Duration::from_millis(60);
// The pointer moves of one stroke through the water.
const STROKE_MOVES: usize = 12;

pub(crate) fn main() {
    let _ = env_logger::try_init();
    println!("=== Robot Explainer Pool ===");
    robot_exit::arm_timeout(120);
    let shots = output_paths::diagnostic_path("cranpose-robot-explainer-pool");
    if let Err(err) = std::fs::create_dir_all(&shots) {
        robot_exit::fail_without_shutdown(&format!("{}: {err}", shots.display()));
    }
    robot_launch::launch("Robot Explainer Pool", WIDTH as u32, HEIGHT as u32)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_robot_app_hook(robot_launch::set_tab_hook)
        .with_test_driver(move |robot| {
            robot_launch::switch_tab(&robot, "explainer");
            wait(&robot, Duration::from_millis(300));
            scroll_to(&robot, &shots, HEADLINE, (WIDTH, HEIGHT));
            // The rings that greet the pool.
            wait(&robot, Duration::from_millis(700));
            save(&robot, &shots, "pool_rings.png");
            type_in_lane(&robot, &shots);
            // The press on bare floor also takes the focus from the field,
            // before the drag over the words.
            ring_the_water(&robot, &shots);
            select_words(&robot, &shots);
            println!("PASS: pool shots in {}", shots.display());
            robot.exit().ok();
        })
        .run(robot_launch::counter_demo);
}

// Clicks the lane, replaces its words by typing and checks that the field
// and the floor both show the new words.
fn type_in_lane(robot: &Robot, shots: &Path) {
    let (x, y, width, height) = text_bounds(robot, FIRST_WORDS);
    if let Err(err) = robot.click(x + width * 0.6, y + height * 0.5) {
        robot_exit::fail(robot, &format!("click the lane: {err}"));
    }
    wait(robot, Duration::from_millis(200));
    match robot.has_focused_text_field() {
        Ok(true) => {}
        Ok(false) => robot_exit::fail(
            robot,
            "a click on the lane under the water focused no field",
        ),
        Err(err) => robot_exit::fail(robot, &format!("focus query: {err}")),
    }
    if let Err(err) = robot.send_key_with_modifiers("a", Modifiers::COMMAND) {
        robot_exit::fail(robot, &format!("select all: {err}"));
    }
    wait(robot, Duration::from_millis(120));
    save(robot, shots, "pool_lane_selected.png");

    if let Err(err) = robot.type_text(TYPED) {
        robot_exit::fail(robot, &format!("type: {err}"));
    }
    wait(robot, Duration::from_millis(150));
    save(robot, shots, "pool_typed.png");
    let typed = text_bounds(robot, TYPED);
    let ink = pixels(robot, WIDTH, typed, is_ink);
    println!("  ink pixels over the typed words: {ink}");
    if ink < INK_PIXELS {
        robot_exit::fail(robot, "the typed words do not show on the floor");
    }
}

// Moves the pointer over bare floor right of the words, then presses
// there. Right after each touch the water there must change faster,
// measured against the water farther up, than it does with no touch: the
// slow swell moves both places alike, a wake or a splash runs fast through
// the touched one only.
fn ring_the_water(robot: &Robot, shots: &Path) {
    let (x, y, width, height) = text_bounds(robot, WORDS);
    let at = ((x + width + WIDTH) * 0.5, y + height * 0.5);
    // The pointer leaves the lane, too far in one move to stir the water,
    // and the lane's glow fades before anything is measured.
    if let Err(err) = robot.mouse_move(WIDTH - 4.0, at.1) {
        robot_exit::fail(robot, &format!("move off the lane: {err}"));
    }
    wait(robot, Duration::from_millis(1000));
    let swell = water_change(robot, at, || stroke(robot, at, false));
    let wake = water_change(robot, at, || stroke(robot, at, true));
    save(robot, shots, "pool_wake.png");
    wait(robot, Duration::from_millis(900));
    let calm = water_change(robot, at, || {});
    let splash = water_change(robot, at, || {
        if let Err(err) = robot.click(at.0, at.1) {
            robot_exit::fail(robot, &format!("press the floor: {err}"));
        }
        wait(robot, Duration::from_millis(150));
    });
    save(robot, shots, "pool_ring.png");
    println!(
        "  the water changed {swell:.2} times as fast as farther up by itself, \
         {wake:.2} under a moving pointer, {calm:.2} by itself again and {splash:.2} after a press"
    );
    if wake < swell * RINGS_OVER_SWELL {
        robot_exit::fail(
            robot,
            "a pointer moving over the floor left no wake in the water",
        );
    }
    if splash < calm * RINGS_OVER_SWELL {
        robot_exit::fail(robot, "a press on the floor left no rings in the water");
    }
}

// A hand's stroke back and forth across the square round `at`, one move a
// frame; without the hand, the same frames pass with the pointer at rest.
fn stroke(robot: &Robot, at: (f32, f32), hand: bool) {
    for step in 0..STROKE_MOVES {
        if hand {
            let swing = (step as f32 / (STROKE_MOVES / 2) as f32 - 1.0).abs();
            let along = at.0 + PATCH * (0.5 - swing);
            if let Err(err) = robot.mouse_move(along, at.1 + 8.0 * (step % 2) as f32) {
                robot_exit::fail(robot, &format!("move over the floor: {err}"));
            }
        }
        wait(robot, Duration::from_millis(16));
    }
}

// How much the water round `at` changes over a short span right after
// `touch`, as a share of how much the water farther up changes then.
fn water_change(robot: &Robot, at: (f32, f32), touch: impl Fn()) -> f32 {
    let above = (at.0, at.1 - 1.8 * PATCH);
    touch();
    let before = screenshot(robot);
    wait(robot, SPAN);
    let after = screenshot(robot);
    change(&before, &after, at) / change(&before, &after, above).max(0.1)
}

// Drags over the paragraph's first words from inside a letter and checks
// that their tiles turn gold: the pointer lands on the words it sees
// through the water.
fn select_words(robot: &Robot, shots: &Path) {
    let (x, y, width, height) = text_bounds(robot, WORDS);
    let first_line = (height * 0.12).min(16.0);
    let row = (x, y, width * 0.5, height * 0.4);
    let before = pixels(robot, WIDTH, row, is_gold);
    drag(robot, (x + 12.0, y + first_line), x + width * 0.4);
    wait(robot, Duration::from_millis(250));
    save(robot, shots, "pool_selected.png");
    let after = pixels(robot, WIDTH, row, is_gold);
    println!("  gold pixels over the first words: {before} before the drag, {after} after");
    if after < before + GOLD_PIXELS {
        robot_exit::fail(
            robot,
            "a drag over words under the water selected none of them",
        );
    }
}

fn screenshot(robot: &Robot) -> RobotScreenshot {
    match robot.screenshot() {
        Ok(shot) => shot,
        Err(err) => robot_exit::fail(robot, &format!("screenshot: {err}")),
    }
}

// The mean change of a channel between two pictures over a square of side
// `PATCH` round `(x, y)`.
fn change(before: &RobotScreenshot, after: &RobotScreenshot, (x, y): (f32, f32)) -> f32 {
    let half = PATCH * 0.5;
    let columns = ((x - half).max(0.0) as u32)..((x + half) as u32).min(before.width);
    let rows = ((y - half).max(0.0) as u32)..((y + half) as u32).min(before.height);
    let (mut sum, mut count) = (0u64, 0u64);
    for row in rows {
        for column in columns.clone() {
            let at = ((row * before.width + column) * 4) as usize;
            for channel in 0..3 {
                sum += u64::from(before.pixels[at + channel].abs_diff(after.pixels[at + channel]));
                count += 1;
            }
        }
    }
    sum as f32 / count.max(1) as f32
}

fn is_gold([red, green, blue]: [u8; 3]) -> bool {
    red > 190 && green > 90 && blue < 90
}

// The cobalt glass the letters are cut from.
fn is_ink([red, green, blue]: [u8; 3]) -> bool {
    red < 70 && green < 90 && blue < 140
}
