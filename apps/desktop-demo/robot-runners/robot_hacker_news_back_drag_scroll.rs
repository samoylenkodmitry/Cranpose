pub mod hacker_news_robot_support;

use cranpose::AppLauncher;
use cranpose_core::CompositionLocalProvider;
use cranpose_services::local_http_client;
use cranpose_testing::{find_in_semantics, find_text_exact};
use desktop_app::app::{self, DemoTab};
use hacker_news_robot_support::{
    click_button, click_first_visible_comments_button, create_mock_client, fail, raw_drag,
    semantics_bounds, settle_visible_mock_story_numbers, visible_mock_story_numbers,
    wait_for_comments_data, wait_for_no_text, wait_for_text,
};
use std::time::Duration;

const DRAG_MEASUREMENT_GESTURES: usize = 1;
const DRAG_SETTLE_ATTEMPTS: usize = 8;
const REWIND_GESTURE_LIMIT: usize = 3;

#[derive(Debug)]
struct DragMeasurement {
    before_visible: Vec<usize>,
    after_visible: Vec<usize>,
    min_story_advance: usize,
    max_story_advance: usize,
}

fn measure_drag_progress(
    robot: &cranpose::Robot,
    list_bounds: (f32, f32, f32, f32),
    label: &str,
) -> DragMeasurement {
    let before_visible = visible_mock_story_numbers(robot, list_bounds);
    let (list_x, list_y, list_w, list_h) = list_bounds;
    let drag_x = list_x + list_w * 0.5;
    let drag_start_y = list_y + list_h * 0.82;
    let drag_end_y = list_y + list_h * 0.22;
    let mut after_visible = before_visible.clone();

    println!("  • {label} before raw drag: {before_visible:?}");
    for gesture in 0..DRAG_MEASUREMENT_GESTURES {
        raw_drag(
            robot,
            drag_x,
            drag_start_y,
            drag_end_y,
            12,
            Duration::from_millis(16),
        );
        after_visible = settle_visible_mock_story_numbers(
            robot,
            list_bounds,
            DRAG_SETTLE_ATTEMPTS,
            Duration::from_millis(40),
        );

        let reopened_discussion = find_in_semantics(robot, |elem| find_text_exact(elem, "Back"))
            .is_some()
            || find_in_semantics(robot, |elem| find_text_exact(elem, "commenter-1")).is_some();
        if reopened_discussion {
            fail(
                robot,
                format!(
                    "{label} raw drag reopened the discussion instead of scrolling the story list on gesture {}",
                    gesture + 1
                ),
            );
        }

        println!(
            "  • {label} after raw drag #{}: {:?}",
            gesture + 1,
            after_visible
        );
    }

    let min_story_advance = after_visible
        .iter()
        .copied()
        .min()
        .unwrap_or(0)
        .saturating_sub(before_visible.iter().copied().min().unwrap_or(0));
    let max_story_advance = after_visible
        .iter()
        .copied()
        .max()
        .unwrap_or(0)
        .saturating_sub(before_visible.iter().copied().max().unwrap_or(0));

    DragMeasurement {
        before_visible,
        after_visible,
        min_story_advance,
        max_story_advance,
    }
}

fn open_hacker_news_tab(robot: &cranpose::Robot) -> (f32, f32, f32, f32) {
    println!("  • settling initial Hacker News startup");
    std::thread::sleep(Duration::from_millis(500));
    let _ = robot.wait_for_idle();

    println!("  • waiting for first mock story");
    if !wait_for_text(robot, "Mock Story #1") {
        fail(robot, "mock stories did not appear");
    }

    println!("  • locating HackerNewsList semantics");
    let Some(list_bounds) = semantics_bounds(robot, "HackerNewsList") else {
        fail(robot, "HackerNewsList semantics not found");
    };
    list_bounds
}

fn rewind_story_list_to_top(robot: &cranpose::Robot, list_bounds: (f32, f32, f32, f32)) {
    let (list_x, list_y, list_w, list_h) = list_bounds;
    let drag_x = list_x + list_w * 0.5;
    let drag_start_y = list_y + list_h * 0.22;
    let drag_end_y = list_y + list_h * 0.82;

    for _ in 0..REWIND_GESTURE_LIMIT {
        let visible = visible_mock_story_numbers(robot, list_bounds);
        if visible.first().copied().unwrap_or(usize::MAX) <= 1 {
            return;
        }

        raw_drag(
            robot,
            drag_x,
            drag_start_y,
            drag_end_y,
            12,
            Duration::from_millis(16),
        );
        let _ = settle_visible_mock_story_numbers(
            robot,
            list_bounds,
            DRAG_SETTLE_ATTEMPTS,
            Duration::from_millis(40),
        );
    }

    let visible = visible_mock_story_numbers(robot, list_bounds);
    if visible.first().copied().unwrap_or(usize::MAX) > 1 {
        fail(
            robot,
            format!("could not restore the story list to the top before the Back scenario; visible={visible:?}"),
        );
    }
}

fn main() {
    env_logger::init();
    println!("=== Hacker News Back Drag Scroll Robot Test ===");
    AppLauncher::new()
        .with_title("Hacker News Back Drag Scroll Robot Test")
        .with_size(390, 844)
        .with_headless(true)
        .with_test_driver(|robot| {
            println!("  • opening Hacker News startup");
            let list_bounds = open_hacker_news_tab(&robot);
            println!("  • measuring fresh drag baseline");
            let fresh_drag = measure_drag_progress(&robot, list_bounds, "fresh list");
            if fresh_drag.max_story_advance < 3 || fresh_drag.min_story_advance < 2 {
                fail(
                    &robot,
                    format!(
                        "fresh story list did not move enough for a meaningful baseline; before={:?} after={:?} min_advance={} max_advance={}",
                        fresh_drag.before_visible,
                        fresh_drag.after_visible,
                        fresh_drag.min_story_advance,
                        fresh_drag.max_story_advance
                    ),
                );
            }

            println!("  • rewinding list to top");
            rewind_story_list_to_top(&robot, list_bounds);

            println!("  • opening first visible comments");
            if !click_first_visible_comments_button(&robot, list_bounds) {
                fail(&robot, "could not click the first visible comments button");
            }

            println!("  • waiting for comments");
            if !wait_for_comments_data(&robot) {
                fail(&robot, "comments did not load");
            }

            println!("  • clicking Back");
            if !click_button(&robot, "Back") {
                fail(&robot, "Back button was not found after opening comments");
            }

            println!("  • waiting for story list restoration");
            if !wait_for_no_text(&robot, "Back") {
                fail(&robot, "Back button stayed visible after returning to the story list");
            }

            println!("  • locating restored HackerNewsList semantics");
            let Some(list_bounds) = semantics_bounds(&robot, "HackerNewsList") else {
                fail(&robot, "HackerNewsList semantics not found after Back");
            };
            let restored_visible = visible_mock_story_numbers(&robot, list_bounds);
            if restored_visible.is_empty() {
                fail(&robot, "story list did not return after Back");
            }
            if restored_visible.first().copied().unwrap_or(usize::MAX) > 1 {
                fail(
                    &robot,
                    format!(
                        "story list did not restore the top position before the Back drag check; visible={restored_visible:?}"
                    ),
                );
            }

            println!("  • measuring restored drag");
            let restored_drag = measure_drag_progress(&robot, list_bounds, "restored list");
            if restored_drag.max_story_advance < 3 || restored_drag.min_story_advance < 2 {
                fail(
                    &robot,
                    format!(
                        "restored story list did not advance enough after raw mouse drag; before={:?} after={:?} min_advance={} max_advance={}",
                        restored_drag.before_visible,
                        restored_drag.after_visible,
                        restored_drag.min_story_advance,
                        restored_drag.max_story_advance
                    ),
                );
            }

            let restored_is_materially_slower =
                restored_drag.max_story_advance * 4 < fresh_drag.max_story_advance * 3
                    || restored_drag.min_story_advance * 4 < fresh_drag.min_story_advance * 3;
            if restored_is_materially_slower {
                fail(
                    &robot,
                    format!(
                        "restored story list drag speed regressed after Back; fresh_before={:?} fresh_after={:?} fresh_min_advance={} fresh_max_advance={} restored_before={:?} restored_after={:?} restored_min_advance={} restored_max_advance={}",
                        fresh_drag.before_visible,
                        fresh_drag.after_visible,
                        fresh_drag.min_story_advance,
                        fresh_drag.max_story_advance,
                        restored_drag.before_visible,
                        restored_drag.after_visible,
                        restored_drag.min_story_advance,
                        restored_drag.max_story_advance
                    ),
                );
            }

            println!("  ✓ Story list scroll speed after Back matches the fresh-list baseline");
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
