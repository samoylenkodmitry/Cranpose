//! Robot test for the Markdown tab custom scrollbar drag behavior.
//!
//! Run with:
//! `cargo run --package desktop-app --example robot_markdown_scrollbar --features robot-app`

use cranpose::AppLauncher;
use cranpose_core::CompositionLocalProvider;
use cranpose_services::{local_http_client, HttpClient, HttpClientRef, HttpFuture};
use cranpose_testing::{
    find_button_in_semantics, find_in_semantics, find_text, print_semantics_with_bounds,
};
use desktop_app::app;
use std::sync::Arc;
use std::time::Duration;

const VIEWPORT_TAG: &str = "MarkdownListViewport";
const SCROLLBAR_TAG: &str = "MarkdownScrollbarRail";
const TOP_SENTINEL: &str = "Line 001";
const DEEP_SENTINEL: &str = "Line 240";

struct MockMarkdownClient {
    body: String,
}

impl MockMarkdownClient {
    fn new() -> Self {
        let mut body = String::from("# Markdown Scrollbar Fixture\n\n");
        for i in 1..=260usize {
            body.push_str(&format!("- Line {i:03}\n"));
        }
        Self { body }
    }
}

impl HttpClient for MockMarkdownClient {
    fn get_text<'a>(&'a self, _url: &'a str) -> HttpFuture<'a, String> {
        let body = self.body.clone();
        Box::pin(async move { Ok(body) })
    }
}

fn center(bounds: (f32, f32, f32, f32)) -> (f32, f32) {
    (bounds.0 + bounds.2 * 0.5, bounds.1 + bounds.3 * 0.5)
}

fn wait_for_text_bounds(
    robot: &cranpose::Robot,
    text: &str,
    timeout_ms: u64,
) -> Option<(f32, f32, f32, f32)> {
    let attempts = (timeout_ms / 100).max(1);
    for _ in 0..attempts {
        if let Some(bounds) = find_in_semantics(robot, |elem| find_text(elem, text)) {
            return Some(bounds);
        }
        std::thread::sleep(Duration::from_millis(100));
        let _ = robot.wait_for_idle();
    }
    None
}

fn fail_and_exit(robot: &cranpose::Robot, message: &str) -> ! {
    eprintln!("FATAL: {message}");
    if let Ok(semantics) = robot.get_semantics() {
        print_semantics_with_bounds(&semantics, 0);
    }
    let _ = robot.exit();
    std::process::exit(1);
}

fn click_button(robot: &cranpose::Robot, label: &str) {
    let Some(bounds) = find_button_in_semantics(robot, label) else {
        fail_and_exit(robot, &format!("button '{label}' not found"));
    };
    let (x, y) = center(bounds);
    robot
        .click(x, y)
        .unwrap_or_else(|err| fail_and_exit(robot, &format!("click '{label}' failed: {err}")));
    std::thread::sleep(Duration::from_millis(150));
    let _ = robot.wait_for_idle();
}

fn drag_scrollbar(
    robot: &cranpose::Robot,
    rail_bounds: (f32, f32, f32, f32),
    from_frac: f32,
    to_frac: f32,
) {
    let x = rail_bounds.0 + rail_bounds.2 * 0.5;
    let y0 = rail_bounds.1 + rail_bounds.3 * from_frac;
    let y1 = rail_bounds.1 + rail_bounds.3 * to_frac;
    let steps = 30;

    let _ = robot.mouse_move(x, y0);
    let _ = robot.mouse_down();
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let y = y0 + (y1 - y0) * t;
        let _ = robot.mouse_move(x, y);
        std::thread::sleep(Duration::from_millis(12));
    }
    let _ = robot.mouse_up();
    std::thread::sleep(Duration::from_millis(120));
    let _ = robot.wait_for_idle();
}

fn main() {
    env_logger::init();
    println!("=== Markdown Scrollbar Robot Test ===");

    AppLauncher::new()
        .with_title("Markdown Scrollbar Robot Test")
        .with_size(1400, 900)
        .with_headless(false)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            click_button(&robot, "Fetch");

            if wait_for_text_bounds(&robot, TOP_SENTINEL, 10_000).is_none() {
                fail_and_exit(&robot, "markdown content did not load");
            }

            let Some(viewport_bounds) = wait_for_text_bounds(&robot, VIEWPORT_TAG, 2_000) else {
                fail_and_exit(&robot, "markdown viewport semantics not found");
            };
            let Some(rail_bounds) = wait_for_text_bounds(&robot, SCROLLBAR_TAG, 2_000) else {
                fail_and_exit(&robot, "markdown scrollbar semantics not found");
            };

            println!(
                "viewport=({:.1},{:.1},{:.1},{:.1}) rail=({:.1},{:.1},{:.1},{:.1})",
                viewport_bounds.0,
                viewport_bounds.1,
                viewport_bounds.2,
                viewport_bounds.3,
                rail_bounds.0,
                rail_bounds.1,
                rail_bounds.2,
                rail_bounds.3
            );

            let before_bottom_visible =
                find_in_semantics(&robot, |elem| find_text(elem, DEEP_SENTINEL)).is_some();
            println!("before_drag bottom_visible={before_bottom_visible}");

            drag_scrollbar(&robot, rail_bounds, 0.10, 0.90);
            let mut after_bottom_visible =
                find_in_semantics(&robot, |elem| find_text(elem, DEEP_SENTINEL)).is_some();
            if !after_bottom_visible {
                // Retry once from current position.
                drag_scrollbar(&robot, rail_bounds, 0.30, 0.95);
                after_bottom_visible =
                    find_in_semantics(&robot, |elem| find_text(elem, DEEP_SENTINEL)).is_some();
            }

            println!("after_drag bottom_visible={after_bottom_visible}");
            if !after_bottom_visible {
                fail_and_exit(
                    &robot,
                    "scrollbar drag did not move viewport to later markdown lines",
                );
            }

            println!("✓ PASS: Markdown scrollbar drag reveals deep list items");
            std::thread::sleep(Duration::from_millis(600));
            let _ = robot.exit();
        })
        .run({
            let mock_client: HttpClientRef = Arc::new(MockMarkdownClient::new());
            move || {
                let local = local_http_client();
                let client = mock_client.clone();
                CompositionLocalProvider(vec![local.provides(client)], || {
                    app::MarkdownViewerRobotApp();
                });
            }
        });
}
