//! Robot test for the Markdown tab custom scrollbar drag behavior.
//!
//! Run with:
//! `cargo run --package desktop-app --example robot_markdown_scrollbar --features robot-app`

use cranpose::fps_stats;
use cranpose::AppLauncher;
use cranpose_core::CompositionLocalProvider;
use cranpose_services::{local_http_client, HttpClient, HttpClientRef, HttpFuture};
use cranpose_testing::{
    find_button_in_semantics, find_in_semantics, find_text, print_semantics_with_bounds,
};
use desktop_app::app;
use std::fs;
use std::sync::Arc;
use std::time::Duration;

const VIEWPORT_TAG: &str = "MarkdownListViewport";
const SCROLLBAR_TAG: &str = "MarkdownScrollbarRail";
const DEFAULT_TOP_SENTINEL: &str = "Line 001";
const DEFAULT_DEEP_SENTINEL: &str = "Line 240";

struct MockMarkdownClient {
    body: String,
}

impl MockMarkdownClient {
    fn new() -> Self {
        if let Some(path) = std::env::var("CRANPOSE_MARKDOWN_FIXTURE_PATH")
            .ok()
            .filter(|value| !value.trim().is_empty())
        {
            let body = fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("failed to read fixture at {path}: {err}"));
            return Self { body };
        }

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
    wait_for_idle: bool,
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
    if wait_for_idle {
        let _ = robot.wait_for_idle();
    }
}

fn drag_viewport(
    robot: &cranpose::Robot,
    viewport_bounds: (f32, f32, f32, f32),
    from_frac: f32,
    to_frac: f32,
    wait_for_idle: bool,
) {
    let x = viewport_bounds.0 + viewport_bounds.2 * 0.5;
    let y0 = viewport_bounds.1 + viewport_bounds.3 * from_frac;
    let y1 = viewport_bounds.1 + viewport_bounds.3 * to_frac;
    let steps = 36;

    let _ = robot.mouse_move(x, y0);
    let _ = robot.mouse_down();
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let y = y0 + (y1 - y0) * t;
        let _ = robot.mouse_move(x, y);
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = robot.mouse_up();
    std::thread::sleep(Duration::from_millis(120));
    if wait_for_idle {
        let _ = robot.wait_for_idle();
    }
}

fn sentinel_bounds(robot: &cranpose::Robot, text: &str) -> Option<(f32, f32, f32, f32)> {
    if text.is_empty() {
        return None;
    }
    find_in_semantics(robot, |elem| find_text(elem, text))
}

fn main() {
    env_logger::init();
    println!("=== Markdown Scrollbar Robot Test ===");
    let headless = std::env::var("CRANPOSE_HEADLESS")
        .ok()
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false);
    let drag_loops = std::env::var("CRANPOSE_MARKDOWN_SCROLL_LOOPS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(1);
    let hold_secs = std::env::var("CRANPOSE_MARKDOWN_HOLD_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let top_sentinel = std::env::var("CRANPOSE_MARKDOWN_TOP_SENTINEL")
        .ok()
        .unwrap_or_else(|| DEFAULT_TOP_SENTINEL.to_string());
    let deep_sentinel = std::env::var("CRANPOSE_MARKDOWN_DEEP_SENTINEL")
        .ok()
        .unwrap_or_else(|| DEFAULT_DEEP_SENTINEL.to_string());
    let wait_for_idle_after_drag = std::env::var("CRANPOSE_MARKDOWN_WAIT_IDLE_AFTER_DRAG")
        .ok()
        .map(|v| !matches!(v.as_str(), "0" | "false" | "FALSE" | "no" | "NO"))
        .unwrap_or(true);
    let viewport_drag_down_loops = std::env::var("CRANPOSE_MARKDOWN_VIEWPORT_DRAG_DOWN_LOOPS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);
    let viewport_drag_up_loops = std::env::var("CRANPOSE_MARKDOWN_VIEWPORT_DRAG_UP_LOOPS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);
    let viewport_drag_from_frac = std::env::var("CRANPOSE_MARKDOWN_VIEWPORT_DRAG_FROM_FRAC")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(0.75);
    let viewport_drag_to_frac = std::env::var("CRANPOSE_MARKDOWN_VIEWPORT_DRAG_TO_FRAC")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(0.20);
    let viewport_drag_stop_on_deep = std::env::var("CRANPOSE_MARKDOWN_STOP_ON_DEEP")
        .ok()
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false);
    let return_sentinel = std::env::var("CRANPOSE_MARKDOWN_RETURN_SENTINEL")
        .ok()
        .unwrap_or_else(|| top_sentinel.clone());
    println!(
        "config: headless={headless} drag_loops={drag_loops} hold_secs={hold_secs} top_sentinel={top_sentinel:?} deep_sentinel={deep_sentinel:?} return_sentinel={return_sentinel:?} wait_idle_after_drag={wait_for_idle_after_drag} viewport_drag_down_loops={viewport_drag_down_loops} viewport_drag_up_loops={viewport_drag_up_loops} viewport_drag_from_frac={viewport_drag_from_frac:.2} viewport_drag_to_frac={viewport_drag_to_frac:.2} viewport_drag_stop_on_deep={viewport_drag_stop_on_deep}"
    );

    AppLauncher::new()
        .with_title("Markdown Scrollbar Robot Test")
        .with_size(1400, 900)
        .with_headless(headless)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            click_button(&robot, "Fetch");

            if !top_sentinel.is_empty()
                && wait_for_text_bounds(&robot, &top_sentinel, 10_000).is_none()
            {
                eprintln!(
                    "WARN: top sentinel {:?} not found within timeout; continuing",
                    top_sentinel
                );
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

            let before_bottom_visible = if deep_sentinel.is_empty() {
                false
            } else {
                find_in_semantics(&robot, |elem| find_text(elem, &deep_sentinel)).is_some()
            };
            println!("before_drag bottom_visible={before_bottom_visible}");

            let mut after_bottom_visible = false;
            for loop_idx in 0..drag_loops {
                if loop_idx % 2 == 0 {
                    drag_scrollbar(&robot, rail_bounds, 0.10, 0.90, wait_for_idle_after_drag);
                } else {
                    drag_scrollbar(&robot, rail_bounds, 0.90, 0.15, wait_for_idle_after_drag);
                }
                if !deep_sentinel.is_empty() {
                    after_bottom_visible =
                        find_in_semantics(&robot, |elem| find_text(elem, &deep_sentinel)).is_some();
                }
            }

            println!("after_drag bottom_visible={after_bottom_visible}");
            if drag_loops > 0 && !deep_sentinel.is_empty() && !after_bottom_visible {
                fail_and_exit(
                    &robot,
                    "scrollbar drag did not move viewport to later markdown lines",
                );
            }

            if viewport_drag_down_loops > 0 || viewport_drag_up_loops > 0 {
                let mut deep_bounds = sentinel_bounds(&robot, &deep_sentinel);
                let mut return_bounds = sentinel_bounds(&robot, &return_sentinel);
                println!(
                    "viewport_drag start: deep_present={} return_present={}",
                    deep_bounds.is_some(),
                    return_bounds.is_some()
                );

                let mut down_drags_executed = 0u32;
                for _ in 0..viewport_drag_down_loops {
                    drag_viewport(
                        &robot,
                        viewport_bounds,
                        viewport_drag_from_frac,
                        viewport_drag_to_frac,
                        wait_for_idle_after_drag,
                    );
                    down_drags_executed += 1;
                    if viewport_drag_stop_on_deep
                        && !deep_sentinel.is_empty()
                        && sentinel_bounds(&robot, &deep_sentinel).is_some()
                    {
                        break;
                    }
                }

                deep_bounds = sentinel_bounds(&robot, &deep_sentinel);
                return_bounds = sentinel_bounds(&robot, &return_sentinel);
                println!(
                    "after_viewport_down: drags_executed={} deep_present={} deep_y={:?} return_present={} return_y={:?}",
                    down_drags_executed,
                    deep_bounds.is_some(),
                    deep_bounds.map(|b| b.1),
                    return_bounds.is_some(),
                    return_bounds.map(|b| b.1)
                );

                for _ in 0..viewport_drag_up_loops {
                    drag_viewport(
                        &robot,
                        viewport_bounds,
                        viewport_drag_to_frac,
                        viewport_drag_from_frac,
                        wait_for_idle_after_drag,
                    );
                }

                deep_bounds = sentinel_bounds(&robot, &deep_sentinel);
                return_bounds = sentinel_bounds(&robot, &return_sentinel);
                println!(
                    "after_viewport_up: deep_present={} deep_y={:?} return_present={} return_y={:?}",
                    deep_bounds.is_some(),
                    deep_bounds.map(|b| b.1),
                    return_bounds.is_some(),
                    return_bounds.map(|b| b.1)
                );
            }

            println!("✓ PASS: Markdown scrollbar interaction completed");
            if hold_secs > 0 {
                println!("holding for {}s to collect perf samples...", hold_secs);
                std::thread::sleep(Duration::from_secs(hold_secs));
            }
            let stats = fps_stats();
            println!(
                "fps_summary: fps={:.1} frame_ms={:.2} recompositions={} recomps_per_sec={}",
                stats.fps, stats.avg_ms, stats.recompositions, stats.recomps_per_second
            );
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
