use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use cranpose::{AppLauncher, RobotScreenshot, SemanticElement};
use cranpose_testing::{find_button, find_in_semantics, find_text};
use desktop_app::app;
use image::RgbaImage;

mod robot_exit;
mod text_input_robot_helpers;

type SelectedEditable = ((f32, f32, f32, f32), (usize, usize));

fn main() {
    env_logger::init();
    println!("=== Click-Drag Selection Test ===\n");
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR").unwrap_or_else(|_| "target/drag-selection".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create screenshot directory");

    AppLauncher::new()
        .with_title("Drag Selection Test")
        .with_size(600, 400)
        .with_headless(true)
        .with_test_driver(move |robot| {
            robot_exit::arm_timeout(60);

            std::thread::sleep(Duration::from_millis(300));
            println!("✓ App ready\n");

            println!("--- Step 1: Switch to Text Input Tab ---");
            if text_input_robot_helpers::open_text_input_tab(&robot) {
                println!("✓ Clicked Text Input tab\n");
            } else {
                println!("✗ FAIL: Could not find Text Input tab");
                let _ = robot.exit();
                return;
            }

            println!("--- Step 2: Find text field ---");
            if let Some((field_x, field_y, field_w, field_h)) =
                text_input_robot_helpers::wait_for_in_semantics(&robot, |robot| {
                    find_in_semantics(robot, |elem| find_text(elem, "Type here..."))
                })
            {
                println!("✓ Found text field at ({:.0}, {:.0})\n", field_x, field_y);

                println!("--- Step 3: Add text ---");
                for _ in 0..5 {
                    if let Some((x, y, w, h)) =
                        find_in_semantics(&robot, |elem| find_button(elem, "Add !"))
                    {
                        let cx = x + w / 2.0;
                        let cy = y + h / 2.0;
                        let _ = robot.mouse_move(cx, cy);
                        std::thread::sleep(Duration::from_millis(20));
                        let _ = robot.mouse_down();
                        std::thread::sleep(Duration::from_millis(20));
                        let _ = robot.mouse_up();
                        std::thread::sleep(Duration::from_millis(30));
                    }
                }
                std::thread::sleep(Duration::from_millis(200));
                println!("✓ Added text\n");

                println!("--- Step 4: Click-drag selection ---");

                let start_x = field_x + field_w - 10.0;
                let end_x = field_x + 10.0;
                let center_y = field_y + field_h / 2.0;

                let _ = robot.mouse_move(start_x, center_y);
                std::thread::sleep(Duration::from_millis(50));

                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));

                let focused = robot.has_focused_text_field().unwrap_or(false);
                println!("  • Field focused (has_focused_field): {}", focused);
                if !focused {
                    println!("    (Note: app-thread focus query returned false)");
                }

                for step in 1..=3 {
                    let t = step as f32 / 3.0;
                    let drag_x = start_x + (end_x - start_x) * t;
                    let _ = robot.mouse_move(drag_x, center_y);
                    std::thread::sleep(Duration::from_millis(50));
                }
                println!("  • Dragged across text");

                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(100));

                let still_focused = robot.has_focused_text_field().unwrap_or(false);
                println!(
                    "  • Still focused after drag (has_focused_field): {}",
                    still_focused
                );
                if !still_focused {
                    println!("    (Note: app-thread focus query returned false)");
                }

                let semantics = robot.get_semantics().expect("selection semantics");
                let (selected_bounds, selection) = selected_editable(&semantics)
                    .expect("mouse drag must publish a non-collapsed text selection");
                let shot = robot.screenshot().expect("desktop handle proof frame");
                save(&shot, &shot_dir, "desktop-mouse-selection-handles");
                let (start_handle_pixels, end_handle_pixels) =
                    count_blue_handle_bands(&shot, selected_bounds);
                println!(
                    "  • Selection {:?}, start/end handle pixels: {}/{}",
                    selection, start_handle_pixels, end_handle_pixels
                );
                assert!(
                    start_handle_pixels >= 8 && end_handle_pixels >= 8,
                    "desktop pointer selection must render both handles; found {start_handle_pixels}/{end_handle_pixels} solid-blue pixels"
                );

                let end_handle = lower_handle_center(&shot, selected_bounds)
                    .expect("rendered end handle must have a measurable blue dot");
                let before = normalized(selection);
                let drag_targets = [
                    (end_handle.0 - 24.0).max(selected_bounds.0 + 8.0),
                    (end_handle.0 - 48.0).max(selected_bounds.0 + 8.0),
                ];
                println!(
                    "  • Mouse-grabbing end handle at ({:.1}, {:.1})",
                    end_handle.0, end_handle.1
                );
                robot
                    .mouse_move(end_handle.0, end_handle.1)
                    .expect("move mouse to end handle");
                std::thread::sleep(Duration::from_millis(20));
                robot.mouse_down().expect("mouse down on end handle");
                let mut previous = before;
                let mut last_handle_x = end_handle.0;
                for (index, drag_x) in drag_targets.into_iter().enumerate() {
                    robot
                        .mouse_move(drag_x, end_handle.1)
                        .expect("direct mouse handle move");
                    robot
                        .wait_for_present_frame()
                        .expect("present direct mouse handle frame");
                    let moved_shot = robot
                        .screenshot()
                        .expect("rendered desktop handle-drag input frame");
                    save(
                        &moved_shot,
                        &shot_dir,
                        &format!("desktop-mouse-handle-direct-follow-{index}"),
                    );
                    let moved_semantics = robot.get_semantics().expect("moved selection semantics");
                    let (moved_bounds, moved_selection) = selected_editable(&moved_semantics)
                        .expect("mouse handle drag must preserve a range selection");
                    let after = normalized(moved_selection);
                    assert_eq!(
                        after.0, before.0,
                        "dragging the end handle must keep the opposite endpoint fixed"
                    );
                    assert!(
                        after.1 < previous.1,
                        "each delivered desktop mouse move must advance its endpoint: {previous:?} → {after:?}"
                    );
                    let moved_handle = lower_handle_center(&moved_shot, moved_bounds)
                        .expect("moved end handle must remain measurable");
                    assert!(
                        (moved_handle.0 - drag_x).abs() <= 34.0,
                        "end handle did not follow the mouse in input frame {index}: pointer={drag_x:.1}, handle={:.1}",
                        moved_handle.0
                    );
                    previous = after;
                    last_handle_x = moved_handle.0;
                }
                robot.mouse_up().expect("release desktop end handle");
                println!(
                    "  • Endpoint {:?} → {:?}, handle x={:.1} (pointer x={:.1})",
                    before,
                    previous,
                    last_handle_x,
                    drag_targets[1]
                );

                println!("\n✓ PASS: Click-drag selection test completed");
                let _ = robot.exit();
            } else {
                println!("✗ FAIL: Could not find text field after waiting for semantics");
                let _ = robot.exit();
            }
        })
        .run(|| {
            app::combined_app();
        });
}

fn normalized(selection: (usize, usize)) -> (usize, usize) {
    (selection.0.min(selection.1), selection.0.max(selection.1))
}

fn lower_handle_center(shot: &RobotScreenshot, rect: (f32, f32, f32, f32)) -> Option<(f32, f32)> {
    let (sx, sy) = (
        shot.width as f32 / shot.logical_width.max(1.0),
        shot.height as f32 / shot.logical_height.max(1.0),
    );
    let (x, y, width, height) = rect;
    let left = (x.max(0.0) * sx) as usize;
    let right = (((x + width) * sx) as usize).min(shot.width as usize);
    let top = (((y + height * 0.5) * sy) as usize).min(shot.height as usize);
    let bottom = (((y + height + 8.0) * sy) as usize).min(shot.height as usize);

    let is_blue = |px: usize, py: usize| {
        let i = (py * shot.width as usize + px) * 4;
        let (r, g, b) = (shot.pixels[i], shot.pixels[i + 1], shot.pixels[i + 2]);
        b > 170 && b.saturating_sub(r) > 55 && b.saturating_sub(g) > 25
    };
    let max_y = (top..bottom)
        .rev()
        .find(|&py| (left..right).any(|px| is_blue(px, py)))?;
    let band_top = max_y.saturating_sub((4.0 * sy).ceil() as usize);
    let mut sum_x = 0usize;
    let mut sum_y = 0usize;
    let mut count = 0usize;
    for py in band_top..=max_y {
        for px in left..right {
            if is_blue(px, py) {
                sum_x += px;
                sum_y += py;
                count += 1;
            }
        }
    }
    (count > 0).then(|| {
        (
            sum_x as f32 / count as f32 / sx,
            sum_y as f32 / count as f32 / sy,
        )
    })
}

fn selected_editable(elements: &[SemanticElement]) -> Option<SelectedEditable> {
    for element in elements {
        if element.editable_text {
            if let Some(selection) = element.text_selection {
                if selection.0 != selection.1 {
                    let bounds = element.bounds;
                    return Some(((bounds.x, bounds.y, bounds.width, bounds.height), selection));
                }
            }
        }
        if let Some(found) = selected_editable(&element.children) {
            return Some(found);
        }
    }
    None
}

fn count_blue_handle_bands(shot: &RobotScreenshot, rect: (f32, f32, f32, f32)) -> (usize, usize) {
    let (sx, sy) = (
        shot.width as f32 / shot.logical_width.max(1.0),
        shot.height as f32 / shot.logical_height.max(1.0),
    );
    let (x, y, width, height) = rect;
    let left = (x.max(0.0) * sx) as usize;
    let top = (y.max(0.0) * sy) as usize;
    let right = (((x + width) * sx) as usize).min(shot.width as usize);
    let bottom = (((y + height) * sy) as usize).min(shot.height as usize);
    let upper_end = ((y + height * 0.45) * sy) as usize;
    let lower_start = ((y + height * 0.55) * sy) as usize;
    let mut upper = 0;
    let mut lower = 0;
    for py in top..bottom {
        for px in left..right {
            let i = (py * shot.width as usize + px) * 4;
            let (r, g, b) = (shot.pixels[i], shot.pixels[i + 1], shot.pixels[i + 2]);
            if b > 170 && b.saturating_sub(r) > 55 && b.saturating_sub(g) > 25 {
                if py < upper_end {
                    upper += 1;
                } else if py >= lower_start {
                    lower += 1;
                }
            }
        }
    }
    (upper, lower)
}

fn save(shot: &RobotScreenshot, dir: &Path, name: &str) {
    let image = RgbaImage::from_raw(shot.width, shot.height, shot.pixels.clone())
        .expect("screenshot buffer");
    let path = dir.join(format!("{name}.png"));
    image.save(&path).expect("save screenshot");
    println!("saved {}", path.display());
}
