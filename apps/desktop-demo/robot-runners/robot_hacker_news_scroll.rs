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
use cranpose_services::{local_http_client, HttpClient, HttpClientRef, HttpError, HttpFuture};
use cranpose_testing::{
    find_button, find_button_in_semantics, find_element_by_text_exact, find_in_semantics,
    find_text_exact, print_semantics_with_bounds, root_bounds,
};
use desktop_app::app;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

const MOCK_STORY_COUNT: usize = 60;
const TARGET_STORY_ID: u64 = 1_000_011;

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
        let comment_root_a = id * 10 + 1;
        let comment_root_b = id * 10 + 2;
        json!({
            "id": id,
            "title": format!("Mock Story #{}", index + 1),
            "by": "robot",
            "score": 100 + index as i32,
            "time": 1_700_000_000 + index as i64 * 60,
            "url": format!("https://example.com/story/{}", id),
            "descendants": 3,
            "kids": [comment_root_a, comment_root_b],
            "type": "story"
        })
        .to_string()
    }

    fn comment_json(&self, id: u64) -> Option<String> {
        let story_id = id / 10;
        let suffix = id % 10;
        if !self.ids.contains(&story_id) {
            return None;
        }

        let payload = match suffix {
            1 => json!({
                "id": id,
                "by": "root-a",
                "text": "Mock root comment A",
                "kids": [story_id * 10 + 3],
                "type": "comment"
            }),
            2 => json!({
                "id": id,
                "by": "root-b",
                "text": "Mock root comment B",
                "kids": [],
                "type": "comment"
            }),
            3 => json!({
                "id": id,
                "by": "child-a1",
                "text": "Mock child comment",
                "kids": [],
                "type": "comment"
            }),
            _ => return None,
        };

        Some(payload.to_string())
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
            if let Some(payload) = self.comment_json(id) {
                Ok(payload)
            } else if self.ids.contains(&id) {
                Ok(self.story_json(id))
            } else {
                Err(HttpError::RequestFailed {
                    url: url.to_string(),
                    message: "Unknown mock item".to_string(),
                })
            }
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

            let click_story_comments_button = |story_label: &str| -> bool {
                let Ok(elements) = robot.get_semantics() else {
                    return false;
                };
                let Some(story_elem) = find_element_by_text_exact(&elements, story_label) else {
                    println!("  ✗ Story semantics '{}' not found!", story_label);
                    return false;
                };
                if let Some((x, y, w, h)) = find_button(story_elem, "View 3 comments") {
                    robot.click(x + w / 2.0, y + h / 2.0).ok();
                    std::thread::sleep(Duration::from_millis(200));
                    return true;
                }
                println!(
                    "  ✗ Story '{}' did not expose its comments button!",
                    story_label
                );
                false
            };

            let wait_for_text = |text: &str| -> bool {
                for _ in 0..60 {
                    if find_in_semantics(&robot, |elem| find_text_exact(elem, text)).is_some() {
                        return true;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                false
            };

            let semantics_bounds = |label: &str| -> Option<(f32, f32, f32, f32)> {
                let elements = robot.get_semantics().ok()?;
                find_element_by_text_exact(&elements, label).map(|elem| {
                    (
                        elem.bounds.x,
                        elem.bounds.y,
                        elem.bounds.width,
                        elem.bounds.height,
                    )
                })
            };

            let assert_within_root = |name: &str, bounds: (f32, f32, f32, f32)| {
                let Some((root_x, root_y, root_w, root_h)) = root_bounds(&robot) else {
                    println!("  ✗ FAIL: missing root bounds");
                    robot.exit().ok();
                    std::process::exit(1);
                };
                let (x, y, w, h) = bounds;
                let root_right = root_x + root_w;
                let root_bottom = root_y + root_h;
                let right = x + w;
                let bottom = y + h;
                if x < root_x || y < root_y || right > root_right || bottom > root_bottom {
                    println!(
                        "  ✗ FAIL: {name} bounds=({x:.1},{y:.1},{w:.1},{h:.1}) exceed root=({root_x:.1},{root_y:.1},{root_w:.1},{root_h:.1})"
                    );
                    if let Ok(elements) = robot.get_semantics() {
                        print_semantics_with_bounds(&elements, 0);
                    }
                    robot.exit().ok();
                    std::process::exit(1);
                }
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
            let (list_x, list_y, list_w, list_h) =
                if let Some(bounds) = semantics_bounds("HackerNewsList") {
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

            let list_rail_bounds = if let Some(bounds) = semantics_bounds("HackerNewsListScrollbarRail")
            {
                bounds
            } else {
                println!("  ✗ FAIL: HackerNewsListScrollbarRail semantics not found");
                if let Some(elements) = semantics.as_deref() {
                    print_semantics_with_bounds(elements, 0);
                }
                robot.exit().ok();
                std::process::exit(1);
            };

            assert_within_root("HackerNewsList", (list_x, list_y, list_w, list_h));
            assert_within_root("HackerNewsListScrollbarRail", list_rail_bounds);

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
            let _ = robot.wait_for_idle();
            if !click_story_comments_button(&format!("HackerNewsStory {}", TARGET_STORY_ID)) {
                println!("FATAL: Could not select the target story");
                if let Ok(elements) = robot.get_semantics() {
                    print_semantics_with_bounds(&elements, 0);
                }
                robot.exit().ok();
                std::process::exit(1);
            }

            if !wait_for_text("Mock root comment A") {
                println!("FATAL: Mock comments did not appear");
                if let Ok(elements) = robot.get_semantics() {
                    print_semantics_with_bounds(&elements, 0);
                }
                robot.exit().ok();
                std::process::exit(1);
            }

            let comments_list_bounds =
                if let Some(bounds) = semantics_bounds("HackerNewsCommentsList") {
                    bounds
                } else {
                    println!("  ✗ FAIL: HackerNewsCommentsList semantics not found");
                    if let Ok(elements) = robot.get_semantics() {
                        print_semantics_with_bounds(&elements, 0);
                    }
                    robot.exit().ok();
                    std::process::exit(1);
                };
            let comments_rail_bounds =
                if let Some(bounds) = semantics_bounds("HackerNewsCommentsScrollbarRail") {
                    bounds
                } else {
                    println!("  ✗ FAIL: HackerNewsCommentsScrollbarRail semantics not found");
                    if let Ok(elements) = robot.get_semantics() {
                        print_semantics_with_bounds(&elements, 0);
                    }
                    robot.exit().ok();
                    std::process::exit(1);
                };

            assert_within_root("HackerNewsCommentsList", comments_list_bounds);
            assert_within_root("HackerNewsCommentsScrollbarRail", comments_rail_bounds);
            println!("  ✓ Comments pane bounds stay within the window");
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
