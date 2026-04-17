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

pub mod hacker_news_robot_support;

use cranpose::{AppLauncher, Robot};
use cranpose_core::CompositionLocalProvider;
use cranpose_services::local_http_client;
use cranpose_testing::{find_in_semantics, find_text_exact, root_bounds};
use desktop_app::app::{self, DemoTab};
use hacker_news_robot_support::{
    click_button, click_first_visible_comments_button, create_mock_client, fail, semantics_bounds,
    visible_mock_story_numbers, wait_for_no_text, wait_for_text, Bounds,
};
use std::time::Duration;

fn required_bounds(robot: &Robot, label: &str) -> Bounds {
    semantics_bounds(robot, label)
        .unwrap_or_else(|| fail(robot, format!("{label} semantics not found")))
}

fn assert_within_root(robot: &Robot, name: &str, bounds: Bounds) {
    let Some((root_x, root_y, root_w, root_h)) = root_bounds(robot) else {
        fail(robot, "missing root bounds");
    };
    let (x, y, w, h) = bounds;
    let root_right = root_x + root_w;
    let root_bottom = root_y + root_h;
    let right = x + w;
    let bottom = y + h;
    if x < root_x || y < root_y || right > root_right || bottom > root_bottom {
        fail(
            robot,
            format!(
                "{name} bounds=({x:.1},{y:.1},{w:.1},{h:.1}) exceed root=({root_x:.1},{root_y:.1},{root_w:.1},{root_h:.1})"
            ),
        );
    }
}

fn comment_intersects_viewport(robot: &Robot, comments_list_bounds: Bounds, label: &str) -> bool {
    let (comments_x, comments_y, comments_w, comments_h) = comments_list_bounds;
    semantics_bounds(robot, label).is_some_and(|(x, y, w, h)| {
        let right = x + w;
        let bottom = y + h;
        bottom > comments_y
            && y < comments_y + comments_h
            && right > comments_x
            && x < comments_x + comments_w
    })
}

fn reopened_discussion(robot: &Robot) -> bool {
    find_in_semantics(robot, |elem| find_text_exact(elem, "Back")).is_some()
        || find_in_semantics(robot, |elem| find_text_exact(elem, "commenter-1")).is_some()
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

            if !wait_for_text(&robot, "Mock Story #1") {
                fail(&robot, "mock stories did not appear");
            }

            let list_bounds = required_bounds(&robot, "HackerNewsList");
            {
                let (x, y, w, h) = list_bounds;
                println!("  ✓ HackerNewsList bounds=({x:.1},{y:.1},{w:.1},{h:.1})");
                if h > 760.0 {
                    fail(
                        &robot,
                        format!("HackerNewsList height {h:.1} exceeds viewport expectations"),
                    );
                }
            }

            let list_rail_bounds = required_bounds(&robot, "HackerNewsListScrollbarRail");
            assert_within_root(&robot, "HackerNewsList", list_bounds);
            assert_within_root(&robot, "HackerNewsListScrollbarRail", list_rail_bounds);

            let initial_story_numbers = visible_mock_story_numbers(&robot, list_bounds);
            println!("  ✓ Initial visible stories: {initial_story_numbers:?}");

            if !click_first_visible_comments_button(&robot, list_bounds) {
                fail(&robot, "could not select the target story");
            }

            if !wait_for_text(&robot, "commenter-1") {
                fail(&robot, "mock comments did not appear");
            }

            let comments_list_bounds = required_bounds(&robot, "HackerNewsCommentsList");
            let comments_rail_bounds = required_bounds(&robot, "HackerNewsCommentsScrollbarRail");
            assert_within_root(&robot, "HackerNewsCommentsList", comments_list_bounds);
            assert_within_root(
                &robot,
                "HackerNewsCommentsScrollbarRail",
                comments_rail_bounds,
            );
            let (comments_x, comments_y, comments_w, comments_h) = comments_list_bounds;
            let scroll_start_x = comments_x + comments_w * 0.5;
            let scroll_start_y = comments_y + comments_h * 0.75;
            let scroll_end_y = comments_y + comments_h * 0.25;
            let comment1_before_y = semantics_bounds(&robot, "commenter-1")
                .map(|(_, y, _, _)| y)
                .unwrap_or(comments_y);

            for _ in 0..4 {
                robot
                    .drag(scroll_start_x, scroll_start_y, scroll_start_x, scroll_end_y)
                    .ok();
                std::thread::sleep(Duration::from_millis(250));
                let _ = robot.wait_for_idle();
            }

            let comment1_after_y = semantics_bounds(&robot, "commenter-1")
                .map(|(_, y, _, _)| y)
                .unwrap_or(comments_y - 100.0);
            let first_comment_still_in_view =
                comment_intersects_viewport(&robot, comments_list_bounds, "commenter-1");
            let later_comment_visible =
                comment_intersects_viewport(&robot, comments_list_bounds, "commenter-8")
                    || comment_intersects_viewport(&robot, comments_list_bounds, "commenter-12")
                    || comment_intersects_viewport(&robot, comments_list_bounds, "commenter-18");
            let comment1_moved = comment1_after_y < comment1_before_y - 80.0;

            if first_comment_still_in_view && !later_comment_visible && !comment1_moved {
                fail(&robot, "Comments pane did not scroll after first load");
            }

            println!("  ✓ Comments pane scrolls on the first load");
            println!("  ✓ Comments pane bounds stay within the window");

            if !click_button(&robot, "Back") {
                fail(&robot, "could not find Back button after opening comments");
            }

            if !wait_for_no_text(&robot, "Back") {
                fail(
                    &robot,
                    "Back button stayed visible after returning to the story list",
                );
            }

            if !wait_for_text(&robot, "Mock Story #1") {
                fail(&robot, "Story list did not return after Back");
            }

            let list_bounds = required_bounds(&robot, "HackerNewsList");
            let (list_x, list_y, list_w, list_h) = list_bounds;
            let restored_story_numbers = visible_mock_story_numbers(&robot, list_bounds);
            println!("  ✓ Restored visible stories: {restored_story_numbers:?}");
            let max_story_before_drag = restored_story_numbers.iter().copied().max().unwrap_or(0);
            let story1_before_drag_y = semantics_bounds(&robot, "HackerNewsStory 1000000")
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

                if reopened_discussion(&robot) {
                    fail(
                        &robot,
                        format!(
                            "Drag on restored story list reopened the discussion on drag #{}",
                            drag_index + 1
                        ),
                    );
                }

                scrolled_story_numbers = visible_mock_story_numbers(&robot, list_bounds);
                let max_story_after_drag =
                    scrolled_story_numbers.iter().copied().max().unwrap_or(0);
                story1_after_drag_y = semantics_bounds(&robot, "HackerNewsStory 1000000")
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
                fail(
                    &robot,
                    format!(
                        "Restored story list did not move after drag; before_max={} after={:?} story1_before_y={:.1} story1_after_y={:.1}",
                        max_story_before_drag,
                        scrolled_story_numbers,
                        story1_before_drag_y,
                        story1_after_drag_y
                    ),
                );
            }

            println!("  ✓ Restored story list scrolls after Back");

            let scroll_probe_x = list_x + list_w * 0.5;
            let scroll_probe_y = list_y + list_h * 0.5;
            let story1_before_wheel_y =
                semantics_bounds(&robot, "HackerNewsStory 1000000").map(|(_, y, _, _)| y);
            let mut story1_after_wheel_y = story1_before_wheel_y;
            let mut wheel_moves = 0usize;
            let mut stalled_scrolls = 0usize;
            let visible_after_drag = visible_mock_story_numbers(&robot, list_bounds);
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

                if reopened_discussion(&robot) {
                    fail(
                        &robot,
                        format!(
                            "Mouse wheel reopened the discussion on scroll #{}",
                            wheel_idx + 1
                        ),
                    );
                }

                let next_story1_y =
                    semantics_bounds(&robot, "HackerNewsStory 1000000").map(|(_, y, _, _)| y);
                let next_story_numbers = visible_mock_story_numbers(&robot, list_bounds);
                let next_min_story = next_story_numbers
                    .iter()
                    .copied()
                    .min()
                    .unwrap_or(previous_min_story);
                let next_max_story = next_story_numbers
                    .iter()
                    .copied()
                    .max()
                    .unwrap_or(previous_max_story);
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
                    fail(
                        &robot,
                        format!(
                            "Mouse wheel stopped advancing before reaching deeper items; stalled_scrolls={} max_story={}",
                            stalled_scrolls, max_story_after_wheel
                        ),
                    );
                }
            }

            if wheel_moves < 2 {
                fail(
                    &robot,
                    format!(
                        "Mouse wheel stopped advancing the list; story1_before_y={:?} story1_after_y={:?} wheel_moves={}",
                        story1_before_wheel_y,
                        story1_after_wheel_y,
                        wheel_moves
                    ),
                );
            }

            println!("  ✓ Mouse wheel continues to advance the list after Back");
            let _ = robot.exit();
        })
        .run({
            let mock_client = create_mock_client();
            move || {
                let local = local_http_client();
                CompositionLocalProvider(vec![local.provides(mock_client.clone())], || {
                    app::combined_app_with_initial_tab(Some(DemoTab::HackerNews));
                });
            }
        });
}
