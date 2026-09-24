use std::{
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use cranpose::{Robot, RobotScreenshot};
use cranpose_testing::{find_button_in_semantics, find_in_semantics, find_text};
use image::RgbaImage;

const WATCHDOG: Duration = Duration::from_secs(90);

fn main() {
    let output = std::env::var_os("COROFLOW_ROBOT_OUT").map_or_else(
        || PathBuf::from("target/robot/coroflow-notes"),
        PathBuf::from,
    );
    if let Err(error) = std::fs::create_dir_all(&output) {
        eprintln!("cannot create {}: {error}", output.display());
        std::process::exit(1);
    }
    let launched = coroflow_demo::ui::app::create_app()
        .with_headless(true)
        .with_test_driver(move |robot| {
            thread::spawn(|| {
                thread::sleep(WATCHDOG);
                eprintln!("robot watchdog expired");
                std::process::exit(2);
            });
            let failures = drive(&robot, &output);
            let _ = robot.exit();
            if failures > 0 {
                eprintln!("{failures} check(s) failed");
                std::process::exit(1);
            }
            println!("all checks passed");
        })
        .try_run(coroflow_demo::ui::app::CoroflowDemoApp);
    if let Err(error) = launched {
        eprintln!("launch failed: {error}");
        std::process::exit(1);
    }
}

fn drive(robot: &Robot, output: &Path) -> usize {
    let mut failures = 0;
    let mut check = |ok: bool, what: &str| {
        println!("{} {what}", if ok { "PASS" } else { "FAIL" });
        failures += usize::from(!ok);
    };
    let _ = robot.wait_for_idle();
    settle(robot, 400);
    capture(robot, output, "01-initial");
    check(
        has_text(robot, "4 of 4 notes"),
        "the list starts with four notes",
    );
    check(
        has_text(robot, "Syncing, round 1"),
        "the sync starts with the screen",
    );

    click_text(robot, "Search notes and the catalog");
    let _ = robot.type_text("flo");
    settle(robot, 150);
    let _ = robot.type_text("w");
    settle(robot, 150);
    capture(robot, output, "02-typing");
    check(
        has_text(robot, "1 of 4 notes"),
        "local filtering follows each keystroke",
    );
    settle(robot, 500);
    capture(robot, output, "03-searching");
    check(
        has_text(robot, "Searching"),
        "the catalog request starts after the debounce",
    );
    settle(robot, 1_100);
    capture(robot, output, "04-results");
    check(
        has_text(robot, "StateFlow conflates"),
        "catalog results arrive",
    );

    click_text(robot, "New note");
    let _ = robot.type_text("Try coroflow in a real app");
    settle(robot, 100);
    click_button(robot, "Add");
    settle(robot, 600);
    capture(robot, output, "05-added");
    check(
        has_text(robot, "Added"),
        "adding a note shows a one-shot snackbar",
    );

    click_text(robot, "Try coroflow in a real app");
    settle(robot, 900);
    capture(robot, output, "05b-note");
    check(
        has_text(robot, "Note #4"),
        "tapping a note opens its own screen",
    );
    click_button(robot, "Back");
    settle(robot, 900);
    capture(robot, output, "05c-back-to-list");
    check(
        has_text(robot, "2 of 5 notes"),
        "going back shows the list as it was left",
    );

    click_button(robot, "Diagnostics");
    settle(robot, 500);
    capture(robot, output, "06-diagnostics-running");
    settle(robot, 5_500);
    capture(robot, output, "07-screen-state-stopped");
    settle(robot, 5_500);
    capture(robot, output, "08-sync-stopped");
    check(
        !has_exact_text(robot, "running") && has_exact_text(robot, "stopped"),
        "both upstreams stop after their timeouts",
    );

    click_button(robot, "Notes");
    settle(robot, 800);
    capture(robot, output, "09-back-to-notes");
    click_button(robot, "Diagnostics");
    settle(robot, 300);
    capture(robot, output, "10-restarted");
    check(
        has_text(robot, "started 2×") && has_exact_text(robot, "running"),
        "returning after both stopped restarts them",
    );
    failures
}

fn settle(robot: &Robot, millis: u64) {
    thread::sleep(Duration::from_millis(millis));
    let _ = robot.wait_for_idle();
}

fn has_text(robot: &Robot, text: &str) -> bool {
    find_in_semantics(robot, |element| find_text(element, text)).is_some()
}

fn has_exact_text(robot: &Robot, text: &str) -> bool {
    matches!(robot.find_text_bounds_exact(text), Ok(Some(_)))
}

fn click_at(robot: &Robot, (x, y, width, height): (f32, f32, f32, f32)) {
    let _ = robot.click(x + width / 2.0, y + height / 2.0);
    settle(robot, 100);
}

fn click_text(robot: &Robot, text: &str) {
    match find_in_semantics(robot, |element| find_text(element, text)) {
        Some(bounds) => click_at(robot, bounds),
        None => println!("FAIL could not find \"{text}\""),
    }
}

fn click_button(robot: &Robot, text: &str) {
    match find_button_in_semantics(robot, text) {
        Some(bounds) => click_at(robot, bounds),
        None => println!("FAIL could not find button \"{text}\""),
    }
}

fn capture(robot: &Robot, output: &Path, name: &str) {
    let path = output.join(format!("{name}.png"));
    let saved = robot.screenshot().and_then(|shot| save_png(&path, shot));
    match saved {
        Ok(()) => println!("saved {}", path.display()),
        Err(error) => println!("FAIL screenshot {name}: {error}"),
    }
}

fn save_png(path: &Path, shot: RobotScreenshot) -> Result<(), String> {
    RgbaImage::from_raw(shot.width, shot.height, shot.pixels)
        .ok_or_else(|| "invalid screenshot dimensions".to_string())?
        .save(path)
        .map_err(|error| error.to_string())
}
