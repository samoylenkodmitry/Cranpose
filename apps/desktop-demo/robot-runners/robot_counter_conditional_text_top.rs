//! Robot test for the keyed conditional text in Counter App.
//!
//! Repro:
//! 1. Start on Counter App
//! 2. Increment to 3
//! 3. Verify the odd branch text is present
//! 4. Verify it stays above the main card instead of jumping below it

mod regression_robot_support;

use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_testing::{capture_screenshot, sample_screenshot_pixel_logical};
use desktop_app::app;
use regression_robot_support::{
    click_button, semantics_dump, spawn_timeout, wait_for_text, wait_for_text_prefix,
};

fn counter_value(text: &str) -> Result<i32, String> {
    text.split(':')
        .nth(1)
        .ok_or_else(|| format!("counter text has unexpected format: {text}"))?
        .trim()
        .parse()
        .map_err(|err| format!("failed to parse counter text '{text}': {err}"))
}

fn average_luminance(
    screenshot: &cranpose::RobotScreenshot,
    bounds: (f32, f32, f32, f32),
) -> Result<f32, String> {
    let sample_center_x = bounds.0 + bounds.2 - 14.0;
    let sample_center_y = bounds.1 + bounds.3 * 0.5;
    let offsets = [-4.0, 0.0, 4.0];
    let mut total = 0.0f32;
    let mut samples = 0.0f32;

    for offset_x in offsets {
        for offset_y in offsets {
            let rgba = sample_screenshot_pixel_logical(
                screenshot,
                sample_center_x + offset_x,
                sample_center_y + offset_y,
            )
            .ok_or_else(|| {
                format!(
                    "failed to sample screenshot at ({:.1}, {:.1})",
                    sample_center_x + offset_x,
                    sample_center_y + offset_y
                )
            })?;
            total += 0.2126 * rgba[0] as f32 + 0.7152 * rgba[1] as f32 + 0.0722 * rgba[2] as f32;
            samples += 1.0;
        }
    }

    Ok(total / samples.max(1.0))
}

fn main() {
    env_logger::init();
    println!("=== Robot Counter Conditional Text Top Test ===");

    AppLauncher::new()
        .with_title("Robot Counter Conditional Text Top Test")
        .with_size(1024, 768)
        .with_headless(true)
        .with_test_driver(|robot| {
            spawn_timeout(30, "robot_counter_conditional_text_top");

            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let initial_even_bounds = wait_for_text(
                &robot,
                "if counter % 2 == 0",
                20,
                Duration::from_millis(100),
            )
            .unwrap_or_else(|err| panic!("{err}\n{}", semantics_dump(&robot)));
            let card_header_bounds = wait_for_text(
                &robot,
                "Cranpose Playground",
                1,
                Duration::from_millis(1),
            )
                .unwrap_or_else(|err| panic!("{err}\n{}", semantics_dump(&robot)));
            let initial_screenshot = capture_screenshot(&robot)
                .unwrap_or_else(|| panic!("failed to capture initial screenshot"));
            let initial_luminance = average_luminance(&initial_screenshot, initial_even_bounds)
                .unwrap_or_else(|err| panic!("{err}\n{}", semantics_dump(&robot)));

            println!(
                "Initial even text y={:.1}, header y={:.1}, luminance={:.1}",
                initial_even_bounds.1, card_header_bounds.1, initial_luminance
            );

            for expected in 1..=3 {
                click_button(&robot, "Increment")
                    .unwrap_or_else(|err| panic!("{err}\n{}", semantics_dump(&robot)));
                let (_, _, _, _, counter_text) = wait_for_text_prefix(
                    &robot,
                    "Counter:",
                    30,
                    Duration::from_millis(100),
                )
                .unwrap_or_else(|err| panic!("{err}\n{}", semantics_dump(&robot)));
                let actual = counter_value(&counter_text)
                    .unwrap_or_else(|err| panic!("{err}\n{}", semantics_dump(&robot)));
                assert_eq!(
                    actual,
                    expected,
                    "counter did not advance to {expected}\n{}",
                    semantics_dump(&robot)
                );
            }

            let odd_bounds = wait_for_text(
                &robot,
                "if counter % 2 != 0",
                30,
                Duration::from_millis(100),
            )
            .unwrap_or_else(|err| panic!("{err}\n{}", semantics_dump(&robot)));
            let header_bounds = wait_for_text(
                &robot,
                "Cranpose Playground",
                1,
                Duration::from_millis(1),
            )
                .unwrap_or_else(|err| panic!("{err}\n{}", semantics_dump(&robot)));
            let odd_screenshot = capture_screenshot(&robot)
                .unwrap_or_else(|| panic!("failed to capture odd-state screenshot"));
            let odd_luminance = average_luminance(&odd_screenshot, odd_bounds)
                .unwrap_or_else(|err| panic!("{err}\n{}", semantics_dump(&robot)));

            println!(
                "Odd text bounds=({:.1}, {:.1}, {:.1}, {:.1}), header bounds=({:.1}, {:.1}, {:.1}, {:.1}), luminance={:.1}",
                odd_bounds.0,
                odd_bounds.1,
                odd_bounds.2,
                odd_bounds.3,
                header_bounds.0,
                header_bounds.1,
                header_bounds.2,
                header_bounds.3,
                odd_luminance
            );

            assert!(
                odd_bounds.1 <= initial_even_bounds.1 + 80.0,
                "odd branch text jumped away from the top of the view: initial_y={:.1} odd_y={:.1}\n{}",
                initial_even_bounds.1,
                odd_bounds.1,
                semantics_dump(&robot)
            );
            assert!(
                odd_bounds.1 + odd_bounds.3 <= header_bounds.1 + 10.0,
                "odd branch text ended below the main counter card: odd_bottom={:.1} header_top={:.1}\n{}",
                odd_bounds.1 + odd_bounds.3,
                header_bounds.1,
                semantics_dump(&robot)
            );
            assert!(
                odd_luminance >= initial_luminance + 15.0,
                "top conditional text did not visually switch to the brighter odd-state pill: initial_luminance={:.1} odd_luminance={:.1}\n{}",
                initial_luminance,
                odd_luminance,
                semantics_dump(&robot)
            );

            println!("✓ Conditional odd text stayed at the top of Counter App");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
