mod output_paths;
mod robot_exit;
mod text_showcase_external_helpers;
mod visual_contract_metrics;

use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_testing::{
    changed_pixel_count_in_region, find_button_in_semantics, find_text_by_prefix_in_semantics,
    find_text_in_semantics, scroll_text_into_view, ScrollConfig,
};
use desktop_app::app::{self, DemoTab};
use text_showcase_external_helpers::{capture_x11_window, find_window_id, focus_x11_window};
use visual_contract_metrics::feature_stats_rgba;

type Rect = (f32, f32, f32, f32);

const WINDOW_TITLE: &str = "Robot Layout Jitter Regression Contract";
const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 900;

fn center((x, y, w, h): Rect) -> (f32, f32) {
    (x + w * 0.5, y + h * 0.5)
}

fn require_prefix(robot: &cranpose::Robot, prefix: &str) -> (Rect, String) {
    find_text_by_prefix_in_semantics(robot, prefix)
        .map(|(x, y, w, h, text)| ((x, y, w, h), text))
        .unwrap_or_else(|| panic!("text prefix {prefix:?} not found"))
}

fn assert_same_origin(label: &str, before: Rect, after: Rect, epsilon: f32) {
    let dx = after.0 - before.0;
    let dy = after.1 - before.1;
    println!(
        "{label}: before=({:.2},{:.2}) after=({:.2},{:.2}) delta=({dx:.2},{dy:.2})",
        before.0, before.1, after.0, after.1
    );
    if dx.abs() > epsilon || dy.abs() > epsilon {
        robot_exit::fail_without_shutdown(&format!(
            "{label} moved unexpectedly by ({dx:.2},{dy:.2})"
        ));
    }
}

fn select_tab(robot: &cranpose::Robot, tab: DemoTab) {
    let Some((x, y, w, h)) = find_button_in_semantics(robot, tab.label()) else {
        robot_exit::fail_without_shutdown(&format!("tab '{}' not found", tab.label()));
    };
    robot.click(x + w * 0.5, y + h * 0.5).unwrap_or_else(|err| {
        robot_exit::fail_without_shutdown(&format!("click tab failed: {err}"))
    });
    std::thread::sleep(Duration::from_millis(260));
    let _ = robot.wait_for_idle();
}

fn assert_animation_jitter(robot: &cranpose::Robot) {
    select_tab(robot, DemoTab::Animations);
    if find_text_in_semantics(robot, "Animations Showcase").is_none() {
        robot_exit::fail_without_shutdown("Animations Showcase did not load");
    }

    let ((initial_alpha_x, initial_alpha_y, _, _), alpha_text) = require_prefix(robot, "Alpha ");
    println!("initial alpha text={alpha_text:?} y={initial_alpha_y:.2}");
    for frame in 0..12 {
        let _ = robot.wait_for_present_frame();
        let ((x, y, _, _), text) = require_prefix(robot, "Alpha ");
        let dx = x - initial_alpha_x;
        let dy = y - initial_alpha_y;
        println!("alpha frame={frame} text={text:?} delta=({dx:.2},{dy:.2})");
        if dx.abs() > 0.75 || dy.abs() > 0.75 {
            robot_exit::fail_without_shutdown(&format!(
                "Pulse Alpha text jumped during animation frame {frame}: delta=({dx:.2},{dy:.2})"
            ));
        }
    }

    let window_id = find_window_id(WINDOW_TITLE);
    focus_x11_window(&window_id);
    let alpha_bounds = find_text_by_prefix_in_semantics(robot, "Alpha ")
        .map(|(x, y, w, h, _)| (x, y, w, h))
        .unwrap_or_else(|| {
            robot_exit::fail_without_shutdown(
                "Alpha row label was not visible for visual stability",
            )
        });
    let alpha_crop = (
        (alpha_bounds.0 - 6.0).max(0.0) as u32,
        (alpha_bounds.1 - 6.0).max(0.0) as u32,
        1040,
        38,
    );
    let mut alpha_base_y = None;
    for frame in 0..12 {
        let _ = robot.wait_for_present_frame();
        let path = output_paths::diagnostic_path(&format!("layout_jitter_alpha_{frame:02}.png"));
        let image = capture_x11_window(&window_id, &path);
        let Some(edge_y) = row_top_edge_y(&image, alpha_crop) else {
            println!("alpha visual frame={frame}: row too transparent, skipped");
            continue;
        };
        let Some(base_y) = alpha_base_y else {
            alpha_base_y = Some(edge_y);
            println!("alpha visual base edge_y={edge_y:.2}");
            continue;
        };
        let dy = edge_y - base_y;
        println!("alpha visual frame={frame} edge_y={edge_y:.2} delta={dy:.2}");
        if dy.abs() > 1.5 {
            robot_exit::fail_without_shutdown(&format!(
                "Pulse Alpha row jumped vertically in presented pixels: delta={dy:.2}"
            ));
        }
    }
    if alpha_base_y.is_none() {
        robot_exit::fail_without_shutdown(
            "Pulse Alpha row edge never became visible across 12 frames",
        );
    }

    let label_bounds = find_text_in_semantics(robot, "Animation sampled inside graphics_layer")
        .unwrap_or_else(|| {
            robot_exit::fail_without_shutdown(
                "Scale + Fade label was not visible for orange-box stability check",
            )
        });
    let crop_left = (label_bounds.0 - 90.0).max(0.0) as u32;
    let crop_top = (label_bounds.1 - 32.0).max(0.0) as u32;
    let crop_region = (crop_left, crop_top, 110, 90);
    let first_path = output_paths::diagnostic_path("layout_jitter_animation_orange_first.png");
    let first = capture_x11_window(&window_id, &first_path);
    let first_stats =
        feature_stats_rgba(&first, crop_region, is_orange_box_pixel).unwrap_or_else(|| {
            robot_exit::fail_without_shutdown("orange animation box pixels not found")
        });
    let base_y = first_stats.centroid_y();
    println!("orange frame=base stats={first_stats:?} centroid_y={base_y:.2}");

    for frame in 0..8 {
        let _ = robot.wait_for_present_frame();
        let path = output_paths::diagnostic_path(&format!(
            "layout_jitter_animation_orange_{frame:02}.png"
        ));
        let image = capture_x11_window(&window_id, &path);
        let stats =
            feature_stats_rgba(&image, crop_region, is_orange_box_pixel).unwrap_or_else(|| {
                robot_exit::fail_without_shutdown("orange animation box pixels disappeared")
            });
        let dy = stats.centroid_y() - base_y;
        println!("orange frame={frame} stats={stats:?} centroid_y_delta={dy:.2}");
        if dy.abs() > 1.5 {
            robot_exit::fail_without_shutdown(&format!(
                "orange Scale + Fade box jumped vertically: delta={dy:.2}"
            ));
        }
    }
}

fn assert_text_input_jitter(robot: &cranpose::Robot) {
    select_tab(robot, DemoTab::TextInput);
    if find_text_in_semantics(robot, "Text Input Demo").is_none() {
        robot_exit::fail_without_shutdown("Text Input Demo did not load");
    }

    let (field_before, _) = require_prefix(robot, "Type here");
    let (value_before, value_text) = require_prefix(robot, "Current value:");
    println!("text input before value={value_text:?}");
    let before_screenshot = robot.screenshot().expect("text input before screenshot");
    let (click_x, click_y) = center(field_before);
    robot.click(click_x, click_y).expect("click text input");
    std::thread::sleep(Duration::from_millis(80));
    robot.type_text("z").expect("type text input char");
    std::thread::sleep(Duration::from_millis(220));
    let _ = robot.wait_for_idle();

    let (field_after, field_text_after) = require_prefix(robot, "Type here");
    let (value_after, value_text_after) = require_prefix(robot, "Current value:");
    let after_screenshot = robot.screenshot().expect("text input after screenshot");
    println!("text input after field={field_text_after:?} value={value_text_after:?}");
    assert_same_origin("text input field block", field_before, field_after, 1.0);
    assert_same_origin(
        "text input current-value block",
        value_before,
        value_after,
        1.0,
    );
    let region = union_rect(field_before, value_before, 14.0);
    let diff = changed_pixel_count_in_region(&before_screenshot, &after_screenshot, region, 10);
    println!("text input visual diff={diff} region={region:?}");
    if diff > 4_500 {
        robot_exit::fail_without_shutdown(&format!(
            "typing changed too many pixels around the text input block: diff={diff}"
        ));
    }
}

fn assert_dynamic_modifier_moves(robot: &cranpose::Robot) {
    select_tab(robot, DemoTab::ModifierShowcase);
    let Some((x, y, w, h)) = find_button_in_semantics(robot, "Dynamic Modifiers") else {
        robot_exit::fail_without_shutdown("Dynamic Modifiers selector not found");
    };
    robot
        .click(x + w * 0.5, y + h * 0.5)
        .expect("click Dynamic Modifiers selector");
    std::thread::sleep(Duration::from_millis(220));
    let _ = robot.wait_for_idle();

    if scroll_text_into_view(robot, "Move", 12, modifier_scroll_config()).is_none() {
        robot_exit::fail_without_shutdown("Move box text not visible");
    }
    let move_before = find_text_in_semantics(robot, "Move").expect("Move text visible");
    let frame_before = require_prefix(robot, "Frame:").1;
    let Some((bx, by, bw, bh)) = find_button_in_semantics(robot, "Advance Frame") else {
        robot_exit::fail_without_shutdown("Advance Frame button not found");
    };
    robot
        .click(bx + bw * 0.5, by + bh * 0.5)
        .expect("click Advance Frame");
    std::thread::sleep(Duration::from_millis(240));
    let _ = robot.wait_for_idle();

    let move_after = find_text_in_semantics(robot, "Move").expect("Move text still visible");
    let frame_after = require_prefix(robot, "Frame:").1;
    let dx = move_after.0 - move_before.0;
    let dy = move_after.1 - move_before.1;
    println!(
        "dynamic modifiers: frame_before={frame_before:?} frame_after={frame_after:?} move_before={move_before:?} move_after={move_after:?} delta=({dx:.2},{dy:.2})"
    );
    if frame_before == frame_after {
        robot_exit::fail_without_shutdown("Advance Frame did not update the frame text");
    }
    if dx.abs() < 8.0 || dy.abs() > 0.75 {
        robot_exit::fail_without_shutdown(
            &format!("Advance Frame updated text but did not move the Move box horizontally enough: delta=({dx:.2},{dy:.2})"));
    }
}

fn modifier_scroll_config() -> ScrollConfig {
    ScrollConfig {
        center_x: 680.0,
        down_from_y: 760.0,
        down_to_y: 220.0,
        up_from_y: 220.0,
        up_to_y: 760.0,
    }
}

fn union_rect(a: Rect, b: Rect, padding: f32) -> Rect {
    let min_x = a.0.min(b.0) - padding;
    let min_y = a.1.min(b.1) - padding;
    let max_x = (a.0 + a.2).max(b.0 + b.2) + padding;
    let max_y = (a.1 + a.3).max(b.1 + b.3) + padding;
    (min_x.max(0.0), min_y.max(0.0), max_x - min_x, max_y - min_y)
}

fn is_orange_box_pixel(pixel: [u8; 4]) -> bool {
    let [red, green, blue, alpha] = pixel;
    alpha > 170
        && red > 205
        && (145..=225).contains(&green)
        && (120..=210).contains(&blue)
        && red > green.saturating_add(8)
        && red > blue.saturating_add(24)
}

fn row_top_edge_y(image: &image::RgbaImage, region: (u32, u32, u32, u32)) -> Option<f64> {
    let (left, top, width, height) = region;
    let right = left.saturating_add(width).min(image.width());
    let bottom = top.saturating_add(height).min(image.height());
    let luma = |x: u32, y: u32| {
        let pixel = image.get_pixel(x, y).0;
        (pixel[0] as i16 + pixel[1] as i16 + pixel[2] as i16) / 3
    };
    let mut edge_ys: Vec<u32> = Vec::new();
    for x in left..right {
        for y in top + 1..bottom {
            if (luma(x, y) - luma(x, y - 1)).abs() >= 14 {
                edge_ys.push(y);
                break;
            }
        }
    }
    if edge_ys.len() < 200 {
        return None;
    }
    edge_ys.sort_unstable();
    Some(edge_ys[edge_ys.len() / 2] as f64)
}

fn main() {
    env_logger::init();
    println!("=== Robot Layout Jitter Regression Contract ===");

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(false)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(800));
            let _ = robot.wait_for_idle();

            let mode = std::env::var("CRANPOSE_LAYOUT_JITTER_MODE")
                .ok()
                .unwrap_or_else(|| "all".to_string());
            if mode == "animation" || mode == "all" {
                assert_animation_jitter(&robot);
            }
            if mode == "textinput" || mode == "all" {
                assert_text_input_jitter(&robot);
            }
            if mode == "modifier" || mode == "all" {
                assert_dynamic_modifier_moves(&robot);
            }

            println!("PASS: layout jitter contracts passed");
            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}
