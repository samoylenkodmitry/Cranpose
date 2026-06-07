//! E2E regression contract for tab-row scroll geometry and scroll-position retention.

mod output_paths;

use cranpose::AppLauncher;
use cranpose_testing::{
    bounds_span, changed_pixel_count_in_region, collect_tab_bounds, detect_tab_axis,
    find_button_exact_in_semantics, root_bounds, TabAxis,
};
use desktop_app::app;
use image::{ImageBuffer, RgbaImage};
use std::path::Path;
use std::time::Duration;

type Rect = (f32, f32, f32, f32);

fn center((x, y, w, h): Rect) -> (f32, f32) {
    (x + w * 0.5, y + h * 0.5)
}

fn find_tab(tabs: &[(String, Rect)], label: &str) -> Option<Rect> {
    tabs.iter()
        .find(|(candidate, _)| candidate == label)
        .map(|(_, bounds)| *bounds)
}

fn visible_in_root(bounds: Rect, root: Rect) -> bool {
    let (x, y, w, h) = bounds;
    let (rx, ry, rw, rh) = root;
    let center_x = x + w * 0.5;
    let center_y = y + h * 0.5;
    center_x >= rx && center_x <= rx + rw && center_y >= ry && center_y <= ry + rh
}

fn require_tab(tabs: &[(String, Rect)], label: &str) -> Rect {
    find_tab(tabs, label).unwrap_or_else(|| panic!("{label} tab not found in {tabs:?}"))
}

fn scroll_tab_row_left(robot: &cranpose::Robot, start: Rect, distance: f32) {
    let (start_x, start_y) = center(start);
    robot.mouse_move(start_x, start_y).expect("move to tab row");
    std::thread::sleep(Duration::from_millis(30));
    robot
        .drag(start_x, start_y, start_x - distance, start_y)
        .expect("drag tab row left");
    std::thread::sleep(Duration::from_millis(180));
    let _ = robot.wait_for_idle();
}

fn wheel_tab_row_left(robot: &cranpose::Robot, start: Rect) {
    let (start_x, start_y) = center(start);
    robot.mouse_move(start_x, start_y).expect("move to tab row");
    std::thread::sleep(Duration::from_millis(30));
    robot
        .mouse_scroll(-420.0, 0.0)
        .expect("horizontal wheel tab row");
    std::thread::sleep(Duration::from_millis(160));
    let _ = robot.wait_for_idle();
}

fn tab_row_y(tabs: &[(String, Rect)]) -> f32 {
    let mut ys = tabs.iter().map(|(_, (_, y, _, _))| *y).collect::<Vec<_>>();
    ys.sort_by(|left, right| left.total_cmp(right));
    ys[ys.len() / 2]
}

fn fail(_robot: &cranpose::Robot, message: &str) -> ! {
    println!("FATAL: {message}");
    std::process::exit(1);
}

fn save_screenshot(path: &Path, screenshot: &cranpose::RobotScreenshot) {
    let Some(image) = ImageBuffer::<image::Rgba<u8>, _>::from_raw(
        screenshot.width,
        screenshot.height,
        screenshot.pixels.clone(),
    ) else {
        panic!("invalid screenshot for {}", path.display());
    };
    let image: RgbaImage = image;
    image
        .save(path)
        .unwrap_or_else(|err| panic!("failed to save {}: {err}", path.display()));
}

fn main() {
    env_logger::init();
    println!("=== Robot Tabbar Regression Contract ===");

    AppLauncher::new()
        .with_title("Robot Tabbar Regression Contract")
        .with_size(900, 700)
        .with_headless(false)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(600));
            let _ = robot.wait_for_idle();

            let labels = app::demo_tab_labels();
            let root = root_bounds(&robot).unwrap_or((0.0, 0.0, 900.0, 700.0));
            let initial_tabs = collect_tab_bounds(&robot, &labels);
            if initial_tabs.len() < 4 {
                fail(&robot, "not enough tab semantics collected");
            }
            let axis = detect_tab_axis(&initial_tabs).unwrap_or(TabAxis::Horizontal);
            if axis != TabAxis::Horizontal {
                fail(&robot, "tabbar is expected to be horizontal in the desktop demo");
            }
            let Some(span) = bounds_span(&initial_tabs) else {
                fail(&robot, "missing tab span");
            };
            if span.2 - span.0 <= root.2 - 40.0 {
                fail(&robot, "tabbar does not overflow, so scroll contract cannot run");
            }

            let initial_y = tab_row_y(&initial_tabs);
            let initial_counter = require_tab(&initial_tabs, "Counter App");
            let initial_counter_x = initial_counter.0;
            println!("initial_y={initial_y:.2} initial_counter_x={initial_counter_x:.2}");

            wheel_tab_row_left(&robot, initial_counter);
            let after_wheel = collect_tab_bounds(&robot, &labels);
            let after_wheel_y = tab_row_y(&after_wheel);
            let wheel_y_delta = after_wheel_y - initial_y;
            println!("after_wheel_y={after_wheel_y:.2} wheel_y_delta={wheel_y_delta:.2}");
            if wheel_y_delta.abs() > 1.0 {
                fail(
                    &robot,
                    &format!("tab row changed Y during wheel scroll: delta={wheel_y_delta:.2}"),
                );
            }

            scroll_tab_row_left(&robot, initial_counter, 260.0);
            let after_drag = collect_tab_bounds(&robot, &labels);
            let after_drag_y = tab_row_y(&after_drag);
            let y_delta = after_drag_y - initial_y;
            let lazy_after_drag = require_tab(&after_drag, "Lazy List");
            println!(
                "after_drag_y={after_drag_y:.2} y_delta={y_delta:.2} lazy_x={:.2}",
                lazy_after_drag.0
            );
            if y_delta.abs() > 1.0 {
                fail(
                    &robot,
                    &format!("tab row changed Y while scrolling: delta={y_delta:.2}"),
                );
            }

            for label in [
                "CompositionLocal Test",
                "Async Runtime",
                "Animations",
                "Web Fetch",
                "Text Input",
            ] {
                if let Some(bounds) = find_tab(&collect_tab_bounds(&robot, &labels), label) {
                    if visible_in_root(bounds, root) {
                        let (x, y) = center(bounds);
                        robot.click(x, y).expect("click visible tab during tab walk");
                        std::thread::sleep(Duration::from_millis(120));
                        let _ = robot.wait_for_idle();
                    }
                }
            }

            for _ in 0..8 {
                let tabs = collect_tab_bounds(&robot, &labels);
                let hacker_news = require_tab(&tabs, "Hacker News");
                if visible_in_root(hacker_news, root) {
                    break;
                }
                let anchor = find_tab(&tabs, "Lazy List").unwrap_or(hacker_news);
                scroll_tab_row_left(&robot, anchor, 220.0);
            }

            let before_click = collect_tab_bounds(&robot, &labels);
            let before_hn = require_tab(&before_click, "Hacker News");
            let before_counter = require_tab(&before_click, "Counter App");
            if !visible_in_root(before_hn, root) {
                fail(
                    &robot,
                    &format!("Hacker News tab is still not visible after tabbar scroll: {before_hn:?}"),
                );
            }
            let tab_strip_region = (0.0, (before_hn.1 - 18.0).max(0.0), root.2, before_hn.3 + 36.0);
            let screenshot_before_click = robot.screenshot().expect("tabbar before click screenshot");
            let before_path = output_paths::diagnostic_path("tabbar_before_hacker_news_click.png");
            save_screenshot(&before_path, &screenshot_before_click);

            let (hn_x, hn_y) = center(before_hn);
            robot.click(hn_x, hn_y).expect("click Hacker News tab");
            std::thread::sleep(Duration::from_millis(220));
            let _ = robot.wait_for_idle();
            let screenshot_after_click = robot.screenshot().expect("tabbar after click screenshot");
            let after_path = output_paths::diagnostic_path("tabbar_after_hacker_news_click.png");
            save_screenshot(&after_path, &screenshot_after_click);

            let after_click = collect_tab_bounds(&robot, &labels);
            let after_hn = require_tab(&after_click, "Hacker News");
            let after_counter = require_tab(&after_click, "Counter App");
            let reset_delta = after_counter.0 - initial_counter_x;
            let hn_jump = after_hn.0 - before_hn.0;
            println!(
                "before_counter_x={:.2} after_counter_x={:.2} reset_delta={reset_delta:.2} before_hn_x={:.2} after_hn_x={:.2} hn_jump={hn_jump:.2}",
                before_counter.0, after_counter.0, before_hn.0, after_hn.0
            );
            let tab_strip_diff = changed_pixel_count_in_region(
                &screenshot_before_click,
                &screenshot_after_click,
                tab_strip_region,
                10,
            );
            println!(
                "tab_strip_visual_diff={tab_strip_diff} region={tab_strip_region:?} before={} after={}",
                before_path.display(),
                after_path.display()
            );
            if after_counter.0 > before_counter.0 + 80.0
                || reset_delta.abs() < 12.0
                || hn_jump.abs() > 12.0
                || tab_strip_diff > 18_000
            {
                fail(
                    &robot,
                    "clicking Hacker News after tabbar scroll reset or jumped the tab-row scroll position",
                );
            }
            if find_button_exact_in_semantics(&robot, "Hacker News").is_none() {
                fail(&robot, "Hacker News tab disappeared from button semantics");
            }

            println!("PASS: tab row keeps Y position and retained scroll offset");
            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}
