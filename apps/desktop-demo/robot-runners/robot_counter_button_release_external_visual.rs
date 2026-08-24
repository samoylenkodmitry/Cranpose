//! External X11 contract for the Counter App pointer highlight.
//!
//! The visible pointer highlight must brighten while the primary button is
//! pressed and return to its hover intensity after mouseup, even when the
//! pointer is released over the Increment button that also handles the click.

mod output_paths;
mod text_showcase_external_helpers;

use std::{path::PathBuf, time::Duration};

use cranpose::{AppLauncher, Robot};
use cranpose_testing::{find_button_exact_in_semantics, find_text_by_prefix_in_semantics};
use desktop_app::app;
use image::RgbaImage;
use text_showcase_external_helpers::{
    capture_x11_window, find_window_id, focus_x11_window, mouse_down_x11_button,
    mouse_up_x11_button, move_x11_mouse_in_window,
};

const WINDOW_TITLE: &str = "Robot Counter Button Release External Visual";
const WINDOW_WIDTH: u32 = 1024;
const WINDOW_HEIGHT: u32 = 768;
const ROI_HALF_WIDTH: i32 = 112;
const ROI_HALF_HEIGHT: i32 = 96;
const MIN_PRESSED_BRIGHTENING: f32 = 1.6;
const MAX_RELEASED_DRIFT: f32 = 1.1;
const MAX_RELEASED_TO_PRESSED_RATIO: f32 = 0.45;

fn main() {
    env_logger::init();
    println!("=== Robot Counter Button Release External Visual ===");

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(false)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(700));
            let _ = robot.wait_for_idle();

            let window_id = find_window_id(WINDOW_TITLE);
            focus_x11_window(&window_id);

            let (x, y, width, height) = find_button_exact_in_semantics(&robot, "Increment")
                .unwrap_or_else(|| fail(&robot, "Increment button not found"));
            let center_x = x + width * 0.5;
            let center_y = y + height * 0.5;
            move_x11_mouse_in_window(&window_id, center_x, center_y);
            std::thread::sleep(Duration::from_millis(180));
            let _ = robot.wait_for_present_frame();

            let baseline_path = diagnostic("counter_increment_hover_baseline.png");
            let baseline = capture_x11_window(&window_id, &baseline_path);

            mouse_down_x11_button("1");
            std::thread::sleep(Duration::from_millis(160));
            let _ = robot.wait_for_present_frame();
            let pressed_path = diagnostic("counter_increment_pressed.png");
            let pressed = capture_x11_window(&window_id, &pressed_path);

            mouse_up_x11_button("1");
            std::thread::sleep(Duration::from_millis(260));
            let _ = robot.wait_for_present_frame();
            let released_path = diagnostic("counter_increment_released.png");
            let released = capture_x11_window(&window_id, &released_path);

            let counter_text = wait_for_counter_text(&robot).unwrap_or_else(|| {
                fail(&robot, "Counter text missing after Increment button release")
            });
            if !counter_text.contains("1") {
                fail(
                    &robot,
                    &format!("Increment click did not update counter: {counter_text}"),
                );
            }

            let roi = Roi::around(center_x, center_y);
            let pressed_delta = mean_luma_delta(&baseline, &pressed, roi);
            let released_delta = mean_luma_delta(&baseline, &released, roi);
            let released_pressed_delta = mean_luma_delta(&pressed, &released, roi);
            let released_to_pressed_ratio = released_delta / pressed_delta.max(0.001);

            println!(
                "counter_increment_release_visual: pressed_delta={pressed_delta:.3} released_delta={released_delta:.3} released_pressed_delta={released_pressed_delta:.3} released_to_pressed_ratio={released_to_pressed_ratio:.3} baseline={} pressed={} released={}",
                baseline_path.display(),
                pressed_path.display(),
                released_path.display(),
            );

            let mut failures = Vec::new();
            if pressed_delta < MIN_PRESSED_BRIGHTENING {
                failures.push(format!(
                    "pressed highlight did not brighten enough: delta={pressed_delta:.3}"
                ));
            }
            if released_delta > MAX_RELEASED_DRIFT {
                failures.push(format!(
                    "released highlight stayed brighter than hover baseline: delta={released_delta:.3}"
                ));
            }
            if released_to_pressed_ratio > MAX_RELEASED_TO_PRESSED_RATIO {
                failures.push(format!(
                    "released highlight retained too much of the pressed state: ratio={released_to_pressed_ratio:.3}"
                ));
            }

            if !failures.is_empty() {
                fail(
                    &robot,
                    &format!(
                        "{}\nscreenshots: baseline={} pressed={} released={}",
                        failures.join("\n"),
                        baseline_path.display(),
                        pressed_path.display(),
                        released_path.display(),
                    ),
                );
            }

            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}

fn wait_for_counter_text(robot: &Robot) -> Option<String> {
    for _ in 0..20 {
        if let Some((_, _, _, _, text)) = find_text_by_prefix_in_semantics(robot, "Counter:") {
            return Some(text);
        }
        std::thread::sleep(Duration::from_millis(80));
    }
    None
}

fn diagnostic(name: &str) -> PathBuf {
    output_paths::diagnostic_path(name)
}

#[derive(Clone, Copy)]
struct Roi {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl Roi {
    fn around(x: f32, y: f32) -> Self {
        let x = x.round() as i32;
        let y = y.round() as i32;
        Self {
            x0: x - ROI_HALF_WIDTH,
            y0: y - ROI_HALF_HEIGHT,
            x1: x + ROI_HALF_WIDTH,
            y1: y + ROI_HALF_HEIGHT,
        }
    }
}

fn mean_luma_delta(left: &RgbaImage, right: &RgbaImage, roi: Roi) -> f32 {
    assert_eq!(left.dimensions(), right.dimensions());
    let width = left.width() as i32;
    let height = left.height() as i32;
    let x0 = roi.x0.clamp(0, width.saturating_sub(1));
    let y0 = roi.y0.clamp(0, height.saturating_sub(1));
    let x1 = roi.x1.clamp(x0 + 1, width);
    let y1 = roi.y1.clamp(y0 + 1, height);

    let mut total = 0.0;
    let mut count = 0usize;
    for y in y0..y1 {
        for x in x0..x1 {
            let left_luma = luma(left.get_pixel(x as u32, y as u32).0);
            let right_luma = luma(right.get_pixel(x as u32, y as u32).0);
            total += (right_luma - left_luma).abs();
            count += 1;
        }
    }

    total / count.max(1) as f32
}

fn luma(pixel: [u8; 4]) -> f32 {
    pixel[0] as f32 * 0.2126 + pixel[1] as f32 * 0.7152 + pixel[2] as f32 * 0.0722
}

fn fail(robot: &Robot, message: &str) -> ! {
    eprintln!("FAIL: {message}");
    let _ = robot.exit();
    std::process::exit(1);
}
