mod output_paths;
mod text_showcase_external_helpers;

use cranpose::AppLauncher;
use cranpose_testing::{
    find_button_exact_in_semantics, find_text_by_prefix_in_semantics, print_semantics_with_bounds,
};
use desktop_app::app;
use image::RgbaImage;
use std::path::PathBuf;
use std::time::Duration;
use text_showcase_external_helpers::{capture_x11_window, find_window_id};

const WINDOW_TITLE: &str = "Robot Presented Window Redraw";
const WINDOW_WIDTH: u32 = 800;
const WINDOW_HEIGHT: u32 = 600;
const MIN_CHANGED_PIXELS_AFTER_CLICK: usize = 500;

struct WindowCapture {
    image: RgbaImage,
    path: PathBuf,
}

fn capture_window(window_id: &str, tag: &str) -> WindowCapture {
    let path = capture_path(tag);
    let image = capture_x11_window(window_id, &path);
    WindowCapture { image, path }
}

fn capture_path(tag: &str) -> PathBuf {
    output_paths::diagnostic_path(&format!(
        "cranpose-presented-window-redraw-{}-{tag}.png",
        std::process::id()
    ))
}

fn changed_pixel_count(before: &RgbaImage, after: &RgbaImage, tolerance: u8) -> usize {
    assert_eq!(before.dimensions(), after.dimensions());
    before
        .pixels()
        .zip(after.pixels())
        .filter(|(lhs, rhs)| {
            lhs.0
                .iter()
                .zip(rhs.0.iter())
                .any(|(a, b)| a.abs_diff(*b) > tolerance)
        })
        .count()
}

fn remove_capture(capture: &WindowCapture) {
    let _ = std::fs::remove_file(&capture.path);
}

fn counter_value(robot: &cranpose::Robot) -> Option<i32> {
    find_text_by_prefix_in_semantics(robot, "Counter:")
        .and_then(|(_, _, _, _, text)| text.strip_prefix("Counter:")?.trim().parse().ok())
}

fn click_increment_until_state_changes(robot: &mut cranpose::Robot) {
    let initial = counter_value(robot).unwrap_or(0);
    for attempt in 1..=5 {
        let (x, y, w, h) =
            find_button_exact_in_semantics(robot, "Increment").expect("Increment button");
        let center_x = x + w * 0.5;
        let center_y = y + h * 0.5;
        println!(
            "increment_attempt={attempt} initial_counter={initial} button=({x:.1},{y:.1},{w:.1},{h:.1}) center=({center_x:.1},{center_y:.1})"
        );
        robot
            .move_to(center_x, center_y)
            .expect("move to Increment");
        robot
            .wait_for_present_frame()
            .expect("present hover frame before Increment click");
        robot.mouse_down().expect("press Increment");
        std::thread::sleep(Duration::from_millis(20));
        robot.mouse_up().expect("release Increment");

        for _ in 0..15 {
            robot.wait_for_idle().expect("idle after increment");
            if counter_value(robot).is_some_and(|value| value != initial) {
                return;
            }
            std::thread::sleep(Duration::from_millis(80));
        }

        eprintln!("Increment click attempt {attempt} did not update semantics yet");
    }

    if let Ok(semantics) = robot.get_semantics() {
        print_semantics_with_bounds(&semantics, 0);
    }
    panic!("Increment button did not update counter semantics");
}

fn wait_for_presented_change(window_id: &str, before: &WindowCapture) -> WindowCapture {
    let mut after = capture_window(window_id, "after");
    for attempt in 0..=12 {
        let changed = changed_pixel_count(&before.image, &after.image, 8);
        println!("changed_pixels_after_increment={changed} attempt={attempt}");
        if changed >= MIN_CHANGED_PIXELS_AFTER_CLICK {
            return after;
        }

        remove_capture(&after);
        std::thread::sleep(Duration::from_millis(80));
        after = capture_window(window_id, "after");
    }
    after
}

fn main() {
    env_logger::init();
    println!("=== Robot Presented Window Redraw ===");

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(false)
        .with_test_driver(|mut robot| {
            std::thread::sleep(Duration::from_millis(500));
            robot.wait_for_idle().expect("initial idle");

            let window_id = find_window_id(WINDOW_TITLE);
            let before = capture_window(&window_id, "before");

            click_increment_until_state_changes(&mut robot);

            let after = wait_for_presented_change(&window_id, &before);
            let changed = changed_pixel_count(&before.image, &after.image, 8);
            if changed < MIN_CHANGED_PIXELS_AFTER_CLICK {
                println!(
                    "FATAL: visible window did not present enough changed pixels after Increment; changed={changed} threshold={MIN_CHANGED_PIXELS_AFTER_CLICK} before={} after={}",
                    before.path.display(),
                    after.path.display()
                );
                std::process::exit(1);
            }

            remove_capture(&before);
            remove_capture(&after);
            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}
