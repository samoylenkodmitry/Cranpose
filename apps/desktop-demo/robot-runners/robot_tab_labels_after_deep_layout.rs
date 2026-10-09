use crate::{output_paths, robot_launch};

use std::time::Duration;

use cranpose::{AppLauncher, Robot, RobotScreenshot};
use cranpose_testing::find_button_exact_in_semantics;
use image::{ImageBuffer, Rgba};

const WINDOW_TITLE: &str = "Robot Tab Labels After Deep Layout";
const DEEP_LAYOUT_DEPTH: usize = 12;
const ROUNDS: usize = 2;
const CHANNEL_TOLERANCE: u8 = 48;
const LABEL_INK: u8 = 200;
const MIN_LABEL_INK_PIXELS: usize = 200;

pub(crate) fn main() {
    let _ = env_logger::try_init();
    println!("=== {WINDOW_TITLE} ===");

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(800, 600)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(env_bool("CRANPOSE_HEADLESS", true))
        .with_robot_app_hook(robot_launch::set_tab_hook)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(700));

            let row = button_rows(&robot, "Counter App");
            let baseline = settled_shot(&robot);
            save(&baseline, "round0_counter");
            let ink = label_ink(&baseline, row);
            assert!(
                ink >= MIN_LABEL_INK_PIXELS,
                "the tab row shows {ink} label pixels before the deep layout"
            );

            for round in 1..=ROUNDS {
                robot_launch::switch_tab(&robot, "layout");
                raise_depth(&robot, DEEP_LAYOUT_DEPTH);
                robot_launch::switch_tab(&robot, "counter");
                let shot = settled_shot(&robot);
                save(&shot, &format!("round{round}_counter"));
                let changed = changed_pixels(&baseline, &shot, row);
                println!(
                    "round {round}: tab row label pixels {} of {ink}, changed pixels {changed}",
                    label_ink(&shot, row)
                );
                assert_eq!(
                    changed, 0,
                    "round {round}: the tab row changed after leaving a depth {DEEP_LAYOUT_DEPTH} layout"
                );
            }

            println!("PASS: tab labels survived {ROUNDS} deep layout rounds");
            robot.exit().expect("exit");
        })
        .run(robot_launch::counter_demo);
}

fn button_bounds(robot: &Robot, label: &str) -> (f32, f32, f32, f32) {
    let _ = robot.wait_for_idle();
    find_button_exact_in_semantics(robot, label)
        .unwrap_or_else(|| panic!("no '{label}' button\n{}", semantics_dump(robot)))
}

fn button_rows(robot: &Robot, label: &str) -> (f32, f32) {
    let (_, y, _, height) = button_bounds(robot, label);
    (y, y + height)
}

fn raise_depth(robot: &Robot, target: usize) {
    let mut depth = current_depth(robot);
    while depth < target {
        let (x, y, width, height) = button_bounds(robot, "Increase depth");
        robot
            .click(x + width * 0.5, y + height * 0.5)
            .unwrap_or_else(|err| panic!("failed to click Increase depth: {err}"));
        let raised = current_depth(robot);
        assert_eq!(raised, depth + 1, "the depth button did not raise the depth");
        depth = raised;
    }
}

fn current_depth(robot: &Robot) -> usize {
    let _ = robot.wait_for_idle();
    let (_, _, _, _, text) = robot
        .find_text_by_prefix("Current depth:")
        .ok()
        .flatten()
        .unwrap_or_else(|| panic!("no depth label\n{}", semantics_dump(robot)));
    text.trim_start_matches("Current depth:")
        .trim()
        .parse::<usize>()
        .unwrap_or_else(|err| panic!("unreadable depth label '{text}': {err}"))
}

fn semantics_dump(robot: &Robot) -> String {
    robot.get_semantics().map_or_else(
        |err| format!("failed to fetch semantics: {err}"),
        |semantics| Robot::format_semantics(&semantics, 0),
    )
}

fn settled_shot(robot: &Robot) -> RobotScreenshot {
    let _ = robot.wait_for_idle();
    robot
        .pump_frames(3)
        .unwrap_or_else(|err| panic!("failed to pump frames: {err}"));
    robot
        .screenshot()
        .unwrap_or_else(|err| panic!("screenshot failed: {err}"))
}

fn row_pixels(shot: &RobotScreenshot, (top, bottom): (f32, f32)) -> std::ops::Range<usize> {
    let scale = shot.width as f32 / shot.logical_width;
    let first = ((top * scale).floor().max(0.0) as u32).min(shot.height);
    let last = ((bottom * scale).ceil() as u32).clamp(first, shot.height);
    (first * shot.width) as usize * 4..(last * shot.width) as usize * 4
}

fn label_ink(shot: &RobotScreenshot, row: (f32, f32)) -> usize {
    shot.pixels[row_pixels(shot, row)]
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[..3].iter().all(|&channel| channel >= LABEL_INK))
        .count()
}

fn changed_pixels(before: &RobotScreenshot, after: &RobotScreenshot, row: (f32, f32)) -> usize {
    assert_eq!(
        (before.width, before.height),
        (after.width, after.height),
        "screenshot size changed"
    );
    let range = row_pixels(before, row);
    before.pixels[range.clone()]
        .as_chunks::<4>()
        .0
        .iter()
        .zip(after.pixels[range].as_chunks::<4>().0)
        .filter(|(a, b)| {
            a[..3]
                .iter()
                .zip(&b[..3])
                .any(|(a, b)| a.abs_diff(*b) > CHANNEL_TOLERANCE)
        })
        .count()
}

fn save(shot: &RobotScreenshot, name: &str) {
    let directory = output_paths::diagnostic_path("cranpose_tab_labels_after_deep_layout");
    let path = directory.join(format!("{name}.png"));
    let saved = std::fs::create_dir_all(&directory)
        .map_err(|err| err.to_string())
        .and_then(|()| {
            ImageBuffer::<Rgba<u8>, _>::from_raw(shot.width, shot.height, shot.pixels.as_slice())
                .ok_or_else(|| "unexpected screenshot size".to_string())
        })
        .and_then(|image| image.save(&path).map_err(|err| err.to_string()));
    match saved {
        Ok(()) => println!("captured {}", path.display()),
        Err(err) => eprintln!("failed to save {}: {err}", path.display()),
    }
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key).ok().map_or(default, |value| {
        matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES")
    })
}
