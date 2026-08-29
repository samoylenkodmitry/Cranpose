mod markdown_fixture_client;
mod output_paths;
mod text_showcase_external_helpers;
mod visual_contract_metrics;

use std::{sync::Arc, time::Duration};

use cranpose::AppLauncher;
use cranpose_core::CompositionLocalProvider;
use cranpose_services::{local_http_client, HttpClientRef};
use cranpose_testing::{crop_screenshot_logical, find_button_in_semantics, find_text_in_semantics};
use desktop_app::app;
use text_showcase_external_helpers::{
    capture_x11_window_screenshot, find_window_id, focus_x11_window,
};
use visual_contract_metrics::{edge_energy_screenshot, feature_stats_screenshot};

const HN_WINDOW_TITLE: &str = "Robot HN Crispness Regression Contract";
const MARKDOWN_WINDOW_TITLE: &str = "Robot Markdown Scrollbar Regression Contract";
const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 700;
const MIN_HN_TITLE_EDGE_ENERGY: f32 = 14.0;
const MIN_HN_TITLE_WIDTH: f32 = 120.0;
const MAX_MARKDOWN_THUMB_RAIL_RATIO: f64 = 0.88;
const MIN_MARKDOWN_THUMB_PIXELS: usize = 24;

fn fail(_robot: &cranpose::Robot, message: &str) -> ! {
    println!("FATAL: {message}");
    std::process::exit(1);
}

fn wait_for_text(robot: &cranpose::Robot, text: &str) {
    for _ in 0..60 {
        if find_text_in_semantics(robot, text).is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
        let _ = robot.wait_for_idle();
    }
    fail(robot, &format!("text {text:?} did not appear"));
}

fn main() {
    env_logger::init();
    let mode = std::env::var("CRANPOSE_VISUAL_CONTRACT_MODE")
        .ok()
        .unwrap_or_else(|| "all".to_string());
    if mode == "hn" || mode == "all" {
        run_hacker_news_crispness();
    }
    if mode == "markdown" || mode == "all" {
        run_markdown_scrollbar();
    }
}

fn run_hacker_news_crispness() {
    println!("=== Hacker News Text Crispness Contract ===");
    AppLauncher::new()
        .with_title(HN_WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(false)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(900));
            let _ = robot.wait_for_idle();

            let target = "Robot HN Story 001";
            wait_for_text(&robot, target);
            std::thread::sleep(Duration::from_millis(260));
            let _ = robot.wait_for_idle();

            let bounds = find_text_in_semantics(&robot, target)
                .unwrap_or_else(|| fail(&robot, "target Hacker News title not in semantics"));
            if bounds.2 < MIN_HN_TITLE_WIDTH {
                fail(
                    &robot,
                    &format!("target title bounds too narrow for crispness metric: {bounds:?}"),
                );
            }
            let logical = robot.screenshot().expect("read logical screenshot size");
            let window_id = find_window_id(HN_WINDOW_TITLE);
            focus_x11_window(&window_id);
            let screenshot_path = output_paths::diagnostic_path("hn_crispness_presented.png");
            let screenshot = capture_x11_window_screenshot(
                &window_id,
                &screenshot_path,
                logical.logical_width,
                logical.logical_height,
            );
            let crop = crop_screenshot_logical(
                &screenshot,
                (bounds.0 - 2.0).max(0.0),
                (bounds.1 - 2.0).max(0.0),
                bounds.2 + 4.0,
                bounds.3 + 4.0,
            )
            .unwrap_or_else(|| fail(&robot, "failed to crop HN target text"));
            let edge = edge_energy_screenshot(&crop);
            println!(
                "hn_text_crispness target={target:?} bounds={bounds:?} edge_energy={edge:.3} screenshot={}",
                screenshot_path.display()
            );
            if edge < MIN_HN_TITLE_EDGE_ENERGY {
                fail(
                    &robot,
                    &format!(
                        "Hacker News title rendered blurred: edge_energy={edge:.3} threshold={MIN_HN_TITLE_EDGE_ENERGY:.3}"
                    ),
                );
            }

            println!("PASS: Hacker News target text is crisp enough");
            robot.exit().expect("exit");
        })
        .run(app::HackerNewsScrollStabilityRobotApp);
}

fn run_markdown_scrollbar() {
    println!("=== Markdown Initial Scrollbar Contract ===");
    let client: HttpClientRef =
        Arc::new(markdown_fixture_client::MarkdownFixtureClient::default_leetcode_daily());
    AppLauncher::new()
        .with_title(MARKDOWN_WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(false)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(900));
            let _ = robot.wait_for_idle();
            let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Fetch") else {
                fail(&robot, "Fetch button not found");
            };
            robot.click(x + w * 0.5, y + h * 0.5).expect("click Fetch");
            wait_for_text(&robot, "Daily leetcode archive");

            let rail_bounds = find_text_in_semantics(&robot, "MarkdownScrollbarRail")
                .unwrap_or_else(|| fail(&robot, "MarkdownScrollbarRail semantics missing"));
            let screenshot = robot.screenshot().expect("capture markdown screenshot");
            let thumb_stats = feature_stats_screenshot(&screenshot, rail_bounds, is_markdown_thumb_pixel)
                .unwrap_or_else(|| fail(&robot, "Markdown scrollbar thumb pixels not found"));
            let thumb_height = (thumb_stats.max_y - thumb_stats.min_y + 1) as f64;
            let scale_y = screenshot.height as f64 / screenshot.logical_height.max(1.0) as f64;
            let rail_height = rail_bounds.3 as f64 * scale_y;
            let ratio = thumb_height / rail_height.max(1.0);
            println!(
                "markdown_scrollbar_initial rail={rail_bounds:?} thumb={thumb_stats:?} ratio={ratio:.3}"
            );
            if thumb_stats.count < MIN_MARKDOWN_THUMB_PIXELS || ratio > MAX_MARKDOWN_THUMB_RAIL_RATIO {
                fail(
                    &robot,
                    &format!(
                        "Markdown scrollbar starts visually filled before touch: thumb_height={thumb_height:.1} rail_height={rail_height:.1} ratio={ratio:.3}"
                    ),
                );
            }

            println!("PASS: Markdown initial scrollbar thumb is proportional");
            robot.exit().expect("exit");
        })
        .run({
            move || {
                let local = local_http_client();
                let client = client.clone();
                CompositionLocalProvider(vec![local.provides(client)], || {
                    app::MarkdownViewerRobotApp();
                });
            }
        });
}

fn is_markdown_thumb_pixel(pixel: [u8; 4]) -> bool {
    let [red, green, blue, alpha] = pixel;
    alpha > 180 && blue > 150 && green > 120 && red > 80 && blue >= red.saturating_add(20)
}
