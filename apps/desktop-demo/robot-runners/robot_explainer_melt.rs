use std::{path::Path, time::Duration};

use crate::{
    explainer_robot::{
        app_hook, check_held, copy_selection, drag, move_to, pixels, save, shot, text_bounds,
        wait, Area, Walk, HEIGHT, WIDTH,
    },
    output_paths, robot_exit, robot_launch,
};

use cranpose::Robot;

const HEADLINE: &str = "So words";
const PARAGRAPH: &str = "As you scroll, this paragraph heats up. It softens, sags and runs. \
                         Cranpose still lays out every letter, so you can select the drips. \
                         Copy them and they paste straight.";
// View heights of the melt section.
const MELT_PAGES: f32 = 2.5;
const STEPS: u32 = 6;
const DRIPPED_PIXELS: usize = 2000;
const ICING_PIXELS: usize = 400;

pub(crate) fn main() {
    let _ = env_logger::try_init();
    println!("=== Robot Explainer Melt ===");
    robot_exit::arm_timeout(120);
    let shots = output_paths::diagnostic_path("cranpose-robot-explainer-melt");
    if let Err(err) = std::fs::create_dir_all(&shots) {
        robot_exit::fail_without_shutdown(&format!("{}: {err}", shots.display()));
    }
    robot_launch::launch("Robot Explainer Melt", WIDTH as u32, HEIGHT as u32)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_robot_app_hook(app_hook)
        .with_test_driver(move |robot| {
            robot_launch::switch_tab(&robot, "explainer");
            wait(&robot, Duration::from_millis(300));
            move_to(&robot, WIDTH - 4.0, HEIGHT * 0.5);
            let walk = Walk::new(&robot, "melt", MELT_PAGES);
            walk.walk_to(&robot, 0.0);
            save(&robot, &shots, "melt_00.png");
            let set = dark_pixels(&robot);
            let held = text_bounds(&robot, HEADLINE).1;
            let paragraph = text_bounds(&robot, "As you scroll");
            let set_bands = bands(&robot, paragraph);
            for step in 1..=STEPS {
                walk.walk_to(&robot, step as f32 / STEPS as f32);
                save(&robot, &shots, &format!("melt_{step:02}.png"));
            }
            check_held(&robot, HEADLINE, held);
            let melted = dark_pixels(&robot);
            println!("  dark pixels: {set} set, {melted} melted");
            if melted < set + DRIPPED_PIXELS {
                robot_exit::fail(&robot, "the scroll melted no drips out of the words");
            }
            let melted_bands = bands(&robot, paragraph);
            println!("  paragraph lines set at {set_bands:?}, melted at {melted_bands:?}");
            let (Some(first_set), Some(first_melted)) = (set_bands.first(), melted_bands.first())
            else {
                robot_exit::fail(&robot, "the paragraph shows no lines");
            };
            if first_melted.0 < first_set.0 + 4.0 {
                robot_exit::fail(&robot, "the paragraph did not sag");
            }
            move_to(&robot, WIDTH * 0.3, HEIGHT * 0.2);
            wait(&robot, Duration::from_millis(700));
            save(&robot, &shots, "warm.png");
            mark_line(&robot, &shots, paragraph, &melted_bands);
            println!("PASS: melt shots in {}", shots.display());
            robot.exit().ok();
        })
        .run(robot_launch::counter_demo);
}

fn mark_line(robot: &Robot, shots: &Path, paragraph: Area, lines: &[(f32, f32)]) {
    let [_, line, next, ..] = lines else {
        robot_exit::fail(robot, &format!("the paragraph shows too few lines: {lines:?}"));
    };
    // Low in the line: a pointer mapped to the words without the sag would
    // land on the next line.
    let row = line.0 + (line.1 - line.0) * 0.7;
    let (x, _, width, _) = paragraph;
    drag(robot, (x + width * 0.08, row), x + width * 0.6);
    wait(robot, Duration::from_millis(900));
    save(robot, shots, "icing.png");
    let copied = copy_selection(robot);
    println!("  copied {copied:?}");
    if copied.trim().len() < 6 || !PARAGRAPH.contains(copied.trim()) {
        robot_exit::fail(robot, "the copied drips are not the paragraph's words, straight");
    }
    let band = |(top, bottom): (f32, f32)| (x, top, width, bottom - top);
    let under_line = pixels(robot, WIDTH, band(*line), is_icing);
    let under_next = pixels(robot, WIDTH, band(*next), is_icing);
    println!("  icing pixels: {under_line} under the dragged line, {under_next} under the next");
    if under_line < ICING_PIXELS || under_next * 2 > under_line {
        robot_exit::fail(robot, "a drag across a sagged line did not mark that line in icing");
    }
}

// The rows of the paragraph that hold chocolate, joined into the bands of
// its lines.
fn bands(robot: &Robot, (x, y, width, height): Area) -> Vec<(f32, f32)> {
    let shot = shot(robot);
    let columns = (x.max(0.0) as u32)..((x + width) as u32).min(shot.width);
    let rows = (y.max(0.0) as u32)..((y + height + 60.0) as u32).min(shot.height);
    let busy = |row: u32| {
        columns
            .clone()
            .filter(|&column| {
                let at = ((row * shot.width + column) * 4) as usize;
                is_chocolate([shot.pixels[at], shot.pixels[at + 1], shot.pixels[at + 2]])
            })
            .count()
            > (width * 0.08) as usize
    };
    let mut found = Vec::new();
    let mut start = None;
    for row in rows {
        match (busy(row), start) {
            (true, None) => start = Some(row),
            (false, Some(top)) => {
                if row - top >= 8 {
                    found.push((top as f32, row as f32));
                }
                start = None;
            }
            _ => {}
        }
    }
    found
}

fn dark_pixels(robot: &Robot) -> usize {
    pixels(robot, WIDTH, (0.0, 0.0, WIDTH, HEIGHT), is_chocolate)
}

fn is_chocolate([red, green, blue]: [u8; 3]) -> bool {
    red < 120 && green < 80 && blue < 70
}

fn is_icing([red, green, blue]: [u8; 3]) -> bool {
    green > 110 && u16::from(green) > u16::from(red) + 30 && blue > red
}
