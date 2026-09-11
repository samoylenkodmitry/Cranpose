mod liquid_page;
mod robot_exit;
mod robot_shot;

use std::{path::PathBuf, process::ExitCode, time::Duration};

use cranpose::{AppLauncher, Robot, RobotScreenshot};
use cranpose_testing::{find_in_semantics, find_text_exact};
use desktop_app::app::{self, LIQUID_SCROLL_VIEWPORT_TAG};

const WINDOW_WIDTH: u32 = 784;
const WINDOW_HEIGHT: u32 = 620;
const SHOT_SCALE: f32 = 2.0;
const COLLAPSED_OFFSET: f32 = 200.0;
const CARD_TITLE: &str = "iPadOS";
const CARD_SUBTITLE: &str = "Unlock the full potential of iPadOS.";
const BAR_TITLE: &str = "WWDC";
const ICON_SIZE: f32 = 68.0;
const ICON_GAP: f32 = 16.0;
const TITLE_SUBTITLE_GAP: f32 = 3.0;
const SAMPLE_WIDTH: u32 = 12;
const SAMPLE_HEIGHT: u32 = 12;
const LANDING_TOLERANCE: f32 = 3.0;
const MIN_ICON_DROP: f32 = 20.0;
const END_TOLERANCE: f32 = 12.0;

type Bounds = (f32, f32, f32, f32);

fn main() -> ExitCode {
    let _ = env_logger::try_init();
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR")
            .unwrap_or_else(|_| "target/liquid-nav-bar-backdrop".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");
    AppLauncher::new()
        .with_title("Liquid Nav Bar Backdrop")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_robot_app_hook(liquid_page::app_hook)
        .with_test_driver(move |robot| {
            robot_exit::arm_timeout(120);
            std::thread::sleep(Duration::from_millis(700));
            liquid_page::open(&robot);

            let page = find_in_semantics(&robot, |element| {
                find_text_exact(element, LIQUID_SCROLL_VIEWPORT_TAG)
            })
            .unwrap_or_else(|| robot_exit::fail(&robot, "the liquid page must be on show"));
            println!("[nav-bar] page {page:?}");
            let title = text_bounds(&robot, CARD_TITLE);
            let subtitle = text_bounds(&robot, CARD_SUBTITLE);
            let icon_y_at_top = icon_centre_y(title, subtitle);
            let icon_left = title.0 - ICON_GAP - ICON_SIZE;

            liquid_page::scroll_to(&robot, COLLAPSED_OFFSET);
            let bar = text_bounds(&robot, BAR_TITLE);
            let bar_centre_y = bar.1 + bar.3 * 0.5;

            liquid_page::scroll_to(&robot, icon_y_at_top - bar_centre_y);
            let landed = icon_centre_y(text_bounds(&robot, CARD_TITLE), subtitle);
            if (landed - bar_centre_y).abs() > LANDING_TOLERANCE {
                robot_exit::fail(
                    &robot,
                    &format!(
                        "the card's icon did not land under the bar: icon centre y {landed:.1}, bar centre y {bar_centre_y:.1}"
                    ),
                );
            }

            let shot = robot
                .screenshot_with_scale(SHOT_SCALE)
                .expect("screenshot");
            robot_shot::save(&shot, &shot_dir, "icon-under-bar.png");
            let row_top = bar_centre_y - SAMPLE_HEIGHT as f32 * 0.5;
            let luma = |x: f32| mean_luma(&shot, x, row_top);
            let over_icon = luma(icon_left + 8.0);
            let beside_icon = luma(icon_left - 14.0);
            let page_right = page.0 + page.2;
            let left_end = luma(page.0 + 2.0);
            let right_end = luma(page_right - 14.0);
            let right_inside = luma(page_right - 50.0);
            println!(
                "[nav-bar] over icon {over_icon:.1}, beside icon {beside_icon:.1}, left end {left_end:.1}, right end {right_end:.1}, right inside {right_inside:.1}"
            );

            let mut failures = Vec::new();
            if over_icon > beside_icon - MIN_ICON_DROP {
                failures.push(format!(
                    "the bar reads {over_icon:.1} over the card's dark icon but {beside_icon:.1} over the light card beside it; a dark backdrop must read darker"
                ));
            }
            for (name, end) in [("left", left_end), ("right", right_end)] {
                if (end - right_inside).abs() > END_TOLERANCE {
                    failures.push(format!(
                        "the bar's {name} end reads {end:.1} while its interior reads {right_inside:.1} over the same light page"
                    ));
                }
            }
            if !failures.is_empty() {
                robot_exit::fail(&robot, &failures.join("\n"));
            }
            println!("✓ PASS: the nav bar's backdrop follows the content beneath it");
            robot.exit().expect("exit");
        })
        .try_run(app::combined_app)
        .expect("launch nav bar backdrop runner");
    ExitCode::SUCCESS
}

fn icon_centre_y(title: Bounds, subtitle: Bounds) -> f32 {
    title.1 + (title.3 + TITLE_SUBTITLE_GAP + subtitle.3) * 0.5
}

fn text_bounds(robot: &Robot, text: &str) -> Bounds {
    let bounds = find_in_semantics(robot, |element| find_text_exact(element, text))
        .unwrap_or_else(|| robot_exit::fail(robot, &format!("text {text:?} must be on the page")));
    println!(
        "[nav-bar] {text:?} semantics {bounds:?} robot {:?}",
        robot.find_text_bounds(text)
    );
    bounds
}

fn mean_luma(shot: &RobotScreenshot, x: f32, y: f32) -> f32 {
    let sample = robot_shot::logical_sampler(shot);
    let mut total = 0.0;
    for dy in 0..SAMPLE_HEIGHT {
        for dx in 0..SAMPLE_WIDTH {
            let (r, g, b) = sample(x + dx as f32, y + dy as f32);
            total += 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
        }
    }
    total / (SAMPLE_WIDTH * SAMPLE_HEIGHT) as f32
}
