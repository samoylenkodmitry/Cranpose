//! Robot test for Hacker News tab lazy list scroll behavior.
//!
//! Validates:
//! 1. The Hacker News list is constrained to the viewport (no infinite parent height).
//! 2. The list can scroll far enough to reveal later items.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_hacker_news_scroll --features robot-app
//! ```

use cranpose::AppLauncher;
use cranpose_core::CompositionLocalProvider;
use cranpose_testing::{
    find_button_in_semantics, find_element_by_text_exact, find_in_semantics, find_text_exact,
    print_semantics_with_bounds,
};
use cranpose_ui::http::HttpFuture;
use cranpose_ui::{local_http_client, HttpClient, HttpClientRef, HttpError};
use desktop_app::app;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

const MOCK_STORY_COUNT: usize = 60;

struct MockHackerNewsClient {
    ids: Vec<u64>,
}

impl MockHackerNewsClient {
    fn new() -> Self {
        Self {
            ids: (0..MOCK_STORY_COUNT)
                .map(|index| 1_000_000 + index as u64)
                .collect(),
        }
    }

    fn topstories_json(&self) -> String {
        json!(self.ids).to_string()
    }

    fn story_json(&self, id: u64) -> String {
        let index = self
            .ids
            .iter()
            .position(|candidate| *candidate == id)
            .unwrap_or(0);
        json!({
            "id": id,
            "title": format!("Mock Story #{}", index + 1),
            "by": "robot",
            "score": 100 + index as i32,
            "time": 1_700_000_000 + index as i64 * 60,
            "url": format!("https://example.com/story/{}", id),
            "descendants": index as i32,
            "kids": [],
            "type": "story"
        })
        .to_string()
    }

    fn parse_story_id(url: &str) -> Option<u64> {
        let suffix = url.split("/item/").nth(1)?;
        let id_str = suffix.strip_suffix(".json")?;
        id_str.parse::<u64>().ok()
    }
}

impl HttpClient for MockHackerNewsClient {
    fn get_text<'a>(&'a self, url: &'a str) -> HttpFuture<'a, String> {
        let response = if url.ends_with("/topstories.json") {
            Ok(self.topstories_json())
        } else if let Some(id) = Self::parse_story_id(url) {
            Ok(self.story_json(id))
        } else {
            Err(HttpError::RequestFailed {
                url: url.to_string(),
                message: "Unknown mock endpoint".to_string(),
            })
        };

        Box::pin(async move { response })
    }
}

fn main() {
    env_logger::init();
    println!("=== Hacker News Scroll Robot Test ===");

    AppLauncher::new()
        .with_title("Hacker News Scroll Test")
        .with_size(1200, 800)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let click_button = |name: &str| -> bool {
                if let Some((x, y, w, h)) = find_button_in_semantics(&robot, name) {
                    robot.click(x + w / 2.0, y + h / 2.0).ok();
                    std::thread::sleep(Duration::from_millis(200));
                    return true;
                }
                println!("  ✗ Button '{}' not found!", name);
                false
            };

            let wait_for_text = |text: &str| -> bool {
                for _ in 0..40 {
                    if find_in_semantics(&robot, |elem| find_text_exact(elem, text)).is_some() {
                        return true;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                false
            };

            // Navigate to Hacker News tab.
            if !click_button("Hacker News") {
                println!("FATAL: Could not find 'Hacker News' tab button");
                robot.exit().ok();
                std::process::exit(1);
            }

            // Wait for mocked stories to appear.
            if !wait_for_text("Mock Story #1") {
                println!("FATAL: Mock stories did not appear");
                if let Ok(elements) = robot.get_semantics() {
                    print_semantics_with_bounds(&elements, 0);
                }
                robot.exit().ok();
                std::process::exit(1);
            }

            // Ensure list viewport is constrained.
            let semantics = robot.get_semantics().ok();
            let list_bounds = semantics
                .as_deref()
                .and_then(|elements| find_element_by_text_exact(elements, "HackerNewsList"))
                .map(|elem| {
                    (
                        elem.bounds.x,
                        elem.bounds.y,
                        elem.bounds.width,
                        elem.bounds.height,
                    )
                });

            let (list_x, list_y, list_w, list_h) = if let Some(bounds) = list_bounds {
                bounds
            } else {
                println!("  ✗ FAIL: HackerNewsList semantics not found");
                if let Some(elements) = semantics.as_deref() {
                    print_semantics_with_bounds(elements, 0);
                }
                robot.exit().ok();
                std::process::exit(1);
            };

            {
                let (x, y, w, h) = (list_x, list_y, list_w, list_h);
                println!(
                    "  ✓ HackerNewsList bounds=({:.1},{:.1},{:.1},{:.1})",
                    x, y, w, h
                );
                if h > 780.0 {
                    println!(
                        "  ✗ FAIL: HackerNewsList height {:.1} exceeds viewport expectations",
                        h
                    );
                    if let Some(elements) = semantics.as_deref() {
                        print_semantics_with_bounds(elements, 0);
                    }
                    robot.exit().ok();
                    std::process::exit(1);
                }
            }

            // Scroll to reveal later stories.
            let start_x = list_x + list_w / 2.0;
            let start_y = list_y + list_h * 0.75;
            let end_y = list_y + list_h * 0.25;

            for _ in 0..3 {
                robot.drag(start_x, start_y, start_x, end_y).ok();
                std::thread::sleep(Duration::from_millis(250));
                let _ = robot.wait_for_idle();
            }

            let story1_visible =
                find_in_semantics(&robot, |elem| find_text_exact(elem, "Mock Story #1")).is_some();
            let story12_visible =
                find_in_semantics(&robot, |elem| find_text_exact(elem, "Mock Story #12")).is_some();

            if story1_visible && !story12_visible {
                println!("  ✗ FAIL: Scroll did not reveal later stories");
                if let Ok(elements) = robot.get_semantics() {
                    print_semantics_with_bounds(&elements, 0);
                }
                robot.exit().ok();
                std::process::exit(1);
            }

            println!("  ✓ Scroll revealed later stories");
            let _ = robot.exit();
        })
        .run({
            let mock_client: HttpClientRef = Arc::new(MockHackerNewsClient::new());
            move || {
                let local = local_http_client();
                let client = mock_client.clone();
                CompositionLocalProvider(vec![local.provides(client)], || {
                    app::combined_app();
                });
            }
        });
}
