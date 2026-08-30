mod markdown_fixture_client;
mod output_paths;
mod perf_contract;
mod text_showcase_external_helpers;

use std::{path::Path, sync::Arc, time::Duration};

use cranpose::AppLauncher;
use cranpose_core::CompositionLocalProvider;
use cranpose_services::{local_http_client, HttpClientRef};
use cranpose_testing::{
    find_button_exact_in_semantics, find_in_semantics, find_text, print_semantics_with_bounds,
};
use desktop_app::app;
use image::RgbaImage;
use markdown_fixture_client::MarkdownFixtureClient;
use text_showcase_external_helpers::{
    capture_x11_window, click_x11_button, click_x11_button_repeated, find_window_id,
    focus_x11_window, mouse_down_x11_button, mouse_up_x11_button, move_x11_mouse_in_window,
};

const WINDOW_WIDTH: u32 = 1080;
const WINDOW_HEIGHT: u32 = 820;
const WINDOW_TITLE: &str = "Robot Markdown Full Demo Code Block Visual Contract";
const VIEWPORT_TAG: &str = "MarkdownListViewport";
const SCROLL_DELTA_Y: f32 = -220.0;
const MAX_SCROLL_ATTEMPTS: usize = 80;
const SCAN_SCROLL_STEPS: usize = 52;
const SCAN_SCROLL_DELTA_Y: f32 = -560.0;
const REAL_WHEEL_STEPS: usize = 18;
const REAL_WHEEL_BURST_STEPS: usize = 8;
const REAL_WHEEL_BURST_CLICKS: usize = 10;
const ACTIVE_DRAG_GESTURES: usize = 2;
const ACTIVE_DRAG_STEPS: usize = 10;
const MIN_CODE_INK_PIXELS: usize = 220;
const MIN_CODE_INK_ROWS: usize = 10;
const MIN_CODE_SAMPLED_ROWS: usize = 64;
const MIN_SCROLL_CODE_SAMPLED_ROWS: usize = 96;
const MIN_VIEWPORT_INK_PIXELS: usize = 900;
const MIN_VIEWPORT_INK_ROWS: usize = 18;
const MIN_LOWER_VIEWPORT_INK_PIXELS: usize = 320;
const MIN_LOWER_VIEWPORT_INK_ROWS: usize = 8;
const MIN_SCROLL_WORK_FPS: f32 = 120.0;
const MAX_SCROLL_WORK_P95_MS: f32 = 8.33;
const MIN_TEXT_ROW_INK_PIXELS: usize = 4;
const MIN_TEXT_CONTINUITY_ROW_INK_PIXELS: usize = 1;
const MIN_VISIBLE_TEXT_NODE_INK_PIXELS: usize = 64;
const MIN_VISIBLE_TEXT_NODE_INK_ROWS: usize = 6;
const MAX_VISIBLE_TEXT_BOTTOM_BLANK_PX: usize = 64;
const MAX_MARKDOWN_SECTION_GAP_PX: f32 = 96.0;
const MAX_MARKDOWN_TEXT_GAP_PX: f32 = 112.0;
const MAX_VIEWPORT_BLANK_ROW_RATIO: f32 = 0.17;
const MAX_VIEWPORT_ABSOLUTE_BLANK_ROW_RUN: usize = 80;
const CONTENT_BAND_TOP_FRACTION: f32 = 0.18;
const CONTENT_BAND_BOTTOM_FRACTION: f32 = 0.94;
const LOWER_CONTENT_BAND_TOP_FRACTION: f32 = 0.55;
const LOWER_CONTENT_BAND_BOTTOM_FRACTION: f32 = 0.94;
const MIN_FETCH_ROW_VIEWPORT_GAP_PX: f32 = 8.0;

#[derive(Clone, Copy, Debug)]
struct InkMetrics {
    ink_pixels: usize,
    ink_rows: usize,
    blank_row_run: usize,
    sampled_rows: usize,
}

type Bounds = (f32, f32, f32, f32);
type VisibleTextNode = (Bounds, String);
type MergedTextSpan = (f32, f32, Bounds, String);

fn main() {
    env_logger::init();
    println!("=== Robot Markdown Full Demo Code Block Visual Contract ===");

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(false)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(800));
            println!("markdown_full_demo: waiting for initial idle");
            let _ = robot.wait_for_idle();
            println!("markdown_full_demo: initial idle reached");

            for label in [
                "Images",
                "Text",
                "Winamp",
                "XKCD",
                "Shaders",
                "Shader Rect",
                "Markdown",
            ] {
                println!("markdown_full_demo: clicking tab {label}");
                click_tab(&robot, label);
                println!("markdown_full_demo: clicked tab {label}");
            }

            println!("markdown_full_demo: clicking Fetch");
            click_button(&robot, "Fetch");
            println!("markdown_full_demo: clicked Fetch");
            let viewport = wait_for_viewport(&robot, 10_000);
            assert_markdown_viewport_is_below_fetch_row(&robot, viewport);
            let center_x = viewport.0 + viewport.2 * 0.5;
            let center_y = viewport.1 + viewport.3 * 0.5;
            robot
                .move_to(center_x, center_y)
                .expect("move to Markdown viewport");
            robot
                .wait_for_present_frame()
                .expect("present fetched Markdown");

            assert_code_target_paints(&robot, viewport, "fun earliestFinishTime", 0);
            let real_wheel_stats = scan_full_demo_markdown_real_wheel(&robot, viewport);
            scan_full_demo_markdown_real_wheel_bursts(&robot, viewport);
            scan_full_demo_markdown_active_drag(&robot, viewport);
            scan_full_demo_markdown_scroll(&robot, viewport);
            assert_real_wheel_work_performance(real_wheel_stats);

            robot.exit().expect("exit");
        })
        .run({
            let client: HttpClientRef = Arc::new(MarkdownFixtureClient::default_leetcode_daily());
            move || {
                let local = local_http_client();
                let client = client.clone();
                CompositionLocalProvider(vec![local.provides(client)], || {
                    app::combined_app();
                });
            }
        });
}

fn click_tab(robot: &cranpose::Robot, label: &str) {
    let Some((x, y, w, h)) = wait_for_button(robot, label, 60, Duration::from_millis(100)) else {
        panic!("tab {label:?} not found");
    };
    robot
        .click(x + w * 0.5, y + h * 0.5)
        .unwrap_or_else(|err| panic!("click tab {label:?}: {err}"));
    std::thread::sleep(Duration::from_millis(180));
    if label == "Shader Rect" {
        robot
            .wait_for_present_frame()
            .unwrap_or_else(|err| panic!("animated tab {label:?} did not present: {err}"));
    } else {
        let _ = robot.wait_for_idle();
    }
}

fn wait_for_button(
    robot: &cranpose::Robot,
    label: &str,
    attempts: usize,
    delay: Duration,
) -> Option<(f32, f32, f32, f32)> {
    for _ in 0..attempts {
        if let Some(bounds) = find_button_exact_in_semantics(robot, label) {
            return Some(bounds);
        }
        std::thread::sleep(delay);
    }
    None
}

fn click_button(robot: &cranpose::Robot, label: &str) {
    let Some((x, y, w, h)) = find_button_exact_in_semantics(robot, label) else {
        let window_id = find_window_id(WINDOW_TITLE);
        let capture_path =
            output_paths::diagnostic_path(&format!("markdown_full_demo_missing_{label}.png"));
        let _ = capture_x11_window(&window_id, &capture_path);
        println!(
            "button {label:?} missing; current window capture={}",
            capture_path.display()
        );
        match robot.get_semantics() {
            Ok(semantics) => print_semantics_with_bounds(&semantics, 0),
            Err(err) => println!("failed to get semantics after missing button {label:?}: {err}"),
        }
        panic!("button {label:?} not found");
    };
    robot
        .click(x + w * 0.5, y + h * 0.5)
        .unwrap_or_else(|err| panic!("click button {label:?}: {err}"));
    std::thread::sleep(Duration::from_millis(180));
    let _ = robot.wait_for_idle();
}

fn wait_for_viewport(robot: &cranpose::Robot, timeout_ms: u64) -> (f32, f32, f32, f32) {
    let attempts = (timeout_ms / 100).max(1);
    for _ in 0..attempts {
        if let Some(bounds) = find_in_semantics(robot, |elem| find_text(elem, VIEWPORT_TAG)) {
            return bounds;
        }
        std::thread::sleep(Duration::from_millis(100));
        let _ = robot.wait_for_idle();
    }
    panic!("Markdown viewport semantics not found");
}

fn assert_markdown_viewport_is_below_fetch_row(
    robot: &cranpose::Robot,
    viewport: (f32, f32, f32, f32),
) {
    let Some((fetch_x, fetch_y, fetch_w, fetch_h)) = find_button_exact_in_semantics(robot, "Fetch")
    else {
        panic!("Fetch button missing while checking Markdown viewport placement");
    };
    let fetch_bottom = fetch_y + fetch_h;
    let gap = viewport.1 - fetch_bottom;
    println!(
        "markdown_viewport_fetch_gap: fetch=({fetch_x:.1},{fetch_y:.1},{fetch_w:.1},{fetch_h:.1}) viewport=({:.1},{:.1},{:.1},{:.1}) gap={gap:.1}",
        viewport.0, viewport.1, viewport.2, viewport.3
    );
    assert!(
        gap >= MIN_FETCH_ROW_VIEWPORT_GAP_PX,
        "Markdown lazy viewport overlaps the Fetch row: gap={gap:.1}, required>={MIN_FETCH_ROW_VIEWPORT_GAP_PX:.1}, fetch=({fetch_x:.1},{fetch_y:.1},{fetch_w:.1},{fetch_h:.1}), viewport=({:.1},{:.1},{:.1},{:.1})",
        viewport.0,
        viewport.1,
        viewport.2,
        viewport.3
    );
}

fn assert_code_target_paints(
    robot: &cranpose::Robot,
    viewport: (f32, f32, f32, f32),
    target: &str,
    target_index: usize,
) {
    let bounds = scroll_text_into_view(robot, target, viewport)
        .unwrap_or_else(|| panic!("target code text {target:?} was not found after scroll"));
    robot
        .wait_for_present_frame()
        .expect("present target code text");

    let window_id = find_window_id(WINDOW_TITLE);
    let capture_path = output_paths::diagnostic_path(&format!(
        "markdown_full_demo_code_target_{target_index}.png"
    ));
    let image = capture_x11_window(&window_id, &capture_path);
    let metrics = code_ink_metrics(&image, viewport, bounds);
    println!(
        "code_target target={target:?} bounds=({:.1},{:.1},{:.1},{:.1}) metrics={metrics:?} capture={}",
        bounds.0,
        bounds.1,
        bounds.2,
        bounds.3,
        capture_path.display()
    );
    assert_code_metrics(metrics, &capture_path, target);
}

fn scan_full_demo_markdown_scroll(robot: &cranpose::Robot, viewport: (f32, f32, f32, f32)) {
    let window_id = find_window_id(WINDOW_TITLE);

    for step in 0..SCAN_SCROLL_STEPS {
        robot
            .mouse_scroll_and_wait_for_frame(0.0, SCAN_SCROLL_DELTA_Y)
            .expect("scroll full-demo Markdown viewport");

        let capture_path =
            output_paths::diagnostic_path(&format!("markdown_full_demo_scroll_scan_{step:02}.png"));
        let image = capture_x11_window(&window_id, &capture_path);
        let viewport_metrics = viewport_ink_metrics(
            &image,
            viewport,
            CONTENT_BAND_TOP_FRACTION,
            CONTENT_BAND_BOTTOM_FRACTION,
        );
        let semantics = markdown_semantics(robot);
        println!(
            "scroll_scan step={step} viewport_metrics={viewport_metrics:?} capture={}",
            capture_path.display()
        );

        let code_bounds = visible_code_text_bounds(&semantics, viewport);
        for (code_index, bounds) in code_bounds.iter().copied().enumerate() {
            let code_metrics = code_ink_metrics(&image, viewport, bounds);
            println!(
                "scroll_scan step={step} code_index={code_index} bounds=({:.1},{:.1},{:.1},{:.1}) metrics={code_metrics:?}",
                bounds.0, bounds.1, bounds.2, bounds.3
            );
            assert_code_metrics_with_min_rows(
                code_metrics,
                &capture_path,
                "visible scroll-frame code block",
                MIN_SCROLL_CODE_SAMPLED_ROWS,
            );
        }
        if viewport_blank_artifact(viewport_metrics) {
            log_visible_text_nodes(&semantics, viewport, step);
        }
        assert_visible_text_nodes_do_not_leave_blank_tails(
            &semantics,
            &image,
            viewport,
            &capture_path,
        );
        assert_visible_text_nodes_do_not_have_large_blank_gaps(
            &semantics,
            &image,
            viewport,
            &capture_path,
        );
        assert_markdown_section_gaps_are_continuous(&semantics, viewport, &capture_path);
        assert_lower_viewport_band_paints(&image, viewport, &capture_path, step);
        assert_viewport_metrics(viewport_metrics, &capture_path, step);
    }
}

fn scan_full_demo_markdown_real_wheel(
    robot: &cranpose::Robot,
    viewport: (f32, f32, f32, f32),
) -> cranpose::FpsStats {
    let window_id = find_window_id(WINDOW_TITLE);
    let center_x = viewport.0 + viewport.2 * 0.5;
    let center_y = viewport.1 + viewport.3 * 0.5;

    focus_x11_window(&window_id);
    move_x11_mouse_in_window(&window_id, center_x, center_y);
    std::thread::sleep(Duration::from_millis(80));
    robot
        .reset_fps_stats()
        .expect("reset real-wheel markdown FPS");
    let initial_semantics = markdown_semantics(robot);
    let initial_signature = visible_text_signature(&initial_semantics, viewport);
    let mut moved = false;

    for step in 0..REAL_WHEEL_STEPS {
        click_x11_button("5");
        robot
            .wait_for_present_frame()
            .expect("present real-wheel Markdown scroll");
        let capture_path = output_paths::diagnostic_path(&format!(
            "markdown_full_demo_real_wheel_scan_{step:02}.png"
        ));
        let image = capture_x11_window(&window_id, &capture_path);
        let viewport_metrics = viewport_ink_metrics(
            &image,
            viewport,
            CONTENT_BAND_TOP_FRACTION,
            CONTENT_BAND_BOTTOM_FRACTION,
        );
        let semantics = markdown_semantics(robot);
        println!(
            "real_wheel_scan step={step} viewport_metrics={viewport_metrics:?} capture={}",
            capture_path.display()
        );
        if viewport_blank_artifact(viewport_metrics) {
            log_visible_text_nodes(&semantics, viewport, step);
        }
        moved |= visible_text_signature(&semantics, viewport) != initial_signature;
        assert_visible_text_nodes_do_not_leave_blank_tails(
            &semantics,
            &image,
            viewport,
            &capture_path,
        );
        assert_visible_text_nodes_do_not_have_large_blank_gaps(
            &semantics,
            &image,
            viewport,
            &capture_path,
        );
        assert_markdown_section_gaps_are_continuous(&semantics, viewport, &capture_path);
        assert_lower_viewport_band_paints(&image, viewport, &capture_path, step);
        assert_viewport_metrics(viewport_metrics, &capture_path, step);
    }
    assert!(
        moved,
        "real X11 wheel input did not change the visible Markdown content; the robot was not exercising mouse-wheel scroll"
    );

    let stats = robot
        .fps_stats()
        .expect("read real-wheel markdown FPS stats");
    println!(
        "real_wheel_fps_summary: cadence_fps={:.1} work_fps={:.1} p95_ms={:.2} work_p95_ms={:.2} max_ms={:.2} work_max_ms={:.2} missed_120hz={} work_missed_120hz={} stalls_50ms={} work_stalls_50ms={} recompositions={} total_frames={}",
        stats.fps,
        stats.work_fps,
        stats.p95_ms,
        stats.work_p95_ms,
        stats.max_ms,
        stats.work_max_ms,
        stats.missed_120hz_budget,
        stats.work_missed_120hz_budget,
        stats.stalled_50ms_frames,
        stats.work_stalled_50ms_frames,
        stats.recompositions,
        stats.frame_count
    );
    stats
}

fn scan_full_demo_markdown_real_wheel_bursts(
    robot: &cranpose::Robot,
    viewport: (f32, f32, f32, f32),
) {
    let window_id = find_window_id(WINDOW_TITLE);
    let center_x = viewport.0 + viewport.2 * 0.5;
    let center_y = viewport.1 + viewport.3 * 0.5;

    focus_x11_window(&window_id);
    move_x11_mouse_in_window(&window_id, center_x, center_y);
    std::thread::sleep(Duration::from_millis(80));

    for step in 0..REAL_WHEEL_BURST_STEPS {
        click_x11_button_repeated("5", REAL_WHEEL_BURST_CLICKS);
        robot
            .wait_for_present_frame()
            .expect("present burst-wheel Markdown scroll");
        let capture_path = output_paths::diagnostic_path(&format!(
            "markdown_full_demo_real_wheel_burst_scan_{step:02}.png"
        ));
        let image = capture_x11_window(&window_id, &capture_path);
        let viewport_metrics = viewport_ink_metrics(
            &image,
            viewport,
            CONTENT_BAND_TOP_FRACTION,
            CONTENT_BAND_BOTTOM_FRACTION,
        );
        let semantics = markdown_semantics(robot);
        println!(
            "real_wheel_burst_scan step={step} clicks={REAL_WHEEL_BURST_CLICKS} viewport_metrics={viewport_metrics:?} capture={}",
            capture_path.display()
        );
        if viewport_blank_artifact(viewport_metrics) {
            log_visible_text_nodes(&semantics, viewport, step);
        }
        assert_visible_text_nodes_do_not_leave_blank_tails(
            &semantics,
            &image,
            viewport,
            &capture_path,
        );
        assert_visible_text_nodes_do_not_have_large_blank_gaps(
            &semantics,
            &image,
            viewport,
            &capture_path,
        );
        assert_markdown_section_gaps_are_continuous(&semantics, viewport, &capture_path);
        assert_lower_viewport_band_paints(&image, viewport, &capture_path, step);
        assert_viewport_metrics(viewport_metrics, &capture_path, step);
    }
}

fn scan_full_demo_markdown_active_drag(robot: &cranpose::Robot, viewport: (f32, f32, f32, f32)) {
    let window_id = find_window_id(WINDOW_TITLE);
    let center_x = viewport.0 + viewport.2 * 0.5;
    let drag_start_y = viewport.1 + viewport.3 * 0.82;
    let drag_end_y = viewport.1 + viewport.3 * 0.22;

    focus_x11_window(&window_id);
    move_x11_mouse_in_window(&window_id, center_x, drag_start_y);
    std::thread::sleep(Duration::from_millis(80));

    for gesture in 0..ACTIVE_DRAG_GESTURES {
        move_x11_mouse_in_window(&window_id, center_x, drag_start_y);
        std::thread::sleep(Duration::from_millis(30));
        mouse_down_x11_button("1");

        for step in 1..=ACTIVE_DRAG_STEPS {
            let t = step as f32 / ACTIVE_DRAG_STEPS as f32;
            let y = drag_start_y + (drag_end_y - drag_start_y) * t;
            move_x11_mouse_in_window(&window_id, center_x, y);
            robot
                .wait_for_present_frame()
                .expect("present active-drag Markdown scroll");

            let capture_path = output_paths::diagnostic_path(&format!(
                "markdown_full_demo_active_drag_g{gesture:02}_step_{step:02}.png"
            ));
            let image = capture_x11_window(&window_id, &capture_path);
            let viewport_metrics = viewport_ink_metrics(
                &image,
                viewport,
                CONTENT_BAND_TOP_FRACTION,
                CONTENT_BAND_BOTTOM_FRACTION,
            );
            let semantics = markdown_semantics(robot);
            println!(
                "active_drag_scan gesture={gesture} step={step} viewport_metrics={viewport_metrics:?} capture={}",
                capture_path.display()
            );
            if viewport_blank_artifact(viewport_metrics) {
                log_visible_text_nodes(&semantics, viewport, step);
            }
            assert_visible_text_nodes_do_not_leave_blank_tails(
                &semantics,
                &image,
                viewport,
                &capture_path,
            );
            assert_visible_text_nodes_do_not_have_large_blank_gaps(
                &semantics,
                &image,
                viewport,
                &capture_path,
            );
            assert_markdown_section_gaps_are_continuous(&semantics, viewport, &capture_path);
            assert_lower_viewport_band_paints(&image, viewport, &capture_path, step);
            assert_viewport_metrics(viewport_metrics, &capture_path, step);
        }

        mouse_up_x11_button("1");
        std::thread::sleep(Duration::from_millis(80));
    }
}

fn scroll_text_into_view(
    robot: &cranpose::Robot,
    target: &str,
    viewport: (f32, f32, f32, f32),
) -> Option<(f32, f32, f32, f32)> {
    for attempt in 0..MAX_SCROLL_ATTEMPTS {
        if let Some(bounds) = find_text_in_visible_viewport(robot, target, viewport) {
            println!("target {target:?} visible after {attempt} scroll attempts");
            return Some(bounds);
        }
        robot
            .mouse_scroll_and_wait_for_frame(0.0, SCROLL_DELTA_Y)
            .expect("scroll Markdown viewport");
    }
    None
}

fn markdown_semantics(robot: &cranpose::Robot) -> Vec<cranpose::SemanticElement> {
    robot
        .get_semantics()
        .unwrap_or_else(|err| panic!("failed to get Markdown semantics: {err}"))
}

fn visible_code_text_bounds(
    semantics: &[cranpose::SemanticElement],
    viewport: (f32, f32, f32, f32),
) -> Vec<(f32, f32, f32, f32)> {
    let mut out = Vec::new();
    let content_band = content_band_viewport(viewport);
    for root in semantics {
        collect_visible_code_text_bounds(root, content_band, &mut out);
    }
    out
}

fn log_visible_text_nodes(
    semantics: &[cranpose::SemanticElement],
    viewport: (f32, f32, f32, f32),
    step: usize,
) {
    let mut out = Vec::new();
    let content_band = content_band_viewport(viewport);
    for root in semantics {
        collect_visible_text_debug(root, content_band, &mut out);
    }
    for (index, (bounds, text)) in out.into_iter().take(16).enumerate() {
        let snippet = text.replace('\n', "\\n");
        let snippet: String = snippet.chars().take(140).collect();
        println!(
            "scroll_scan step={step} visible_text_index={index} bounds=({:.1},{:.1},{:.1},{:.1}) text={snippet:?}",
            bounds.0, bounds.1, bounds.2, bounds.3
        );
    }
}

fn visible_text_signature(
    semantics: &[cranpose::SemanticElement],
    viewport: (f32, f32, f32, f32),
) -> Vec<String> {
    let mut out = Vec::new();
    let content_band = content_band_viewport(viewport);
    for root in semantics {
        collect_visible_text_debug(root, content_band, &mut out);
    }
    out.into_iter()
        .take(8)
        .map(|(bounds, text)| {
            let snippet: String = text.chars().take(80).collect();
            format!("{:.0}:{:.0}:{snippet}", bounds.1, bounds.3)
        })
        .collect()
}

fn assert_visible_text_nodes_do_not_leave_blank_tails(
    semantics: &[cranpose::SemanticElement],
    image: &RgbaImage,
    viewport: (f32, f32, f32, f32),
    capture_path: &Path,
) {
    let mut visible_texts = Vec::new();
    let content_band = content_band_viewport(viewport);
    for root in semantics {
        collect_visible_text_debug(root, content_band, &mut visible_texts);
    }

    for (bounds, text) in visible_texts {
        let fully_visible =
            bounds.1 >= content_band.1 && bounds.1 + bounds.3 <= content_band.1 + content_band.3;
        if !fully_visible || bounds.3 < 80.0 || text.trim().len() < 24 {
            continue;
        }

        let metrics = text_bounds_ink_metrics(image, viewport, bounds);
        if metrics.sampled_rows < 80 {
            continue;
        }

        assert!(
            metrics.ink_pixels >= MIN_VISIBLE_TEXT_NODE_INK_PIXELS
                && metrics.ink_rows >= MIN_VISIBLE_TEXT_NODE_INK_ROWS,
            "visible Markdown text node has semantic bounds but did not paint enough text: bounds=({:.1},{:.1},{:.1},{:.1}) metrics={metrics:?} text={:?} capture={}",
            bounds.0,
            bounds.1,
            bounds.2,
            bounds.3,
            text.chars().take(120).collect::<String>(),
            capture_path.display()
        );
        assert!(
            metrics.blank_row_run <= MAX_VISIBLE_TEXT_BOTTOM_BLANK_PX,
            "visible Markdown text node reserves a blank tail below painted text: bounds=({:.1},{:.1},{:.1},{:.1}) metrics={metrics:?} text={:?} capture={}",
            bounds.0,
            bounds.1,
            bounds.2,
            bounds.3,
            text.chars().take(120).collect::<String>(),
            capture_path.display()
        );
    }
}

fn assert_visible_text_nodes_do_not_have_large_blank_gaps(
    semantics: &[cranpose::SemanticElement],
    image: &RgbaImage,
    viewport: (f32, f32, f32, f32),
    capture_path: &Path,
) {
    let mut visible_texts = Vec::new();
    let content_band = content_band_viewport(viewport);
    for root in semantics {
        collect_visible_text_debug(root, content_band, &mut visible_texts);
    }

    let mut spans = visible_texts
        .into_iter()
        .filter_map(|(bounds, text)| {
            let text = text.trim();
            if text.is_empty() || bounds.3 < 10.0 {
                return None;
            }
            let top = bounds.1.max(content_band.1);
            let bottom = (bounds.1 + bounds.3).min(content_band.1 + content_band.3);
            (bottom - top >= 10.0).then(|| {
                (
                    top,
                    bottom,
                    bounds,
                    text.chars().take(96).collect::<String>(),
                )
            })
        })
        .collect::<Vec<_>>();
    spans.sort_by(|lhs, rhs| {
        lhs.0
            .total_cmp(&rhs.0)
            .then_with(|| lhs.1.total_cmp(&rhs.1))
    });

    let mut merged: Vec<MergedTextSpan> = Vec::new();
    for span in spans {
        if let Some(last) = merged.last_mut() {
            if span.0 <= last.1 + 8.0 {
                if span.1 > last.1 {
                    last.1 = span.1;
                    last.2 = span.2;
                    last.3 = span.3;
                }
                continue;
            }
        }
        merged.push(span);
    }

    for pair in merged.windows(2) {
        let previous = &pair[0];
        let next = &pair[1];
        let gap = next.0 - previous.1;
        if gap <= MAX_MARKDOWN_TEXT_GAP_PX {
            continue;
        }
        let metrics = vertical_gap_ink_metrics(image, viewport, previous.1, next.0);
        assert!(
            !viewport_blank_artifact(metrics),
            "Markdown left a large blank gap between visible text blocks: gap={gap:.1}, metrics={metrics:?}, previous_bounds=({:.1},{:.1},{:.1},{:.1}), next_bounds=({:.1},{:.1},{:.1},{:.1}), previous_text={:?}, next_text={:?}, capture={}",
            previous.2 .0,
            previous.2 .1,
            previous.2 .2,
            previous.2 .3,
            next.2 .0,
            next.2 .1,
            next.2 .2,
            next.2 .3,
            previous.3,
            next.3,
            capture_path.display()
        );
    }
}

fn assert_markdown_section_gaps_are_continuous(
    semantics: &[cranpose::SemanticElement],
    viewport: (f32, f32, f32, f32),
    capture_path: &Path,
) {
    let mut visible_texts = Vec::new();
    let content_band = content_band_viewport(viewport);
    for root in semantics {
        collect_visible_text_debug(root, content_band, &mut visible_texts);
    }

    let Some(space_complexity_bounds) = visible_texts
        .iter()
        .filter(|(_, text)| text.contains("Space complexity"))
        .map(|(bounds, _)| *bounds)
        .max_by(|a, b| a.1.total_cmp(&b.1))
    else {
        return;
    };

    let Some(code_bounds) = visible_texts
        .iter()
        .filter(|(_, text)| text.trim() == "Code")
        .map(|(bounds, _)| *bounds)
        .min_by(|a, b| a.1.total_cmp(&b.1))
    else {
        return;
    };

    let gap = code_bounds.1 - (space_complexity_bounds.1 + space_complexity_bounds.3);
    assert!(
        gap <= MAX_MARKDOWN_SECTION_GAP_PX,
        "Markdown left a large blank gap between Space complexity and Code: gap={gap:.1}, space_bounds=({:.1},{:.1},{:.1},{:.1}), code_bounds=({:.1},{:.1},{:.1},{:.1}), capture={}",
        space_complexity_bounds.0,
        space_complexity_bounds.1,
        space_complexity_bounds.2,
        space_complexity_bounds.3,
        code_bounds.0,
        code_bounds.1,
        code_bounds.2,
        code_bounds.3,
        capture_path.display()
    );
}

fn collect_visible_text_debug(
    elem: &cranpose::SemanticElement,
    viewport: (f32, f32, f32, f32),
    out: &mut Vec<VisibleTextNode>,
) {
    if let Some(text) = elem.text.as_deref() {
        let bounds = (
            elem.bounds.x,
            elem.bounds.y,
            elem.bounds.width,
            elem.bounds.height,
        );
        if intersects_viewport(bounds, viewport) {
            out.push((bounds, text.to_string()));
        }
    }

    for child in &elem.children {
        collect_visible_text_debug(child, viewport, out);
    }
}

fn collect_visible_code_text_bounds(
    elem: &cranpose::SemanticElement,
    viewport: (f32, f32, f32, f32),
    out: &mut Vec<(f32, f32, f32, f32)>,
) {
    if let Some(text) = elem.text.as_deref() {
        if is_code_text(text) {
            let bounds = (
                elem.bounds.x,
                elem.bounds.y,
                elem.bounds.width,
                elem.bounds.height,
            );
            if intersects_viewport(bounds, viewport) {
                out.push(bounds);
            }
        }
    }

    for child in &elem.children {
        collect_visible_code_text_bounds(child, viewport, out);
    }
}

fn content_band_viewport(viewport: (f32, f32, f32, f32)) -> (f32, f32, f32, f32) {
    let top = viewport.1 + viewport.3 * CONTENT_BAND_TOP_FRACTION;
    let bottom = viewport.1 + viewport.3 * CONTENT_BAND_BOTTOM_FRACTION;
    (viewport.0, top, viewport.2, bottom - top)
}

fn is_code_text(text: &str) -> bool {
    text.contains("fun ")
        || text.contains("pub fn ")
        || text
            .lines()
            .filter(|line| line.trim_start().starts_with("//"))
            .count()
            >= 3
}

fn intersects_viewport(bounds: (f32, f32, f32, f32), viewport: (f32, f32, f32, f32)) -> bool {
    let left = bounds.0.max(viewport.0);
    let right = (bounds.0 + bounds.2).min(viewport.0 + viewport.2);
    let top = bounds.1.max(viewport.1);
    let bottom = (bounds.1 + bounds.3).min(viewport.1 + viewport.3);
    right > left && bottom - top >= 16.0
}

fn find_text_in_visible_viewport(
    robot: &cranpose::Robot,
    target: &str,
    viewport: (f32, f32, f32, f32),
) -> Option<(f32, f32, f32, f32)> {
    let bounds = cranpose_testing::find_text_in_semantics(robot, target)?;
    let target_center_y = bounds.1 + bounds.3 * 0.5;
    let viewport_top = viewport.1 + viewport.3 * 0.08;
    let viewport_bottom = viewport.1 + viewport.3 * 0.92;
    (target_center_y >= viewport_top && target_center_y <= viewport_bottom).then_some(bounds)
}

fn viewport_ink_metrics(
    image: &RgbaImage,
    viewport: (f32, f32, f32, f32),
    top_fraction: f32,
    bottom_fraction: f32,
) -> InkMetrics {
    let scale_x = image.width() as f32 / WINDOW_WIDTH as f32;
    let scale_y = image.height() as f32 / WINDOW_HEIGHT as f32;
    let left = ((viewport.0 + 24.0) * scale_x).floor().max(0.0) as u32;
    let right = ((viewport.0 + viewport.2 - 64.0) * scale_x)
        .ceil()
        .min(image.width() as f32) as u32;
    let top = ((viewport.1 + viewport.3 * top_fraction) * scale_y)
        .floor()
        .max(0.0) as u32;
    let bottom = ((viewport.1 + viewport.3 * bottom_fraction) * scale_y)
        .ceil()
        .min(image.height() as f32) as u32;

    ink_metrics_in_pixel_rect(image, left, top, right, bottom)
}

fn code_ink_metrics(
    image: &RgbaImage,
    viewport: (f32, f32, f32, f32),
    text_bounds: (f32, f32, f32, f32),
) -> InkMetrics {
    let scale_x = image.width() as f32 / WINDOW_WIDTH as f32;
    let scale_y = image.height() as f32 / WINDOW_HEIGHT as f32;

    let content_band = content_band_viewport(viewport);
    let left_logical = text_bounds.0.max(viewport.0 + 18.0);
    let right_logical = (text_bounds.0 + text_bounds.2).min(viewport.0 + viewport.2 - 60.0);
    let top_logical = text_bounds.1.max(content_band.1 + 4.0);
    let bottom_logical = (text_bounds.1 + text_bounds.3)
        .min(content_band.1 + content_band.3 - 4.0)
        .min(top_logical + 280.0);

    let left = (left_logical * scale_x).floor().max(0.0) as u32;
    let right = (right_logical * scale_x).ceil().min(image.width() as f32) as u32;
    let top = (top_logical * scale_y).floor().max(0.0) as u32;
    let bottom = (bottom_logical * scale_y).ceil().min(image.height() as f32) as u32;

    ink_metrics_in_pixel_rect(image, left, top, right, bottom)
}

fn text_bounds_ink_metrics(
    image: &RgbaImage,
    viewport: (f32, f32, f32, f32),
    text_bounds: (f32, f32, f32, f32),
) -> InkMetrics {
    let scale_x = image.width() as f32 / WINDOW_WIDTH as f32;
    let scale_y = image.height() as f32 / WINDOW_HEIGHT as f32;

    let left = (text_bounds.0.max(viewport.0 + 24.0) * scale_x)
        .floor()
        .max(0.0) as u32;
    let right = ((text_bounds.0 + text_bounds.2).min(viewport.0 + viewport.2 - 64.0) * scale_x)
        .ceil()
        .min(image.width() as f32) as u32;
    let top = (text_bounds.1 * scale_y).floor().max(0.0) as u32;
    let bottom = ((text_bounds.1 + text_bounds.3) * scale_y)
        .ceil()
        .min(image.height() as f32) as u32;

    ink_metrics_in_text_pixel_rect(image, left, top, right, bottom)
}

fn vertical_gap_ink_metrics(
    image: &RgbaImage,
    viewport: (f32, f32, f32, f32),
    top_logical: f32,
    bottom_logical: f32,
) -> InkMetrics {
    let scale_x = image.width() as f32 / WINDOW_WIDTH as f32;
    let scale_y = image.height() as f32 / WINDOW_HEIGHT as f32;

    let left = ((viewport.0 + 24.0) * scale_x).floor().max(0.0) as u32;
    let right = ((viewport.0 + viewport.2 - 64.0) * scale_x)
        .ceil()
        .min(image.width() as f32) as u32;
    let top = (top_logical * scale_y).floor().max(0.0) as u32;
    let bottom = (bottom_logical * scale_y).ceil().min(image.height() as f32) as u32;

    ink_metrics_in_text_pixel_rect(image, left, top, right, bottom)
}

fn ink_metrics_in_pixel_rect(
    image: &RgbaImage,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
) -> InkMetrics {
    let mut ink_pixels = 0usize;
    let mut ink_rows = 0usize;
    let mut current_blank_run = 0usize;
    let mut blank_row_run = 0usize;
    let sampled_rows = bottom.saturating_sub(top) as usize;

    for y in top..bottom {
        let mut row_ink = 0usize;
        for x in left..right {
            if is_code_ink(image.get_pixel(x, y).0) {
                row_ink += 1;
            }
        }
        ink_pixels += row_ink;
        if row_ink >= 2 {
            ink_rows += 1;
            current_blank_run = 0;
        } else {
            current_blank_run += 1;
            blank_row_run = blank_row_run.max(current_blank_run);
        }
    }

    InkMetrics {
        ink_pixels,
        ink_rows,
        blank_row_run,
        sampled_rows,
    }
}

fn ink_metrics_in_text_pixel_rect(
    image: &RgbaImage,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
) -> InkMetrics {
    let mut ink_pixels = 0usize;
    let mut ink_rows = 0usize;
    let mut current_blank_run = 0usize;
    let mut blank_row_run = 0usize;
    let sampled_rows = bottom.saturating_sub(top) as usize;

    for y in top..bottom {
        let mut row_ink = 0usize;
        for x in left..right {
            if is_code_ink(image.get_pixel(x, y).0) {
                row_ink += 1;
            }
        }
        ink_pixels += row_ink;
        if row_ink >= MIN_TEXT_ROW_INK_PIXELS {
            ink_rows += 1;
        }
        if row_ink >= MIN_TEXT_CONTINUITY_ROW_INK_PIXELS {
            current_blank_run = 0;
        } else {
            current_blank_run += 1;
            blank_row_run = blank_row_run.max(current_blank_run);
        }
    }

    InkMetrics {
        ink_pixels,
        ink_rows,
        blank_row_run,
        sampled_rows,
    }
}

fn is_code_ink(pixel: [u8; 4]) -> bool {
    let [r, g, b, a] = pixel;
    if a < 180 {
        return false;
    }
    let bright_text = r > 150 && g > 150 && b > 160;
    let link_blue = b > 145 && g > 110 && r < 180;
    let math_yellow = r > 165 && g > 160 && b < 110;
    bright_text || link_blue || math_yellow
}

fn assert_viewport_metrics(metrics: InkMetrics, capture_path: &Path, step: usize) {
    let max_blank_run = max_viewport_blank_run(metrics);
    assert!(
        metrics.ink_pixels >= MIN_VIEWPORT_INK_PIXELS
            && metrics.ink_rows >= MIN_VIEWPORT_INK_ROWS
            && metrics.blank_row_run <= max_blank_run,
        "full-demo Markdown viewport has a large blank artifact during scroll step {step}: metrics={metrics:?}, max_blank_run={max_blank_run}, capture={}",
        capture_path.display()
    );
}

fn assert_lower_viewport_band_paints(
    image: &RgbaImage,
    viewport: (f32, f32, f32, f32),
    capture_path: &Path,
    step: usize,
) {
    let metrics = viewport_ink_metrics(
        image,
        viewport,
        LOWER_CONTENT_BAND_TOP_FRACTION,
        LOWER_CONTENT_BAND_BOTTOM_FRACTION,
    );
    assert!(
        metrics.ink_pixels >= MIN_LOWER_VIEWPORT_INK_PIXELS
            && metrics.ink_rows >= MIN_LOWER_VIEWPORT_INK_ROWS,
        "full-demo Markdown lower viewport band is half-empty during scroll step {step}: metrics={metrics:?}, capture={}",
        capture_path.display()
    );
}

fn assert_real_wheel_work_performance(stats: cranpose::FpsStats) {
    if !perf_contract::hardware_performance_contracts_enabled() {
        assert!(
            stats.frame_count > 0,
            "full-demo Markdown real X11 wheel produced no measured frames: {stats:?}"
        );
        perf_contract::log_software_renderer_budget("full-demo Markdown real X11 wheel");
        return;
    }
    assert!(
        stats.work_fps >= MIN_SCROLL_WORK_FPS
            && stats.work_p95_ms <= MAX_SCROLL_WORK_P95_MS
            && stats.work_stalled_50ms_frames == 0,
        "full-demo Markdown real X11 wheel frames exceeded the 120Hz work budget: work_fps={:.1}, work_p95_ms={:.2}, work_missed_120hz={}, work_stalls_50ms={}, recompositions={}, total_frames={}",
        stats.work_fps,
        stats.work_p95_ms,
        stats.work_missed_120hz_budget,
        stats.work_stalled_50ms_frames,
        stats.recompositions,
        stats.frame_count
    );
}

fn viewport_blank_artifact(metrics: InkMetrics) -> bool {
    metrics.ink_pixels < MIN_VIEWPORT_INK_PIXELS
        || metrics.ink_rows < MIN_VIEWPORT_INK_ROWS
        || metrics.blank_row_run > max_viewport_blank_run(metrics)
}

fn max_viewport_blank_run(metrics: InkMetrics) -> usize {
    ((metrics.sampled_rows as f32 * MAX_VIEWPORT_BLANK_ROW_RATIO).ceil() as usize)
        .min(MAX_VIEWPORT_ABSOLUTE_BLANK_ROW_RUN)
}

fn assert_code_metrics(metrics: InkMetrics, capture_path: &Path, target: &str) {
    assert_code_metrics_with_min_rows(metrics, capture_path, target, MIN_CODE_SAMPLED_ROWS);
}

fn assert_code_metrics_with_min_rows(
    metrics: InkMetrics,
    capture_path: &Path,
    target: &str,
    min_sampled_rows: usize,
) {
    if metrics.sampled_rows < min_sampled_rows {
        return;
    }
    assert!(
        metrics.ink_pixels >= MIN_CODE_INK_PIXELS && metrics.ink_rows >= MIN_CODE_INK_ROWS,
        "visible Markdown code block {target:?} did not paint enough pixels: metrics={metrics:?}, capture={}",
        capture_path.display()
    );
}
