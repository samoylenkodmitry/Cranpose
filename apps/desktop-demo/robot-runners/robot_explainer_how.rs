// The Explainer tab's "How it works": the wheel walks the section's four
// steps, a picture at each, then a drag over a line of the bent copy, the
// print, selects words that show marked on the flat copy, the board, and
// typing into the line that can be edited changes it.

use std::{path::Path, time::Duration};

use crate::{
    explainer_robot::{save, shot, text_bounds, wait, Walk, HEIGHT, WIDTH},
    output_paths, robot_exit, robot_launch,
};

use cranpose::Robot;

// The section's height, in view heights.
const HOW_PAGES: f32 = 2.6;
// Where along its walk each picture is taken: a step of the walk each.
const STOPS: [(f32, &str); 4] = [
    (0.20, "how_1_lay_out.png"),
    (0.47, "how_2_picture.png"),
    (0.66, "how_3_bend.png"),
    (0.92, "how_4_point_back.png"),
];
// How many more highlighter pixels the flat copy shows once words on the
// bent copy are selected.
const MARKED_PIXELS: usize = 200;
const TYPED: &str = " Yes";

pub(crate) fn main() {
    let _ = env_logger::try_init();
    println!("=== Robot Explainer How ===");
    robot_exit::arm_timeout(120);
    let shots = output_paths::diagnostic_path("cranpose-robot-explainer-how");
    if let Err(err) = std::fs::create_dir_all(&shots) {
        robot_exit::fail_without_shutdown(&format!("{}: {err}", shots.display()));
    }
    robot_launch::launch("Robot Explainer How", WIDTH as u32, HEIGHT as u32)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_robot_app_hook(robot_launch::set_tab_hook)
        .with_test_driver(move |robot| {
            robot_launch::switch_tab(&robot, "explainer");
            wait(&robot, Duration::from_millis(300));
            let walk = Walk::new(&robot, "how", HOW_PAGES);
            // Over the figure: the page scrolls on under a pointer that
            // rests, so the spot runs its own route at the last step.
            if let Err(err) = robot.mouse_move(WIDTH * 0.5, HEIGHT * 0.5) {
                robot_exit::fail(&robot, &format!("move the mouse: {err}"));
            }
            for (progress, name) in STOPS {
                walk.walk_to(&robot, progress);
                wait(&robot, Duration::from_millis(500));
                save(&robot, &shots, name);
            }
            select_on_bent_copy(&robot, &shots);
            type_on_flat_copy(&robot, &shots);
            println!("PASS: how shots in {}", shots.display());
            robot.exit().ok();
        })
        .run(robot_launch::counter_demo);
}

// Drags along the last line of the bent copy's middle text, the second
// line of ink above its "bent copy" label, and checks that words show
// marked on the flat copy.
fn select_on_bent_copy(robot: &Robot, shots: &Path) {
    let flat = union(text_bounds(robot, "One paragraph"), text_bounds(robot, "Both copies"));
    let label = text_bounds(robot, "bent copy");
    let unmarked = shot(robot);
    let bands = ink_bands_above(&unmarked, label);
    println!("  ink bands above the bent copy's label: {bands:?}");
    let Some(&(top, bottom)) = bands.get(1) else {
        robot_exit::fail(robot, "the bent copy shows no lines of text above its label");
    };
    let row = (top + bottom) * 0.5;
    let (from, to) = ((label.0 + 24.0, row), (label.0 + 190.0, row));
    for point in [from, to] {
        if inside(flat, point) {
            robot_exit::fail(robot, &format!("the drag point {point:?} lies on the flat copy"));
        }
    }
    let before = pixels(&unmarked, flat, is_highlighter);
    let steps = [
        robot.mouse_move(from.0, from.1),
        robot.mouse_down(),
        robot.mouse_move((from.0 + to.0) * 0.5, row),
        robot.mouse_move(to.0, to.1),
        robot.mouse_up(),
    ];
    if let Some(Err(err)) = steps.into_iter().find(Result::is_err) {
        robot_exit::fail(robot, &format!("drag: {err}"));
    }
    wait(robot, Duration::from_millis(400));
    save(robot, shots, "how_5_selected.png");
    let after = pixels(&shot(robot), flat, is_highlighter);
    println!("  highlighter pixels on the flat copy: {before} before the drag on the bent copy, {after} after");
    if after < before + MARKED_PIXELS {
        robot_exit::fail(robot, "words selected on the bent copy show no mark on the flat copy");
    }
}

// Clicks at the end of the line that can be edited, on the flat copy, and
// types; the line reads the typed words after it, and the page stays where
// it was: the bent copy's label has not moved.
fn type_on_flat_copy(robot: &Robot, shots: &Path) {
    let label = text_bounds(robot, "bent copy");
    let (x, y, width, height) = text_bounds(robot, "You can edit");
    let at = (x + width - 2.0, y + height * 0.5);
    let steps = [robot.click(at.0, at.1), robot.send_key("End")];
    if let Some(Err(err)) = steps.into_iter().find(Result::is_err) {
        robot_exit::fail(robot, &format!("focus the line: {err}"));
    }
    if let Err(err) = robot.type_text(TYPED) {
        robot_exit::fail(robot, &format!("type: {err}"));
    }
    wait(robot, Duration::from_millis(300));
    save(robot, shots, "how_6_typed.png");
    let text = match robot.find_text_by_prefix("You can edit") {
        Ok(Some((.., text))) => text,
        Ok(None) => robot_exit::fail(robot, "the line that can be edited is gone"),
        Err(err) => robot_exit::fail(robot, &format!("text query: {err}")),
    };
    println!("  the line reads {text:?}");
    if !text.ends_with(TYPED) {
        robot_exit::fail(robot, &format!("typing on the flat copy left the line as {text:?}"));
    }
    let moved = text_bounds(robot, "bent copy");
    if (moved.0 - label.0).abs() > 2.0 || (moved.1 - label.1).abs() > 2.0 {
        robot_exit::fail(
            robot,
            &format!("the click on the line moved the bent copy's label from {label:?} to {moved:?}"),
        );
    }
}

// The rows of dark ink above `label`, from the nearest up: each band's top
// and bottom. The lines of the bent copy slope, so only a narrow strip near
// the left edge of the text, where they start, is read.
fn ink_bands_above(shot: &cranpose::RobotScreenshot, label: (f32, f32, f32, f32)) -> Vec<(f32, f32)> {
    // A narrow strip where the drag starts: the copy's lines slant, and a
    // wide strip would join them into one band.
    let columns = ((label.0 + 22.0).max(0.0) as u32)..((label.0 + 30.0) as u32).min(shot.width);
    let mut bands: Vec<(f32, f32)> = Vec::new();
    let mut gap = 0;
    let bottom = (label.1.max(0.0) as u32).saturating_sub(2);
    for row in (bottom.saturating_sub(320)..bottom).rev() {
        let ink = columns
            .clone()
            .filter(|&column| is_ink(pixel(shot, column, row)))
            .count();
        let y = row as f32;
        match bands.last_mut() {
            Some(band) if ink > 2 && gap <= 2 => {
                band.0 = y;
                gap = 0;
            }
            _ if ink > 2 => {
                bands.push((y, y));
                gap = 0;
            }
            _ => gap += 1,
        }
    }
    bands.retain(|&(top, bottom)| bottom - top >= 4.0);
    bands
}

fn pixel(shot: &cranpose::RobotScreenshot, column: u32, row: u32) -> [u8; 3] {
    let at = ((row * shot.width + column) * 4) as usize;
    [shot.pixels[at], shot.pixels[at + 1], shot.pixels[at + 2]]
}

fn union(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> (f32, f32, f32, f32) {
    let (left, top) = (a.0.min(b.0), a.1.min(b.1));
    let (right, bottom) = ((a.0 + a.2).max(b.0 + b.2), (a.1 + a.3).max(b.1 + b.3));
    (left, top, right - left, bottom - top)
}

fn inside((x, y, width, height): (f32, f32, f32, f32), (px, py): (f32, f32)) -> bool {
    px >= x && px <= x + width && py >= y && py <= y + height
}

// How many pixels in `(x, y, width, height)` pass `test`.
fn pixels(
    shot: &cranpose::RobotScreenshot,
    (x, y, width, height): (f32, f32, f32, f32),
    test: fn([u8; 3]) -> bool,
) -> usize {
    let columns = (x.max(0.0) as u32)..((x + width) as u32).min(shot.width);
    let rows = (y.max(0.0) as u32)..((y + height) as u32).min(shot.height);
    rows.flat_map(|row| columns.clone().map(move |column| (column, row)))
        .filter(|&(column, row)| test(pixel(shot, column, row)))
        .count()
}

fn is_highlighter([red, green, blue]: [u8; 3]) -> bool {
    red > 200 && green > 160 && blue < 120
}

// Dark, and grey or blue rather than the green of the table behind.
fn is_ink([red, green, blue]: [u8; 3]) -> bool {
    red.max(green).max(blue) < 110 && u16::from(green) < u16::from(red) + 30
}

