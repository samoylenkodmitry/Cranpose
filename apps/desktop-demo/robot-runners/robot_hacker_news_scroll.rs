//! Robot test for Hacker News tab single-pane scroll behavior.
//!
//! Validates:
//! 1. The Hacker News list is constrained to the viewport (no infinite parent height).
//! 2. The thread pane scrolls after opening the first story.
//! 3. Returning via Back restores a scrollable story list instead of reopening the thread.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_hacker_news_scroll --features robot-app
//! ```

use cranpose::AppLauncher;
use cranpose::SemanticElement;
use cranpose_core::CompositionLocalProvider;
use cranpose_services::{local_http_client, HttpClient, HttpClientRef, HttpError, HttpFuture};
use cranpose_testing::{
    find_button, find_button_in_semantics, find_element_by_text_exact, find_in_semantics,
    find_text_exact, print_semantics_with_bounds, root_bounds,
};
use desktop_app::app::{self, DemoTab};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

const MOCK_STORY_COUNT: usize = 60;
const MOCK_COMMENT_COUNT: usize = 40;

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
        let comment_ids = (1..=MOCK_COMMENT_COUNT)
            .map(|suffix| id * 100 + suffix as u64)
            .collect::<Vec<_>>();
        json!({
            "id": id,
            "title": format!("Mock Story #{}", index + 1),
            "text": format!(
                "<p>{}</p><p>{}</p><p>{}</p>",
                "This is a deliberately tall mock story body used to verify that the thread pane remains scrollable after the first comment load.".repeat(2),
                "The robot test needs enough pre-comment content to catch unbounded first-pass list measurement.".repeat(2),
                "If scrolling is broken, later comments will never become reachable until the pane is recreated.".repeat(2),
            ),
            "by": "robot",
            "score": 100 + index as i32,
            "time": 1_700_000_000 + index as i64 * 60,
            "url": format!("https://example.com/story/{}", id),
            "descendants": MOCK_COMMENT_COUNT,
            "kids": comment_ids,
            "type": "story"
        })
        .to_string()
    }

    fn comment_json(&self, id: u64) -> Option<String> {
        let story_id = id / 100;
        let suffix = id % 100;
        if !self.ids.contains(&story_id) {
            return None;
        }

        if suffix == 0 || suffix > MOCK_COMMENT_COUNT as u64 {
            return None;
        }

        Some(
            json!({
                "id": id,
                "by": format!("commenter-{suffix}"),
                "text": format!(
                    "Mock comment #{suffix}. {}",
                    "This body is long enough to produce a realistic multi-line row in the thread pane.".repeat((suffix as usize % 3) + 1)
                ),
                "kids": [],
                "type": "comment"
            })
            .to_string(),
        )
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

fn collect_story_elements<'a>(elem: &'a SemanticElement, stories: &mut Vec<&'a SemanticElement>) {
    if elem
        .text
        .as_deref()
        .is_some_and(|text| text.starts_with("HackerNewsStory "))
    {
        stories.push(elem);
    }
    for child in &elem.children {
        collect_story_elements(child, stories);
    }
}

fn find_mock_story_number(elem: &SemanticElement) -> Option<usize> {
    if let Some(text) = elem.text.as_deref() {
        if let Some(number) = text.strip_prefix("Mock Story #") {
            if let Ok(number) = number.parse::<usize>() {
                return Some(number);
            }
        }
    }
    for child in &elem.children {
        if let Some(number) = find_mock_story_number(child) {
            return Some(number);
        }
    }
    None
}

fn main() {
    env_logger::init();
    println!("=== Hacker News Single Pane Scroll Robot Test ===");

    AppLauncher::new()
        .with_title("Hacker News Single Pane Scroll Test")
        .with_size(390, 844)
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

            let click_visible_story_comments_button =
                |list_bounds: (f32, f32, f32, f32)| -> bool {
                let Ok(elements) = robot.get_semantics() else {
                    return false;
                };

                let (list_x, list_y, list_w, list_h) = list_bounds;
                let list_right = list_x + list_w;
                let list_bottom = list_y + list_h;
                let mut stories = Vec::new();
                for root in &elements {
                    collect_story_elements(root, &mut stories);
                }

                stories.sort_by(|left, right| {
                    left.bounds
                        .y
                        .partial_cmp(&right.bounds.y)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });

                for story_elem in stories {
                    let Some((x, y, w, h)) =
                        find_button(story_elem, &format!("View {MOCK_COMMENT_COUNT} comments"))
                    else {
                        continue;
                    };
                    let center_x = x + w / 2.0;
                    let center_y = y + h / 2.0;
                    if center_x < list_x
                        || center_x > list_right
                        || center_y < list_y
                        || center_y > list_bottom
                    {
                        continue;
                    }
                    println!(
                        "  ✓ Clicking visible comments button in {}",
                        story_elem.text.as_deref().unwrap_or("[unknown story]")
                    );
                    robot.click(center_x, center_y).ok();
                    std::thread::sleep(Duration::from_millis(200));
                    return true;
                }

                println!("  ✗ No visible story comments button found inside list viewport");
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

            let wait_for_no_text = |text: &str| -> bool {
                for _ in 0..60 {
                    if find_in_semantics(&robot, |elem| find_text_exact(elem, text)).is_none() {
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

            let visible_mock_story_numbers = |list_bounds: (f32, f32, f32, f32)| -> Vec<usize> {
                let Ok(elements) = robot.get_semantics() else {
                    return Vec::new();
                };
                let (list_x, list_y, list_w, list_h) = list_bounds;
                let list_right = list_x + list_w;
                let list_bottom = list_y + list_h;
                let mut numbers = Vec::new();
                let mut stories = Vec::new();
                for root in &elements {
                    collect_story_elements(root, &mut stories);
                }
                for story in stories {
                    let story_right = story.bounds.x + story.bounds.width;
                    let story_bottom = story.bounds.y + story.bounds.height;
                    let intersects_viewport = story_bottom > list_y
                        && story.bounds.y < list_bottom
                        && story_right > list_x
                        && story.bounds.x < list_right;
                    if !intersects_viewport {
                        continue;
                    }
                    if let Some(number) = find_mock_story_number(story) {
                        numbers.push(number);
                    }
                }
                numbers.sort_unstable();
                numbers.dedup();
                numbers
            };

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
                if h > 760.0 {
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

            let initial_story_numbers = visible_mock_story_numbers((list_x, list_y, list_w, list_h));
            println!("  ✓ Initial visible stories: {initial_story_numbers:?}");

            if !click_visible_story_comments_button((list_x, list_y, list_w, list_h)) {
                println!("FATAL: Could not select the target story");
                if let Ok(elements) = robot.get_semantics() {
                    print_semantics_with_bounds(&elements, 0);
                }
                robot.exit().ok();
                std::process::exit(1);
            }

            if !wait_for_text("commenter-1") {
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
            let (comments_x, comments_y, comments_w, comments_h) = comments_list_bounds;
            let scroll_start_x = comments_x + comments_w * 0.5;
            let scroll_start_y = comments_y + comments_h * 0.75;
            let scroll_end_y = comments_y + comments_h * 0.25;
            let comment_intersects_viewport = |label: &str| {
                semantics_bounds(label).is_some_and(|(x, y, w, h)| {
                    let right = x + w;
                    let bottom = y + h;
                    bottom > comments_y
                        && y < comments_y + comments_h
                        && right > comments_x
                        && x < comments_x + comments_w
                })
            };
            let comment1_before_y = semantics_bounds("commenter-1")
                .map(|(_, y, _, _)| y)
                .unwrap_or(comments_y);

            for _ in 0..4 {
                robot
                    .drag(scroll_start_x, scroll_start_y, scroll_start_x, scroll_end_y)
                    .ok();
                std::thread::sleep(Duration::from_millis(250));
                let _ = robot.wait_for_idle();
            }

            let comment1_after_y = semantics_bounds("commenter-1")
                .map(|(_, y, _, _)| y)
                .unwrap_or(comments_y - 100.0);
            let first_comment_still_in_view = comment_intersects_viewport("commenter-1");
            let later_comment_visible = comment_intersects_viewport("commenter-8")
                || comment_intersects_viewport("commenter-12")
                || comment_intersects_viewport("commenter-18");
            let comment1_moved = comment1_after_y < comment1_before_y - 80.0;

            if first_comment_still_in_view && !later_comment_visible && !comment1_moved {
                println!("  ✗ FAIL: Comments pane did not scroll after first load");
                if let Ok(elements) = robot.get_semantics() {
                    print_semantics_with_bounds(&elements, 0);
                }
                robot.exit().ok();
                std::process::exit(1);
            }

            println!("  ✓ Comments pane scrolls on the first load");
            println!("  ✓ Comments pane bounds stay within the window");

            if !click_button("Back") {
                println!("FATAL: Could not find Back button after opening comments");
                if let Ok(elements) = robot.get_semantics() {
                    print_semantics_with_bounds(&elements, 0);
                }
                robot.exit().ok();
                std::process::exit(1);
            }

            if !wait_for_no_text("Back") {
                println!("FATAL: Back button stayed visible after returning to the story list");
                if let Ok(elements) = robot.get_semantics() {
                    print_semantics_with_bounds(&elements, 0);
                }
                robot.exit().ok();
                std::process::exit(1);
            }

            if !wait_for_text("Mock Story #1") {
                println!("FATAL: Story list did not return after Back");
                if let Ok(elements) = robot.get_semantics() {
                    print_semantics_with_bounds(&elements, 0);
                }
                robot.exit().ok();
                std::process::exit(1);
            }

            let restored_story_numbers =
                visible_mock_story_numbers((list_x, list_y, list_w, list_h));
            println!("  ✓ Restored visible stories: {restored_story_numbers:?}");
            let max_story_before_drag = restored_story_numbers.iter().copied().max().unwrap_or(0);
            let story1_before_drag_y = semantics_bounds("HackerNewsStory 1000000")
                .map(|(_, y, _, _)| y)
                .unwrap_or(list_y);
            let drag_start_x = list_x + list_w * 0.5;
            let drag_start_y = list_y + list_h * 0.80;
            let drag_end_y = list_y + list_h * 0.25;
            let mut scrolled_story_numbers = restored_story_numbers.clone();
            let mut story1_after_drag_y = story1_before_drag_y;
            let mut list_moved = false;

            for drag_index in 0..6 {
                robot
                    .drag(drag_start_x, drag_start_y, drag_start_x, drag_end_y)
                    .ok();
                std::thread::sleep(Duration::from_millis(250));
                let _ = robot.wait_for_idle();

                if find_in_semantics(&robot, |elem| find_text_exact(elem, "Back")).is_some()
                    || find_in_semantics(&robot, |elem| find_text_exact(elem, "commenter-1"))
                        .is_some()
                {
                    println!(
                        "  ✗ FAIL: Drag on restored story list reopened the discussion on drag #{}",
                        drag_index + 1
                    );
                    if let Ok(elements) = robot.get_semantics() {
                        print_semantics_with_bounds(&elements, 0);
                    }
                    robot.exit().ok();
                    std::process::exit(1);
                }

                scrolled_story_numbers =
                    visible_mock_story_numbers((list_x, list_y, list_w, list_h));
                let max_story_after_drag =
                    scrolled_story_numbers.iter().copied().max().unwrap_or(0);
                story1_after_drag_y = semantics_bounds("HackerNewsStory 1000000")
                    .map(|(_, y, _, _)| y)
                    .unwrap_or(list_y - 100.0);
                println!(
                    "  • After restored-list drag #{} visible stories: {:?} story1_y={:.1}",
                    drag_index + 1,
                    scrolled_story_numbers,
                    story1_after_drag_y
                );
                list_moved = max_story_after_drag > max_story_before_drag
                    || story1_after_drag_y < story1_before_drag_y - 40.0;
                if list_moved {
                    break;
                }
            }

            if !list_moved {
                println!(
                    "  ✗ FAIL: Restored story list did not move after drag; before_max={} after={:?} story1_before_y={:.1} story1_after_y={:.1}",
                    max_story_before_drag,
                    scrolled_story_numbers,
                    story1_before_drag_y,
                    story1_after_drag_y
                );
                if let Ok(elements) = robot.get_semantics() {
                    print_semantics_with_bounds(&elements, 0);
                }
                robot.exit().ok();
                std::process::exit(1);
            }

            println!("  ✓ Restored story list scrolls after Back");

            let scroll_probe_x = list_x + list_w * 0.5;
            let scroll_probe_y = list_y + list_h * 0.5;
            let story1_before_wheel_y =
                semantics_bounds("HackerNewsStory 1000000").map(|(_, y, _, _)| y);
            let mut story1_after_wheel_y = story1_before_wheel_y;
            let mut wheel_moves = 0usize;
            let mut stalled_scrolls = 0usize;
            let visible_after_drag = visible_mock_story_numbers((list_x, list_y, list_w, list_h));
            let mut previous_min_story = visible_after_drag.iter().copied().min().unwrap_or(0);
            let mut previous_max_story = visible_after_drag
                .iter()
                .copied()
                .max()
                .unwrap_or(max_story_before_drag);
            let mut max_story_after_wheel = previous_max_story;
            for wheel_idx in 0..12 {
                robot.mouse_move(scroll_probe_x, scroll_probe_y).ok();
                robot.mouse_scroll(0.0, -120.0).ok();
                std::thread::sleep(Duration::from_millis(200));
                let _ = robot.wait_for_idle();

                if find_in_semantics(&robot, |elem| find_text_exact(elem, "Back")).is_some()
                    || find_in_semantics(&robot, |elem| find_text_exact(elem, "commenter-1"))
                        .is_some()
                {
                    println!(
                        "  ✗ FAIL: Mouse wheel reopened the discussion on scroll #{}",
                        wheel_idx + 1
                    );
                    if let Ok(elements) = robot.get_semantics() {
                        print_semantics_with_bounds(&elements, 0);
                    }
                    robot.exit().ok();
                    std::process::exit(1);
                }

                let next_story1_y = semantics_bounds("HackerNewsStory 1000000").map(|(_, y, _, _)| y);
                let next_story_numbers = visible_mock_story_numbers((list_x, list_y, list_w, list_h));
                let next_min_story = next_story_numbers.iter().copied().min().unwrap_or(previous_min_story);
                let next_max_story = next_story_numbers.iter().copied().max().unwrap_or(previous_max_story);
                let story1_moved = match (story1_after_wheel_y, next_story1_y) {
                    (Some(previous_y), Some(next_y)) => next_y < previous_y - 12.0,
                    (Some(_), None) => true,
                    _ => false,
                };
                let visible_range_advanced =
                    next_min_story > previous_min_story || next_max_story > previous_max_story;
                if story1_moved || visible_range_advanced {
                    wheel_moves += 1;
                    stalled_scrolls = 0;
                } else {
                    stalled_scrolls += 1;
                }
                story1_after_wheel_y = next_story1_y;
                previous_min_story = next_min_story;
                previous_max_story = next_max_story;
                max_story_after_wheel = max_story_after_wheel.max(next_max_story);
                let story1_display = next_story1_y
                    .map(|y| format!("{y:.1}"))
                    .unwrap_or_else(|| "offscreen".to_string());
                println!(
                    "  • After wheel scroll #{} story1_y={} visible={:?} stalled={} max_story={}",
                    wheel_idx + 1,
                    story1_display,
                    next_story_numbers,
                    stalled_scrolls,
                    max_story_after_wheel
                );

                if stalled_scrolls >= 3 && max_story_after_wheel < 12 {
                    println!(
                        "  ✗ FAIL: Mouse wheel stopped advancing before reaching deeper items; stalled_scrolls={} max_story={}",
                        stalled_scrolls, max_story_after_wheel
                    );
                    if let Ok(elements) = robot.get_semantics() {
                        print_semantics_with_bounds(&elements, 0);
                    }
                    robot.exit().ok();
                    std::process::exit(1);
                }
            }

            if wheel_moves < 2 {
                println!(
                    "  ✗ FAIL: Mouse wheel stopped advancing the list; story1_before_y={:?} story1_after_y={:?} wheel_moves={}",
                    story1_before_wheel_y,
                    story1_after_wheel_y,
                    wheel_moves
                );
                if let Ok(elements) = robot.get_semantics() {
                    print_semantics_with_bounds(&elements, 0);
                }
                robot.exit().ok();
                std::process::exit(1);
            }

            println!("  ✓ Mouse wheel continues to advance the list after Back");
            let _ = robot.exit();
        })
        .run({
            let mock_client: HttpClientRef = Arc::new(MockHackerNewsClient::new());
            move || {
                let local = local_http_client();
                let client = mock_client.clone();
                CompositionLocalProvider(vec![local.provides(client)], || {
                    app::combined_app_with_initial_tab(Some(DemoTab::HackerNews));
                });
            }
        });
}
