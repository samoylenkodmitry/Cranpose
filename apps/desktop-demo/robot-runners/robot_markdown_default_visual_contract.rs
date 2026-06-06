//! Robot test: default Markdown payload must keep the visible viewport populated after wheel scroll.

mod markdown_fixture_client;
mod output_paths;
mod text_showcase_external_helpers;

use cranpose::AppLauncher;
use cranpose_core::CompositionLocalProvider;
use cranpose_services::{local_http_client, HttpClientRef};
use cranpose_testing::{find_button_exact_in_semantics, find_in_semantics, find_text};
use desktop_app::app;
use image::RgbaImage;
use markdown_fixture_client::MarkdownFixtureClient;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use text_showcase_external_helpers::{capture_x11_window, find_window_id};

const WINDOW_WIDTH: u32 = 1080;
const WINDOW_HEIGHT: u32 = 820;
const WINDOW_TITLE: &str = "Robot Markdown Default Visual Contract";
const VIEWPORT_TAG: &str = "MarkdownListViewport";
const WHEEL_SCROLLS: usize = 10;
const WHEEL_DELTA_Y: f32 = -420.0;
const PERF_WHEEL_SCROLLS: usize = 32;
const PERF_WHEEL_DELTA_Y: f32 = -620.0;
const ACTIVE_DRAG_GESTURES: usize = 2;
const ACTIVE_DRAG_STEPS: usize = 8;
const MIN_INK_PIXELS: usize = 900;
const MIN_INK_ROWS: usize = 18;
const MIN_ROW_INK_PIXELS: usize = 4;
const MIN_CONTINUITY_ROW_INK_PIXELS: usize = 1;
const MAX_BLANK_ROW_RUN_RATIO: f32 = 0.25;
const MAX_ABSOLUTE_BLANK_ROW_RUN: usize = 72;
const ACTIVE_DRAG_MAX_BLANK_ROW_RUN_RATIO: f32 = 0.18;
const MAX_VISIBLE_TEXT_BOTTOM_BLANK_PX: u32 = 64;
const MAX_VISIBLE_TEXT_FLOW_GAP_PX: f32 = 144.0;
const MIN_SCROLL_FPS: f32 = 120.0;
const MAX_SCROLL_P95_MS: f32 = 8.33;
const RENDER_STATS_ENV: &str = "CRANPOSE_MARKDOWN_DEFAULT_RENDER_STATS";

#[derive(Clone, Copy, Debug)]
struct ViewportInkMetrics {
    ink_pixels: usize,
    ink_rows: usize,
    blank_row_run: usize,
    sampled_rows: usize,
}

fn main() {
    env_logger::init();
    println!("=== Robot Markdown Default Visual Contract ===");

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(false)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(700));
            let _ = robot.wait_for_idle();

            click_button(&robot, "Fetch");
            let viewport = wait_for_viewport(&robot, 10_000);

            let center_x = viewport.0 + viewport.2 * 0.5;
            let center_y = viewport.1 + viewport.3 * 0.5;
            robot
                .move_to(center_x, center_y)
                .expect("move to Markdown viewport");
            robot.wait_for_present_frame().expect("present at viewport");

            robot.reset_fps_stats().expect("reset rapid scroll FPS stats");
            robot
                .mouse_scroll_sequence_and_wait_for_frames(
                    0.0,
                    PERF_WHEEL_DELTA_Y,
                    PERF_WHEEL_SCROLLS as u32,
                )
                .expect("rapid wheel scroll Markdown viewport");
            let rapid_stats = robot.fps_stats().expect("read FPS stats after rapid scroll");
            println!(
                "rapid_scroll_fps_summary: cadence_fps={:.1} work_fps={:.1} p95_ms={:.2} work_p95_ms={:.2} max_ms={:.2} work_max_ms={:.2} missed_120hz={} work_missed_120hz={} stalls_50ms={} work_stalls_50ms={} recompositions={} total_frames={}",
                rapid_stats.fps,
                rapid_stats.work_fps,
                rapid_stats.p95_ms,
                rapid_stats.work_p95_ms,
                rapid_stats.max_ms,
                rapid_stats.work_max_ms,
                rapid_stats.missed_120hz_budget,
                rapid_stats.work_missed_120hz_budget,
                rapid_stats.stalled_50ms_frames,
                rapid_stats.work_stalled_50ms_frames,
                rapid_stats.recompositions,
                rapid_stats.frame_count
            );
            log_render_stats(&robot, PERF_WHEEL_SCROLLS);

            robot.reset_fps_stats().expect("reset scroll FPS stats");

            for step in 0..WHEEL_SCROLLS {
                robot
                    .mouse_scroll_and_wait_for_frame(0.0, WHEEL_DELTA_Y)
                    .expect("wheel scroll Markdown viewport");
                log_render_stats(&robot, step);
                assert_presented_scroll_frame_has_visible_markdown(
                    &robot,
                    "scroll",
                    step,
                    viewport,
                    0.34,
                    0.92,
                    MAX_BLANK_ROW_RUN_RATIO,
                );
            }
            robot.reset_fps_stats().expect("reset active drag FPS stats");
            assert_active_drag_frames_have_visible_markdown(&robot, viewport);
            let active_drag_visual_stats = robot
                .fps_stats()
                .expect("read FPS stats after visual drag scroll");

            let window_id = find_window_id(WINDOW_TITLE);
            let capture_path =
                output_paths::diagnostic_path("markdown_default_visual_after_scroll.png");
            let image = capture_x11_window(&window_id, &capture_path);
            println!("captured presented Markdown viewport: {}", capture_path.display());

            let metrics = viewport_ink_metrics(&image, viewport, 0.34, 0.92);
            println!(
                "viewport ink metrics: ink_pixels={} ink_rows={} blank_row_run={} sampled_rows={}",
                metrics.ink_pixels, metrics.ink_rows, metrics.blank_row_run, metrics.sampled_rows
            );
            let visible_texts = visible_text_nodes(&robot, viewport, 0.16, 0.94);
            assert_visible_text_nodes_do_not_leave_blank_tails(
                &image,
                viewport,
                &capture_path,
                &visible_texts,
            );
            assert_visible_text_flow_has_no_large_gaps(viewport, &capture_path, &visible_texts);
            assert_complexity_block_reaches_code_heading(viewport, &capture_path, &visible_texts);
            assert_viewport_has_visible_markdown(
                metrics,
                &capture_path,
                MAX_BLANK_ROW_RUN_RATIO,
            );

            println!(
                "active_drag_visual_summary: cadence_fps={:.1} work_fps={:.1} p95_ms={:.2} work_p95_ms={:.2} max_ms={:.2} work_max_ms={:.2} missed_120hz={} work_missed_120hz={} stalls_50ms={} work_stalls_50ms={} recompositions={} total_frames={}",
                active_drag_visual_stats.fps,
                active_drag_visual_stats.work_fps,
                active_drag_visual_stats.p95_ms,
                active_drag_visual_stats.work_p95_ms,
                active_drag_visual_stats.max_ms,
                active_drag_visual_stats.work_max_ms,
                active_drag_visual_stats.missed_120hz_budget,
                active_drag_visual_stats.work_missed_120hz_budget,
                active_drag_visual_stats.stalled_50ms_frames,
                active_drag_visual_stats.work_stalled_50ms_frames,
                active_drag_visual_stats.recompositions,
                active_drag_visual_stats.frame_count
            );
            assert_visual_drag_work_performance(active_drag_visual_stats);
            assert_scroll_performance("rapid scroll", rapid_stats);

            robot.exit().expect("exit");
        })
        .run({
            let client: HttpClientRef = Arc::new(MarkdownFixtureClient::default_leetcode_daily());
            move || {
                let local = local_http_client();
                let client = client.clone();
                CompositionLocalProvider(vec![local.provides(client)], || {
                    app::MarkdownViewerRobotApp();
                });
            }
        });
}

fn assert_scroll_performance(label: &str, stats: cranpose::FpsStats) {
    assert!(
        stats.fps >= MIN_SCROLL_FPS
            && stats.p95_ms <= MAX_SCROLL_P95_MS
            && stats.stalled_50ms_frames == 0
            && stats.work_fps >= MIN_SCROLL_FPS
            && stats.work_p95_ms <= MAX_SCROLL_P95_MS
            && stats.work_stalled_50ms_frames == 0,
        "default Markdown {label} missed the 120Hz presented-frame contract: cadence_fps={:.1}, work_fps={:.1}, p95_ms={:.2}, work_p95_ms={:.2}, missed_120hz={}, work_missed_120hz={}, stalls_50ms={}, work_stalls_50ms={}, recompositions={}, total_frames={}",
        stats.fps,
        stats.work_fps,
        stats.p95_ms,
        stats.work_p95_ms,
        stats.missed_120hz_budget,
        stats.work_missed_120hz_budget,
        stats.stalled_50ms_frames,
        stats.work_stalled_50ms_frames,
        stats.recompositions,
        stats.frame_count
    );
}

fn assert_visual_drag_work_performance(stats: cranpose::FpsStats) {
    assert!(
        stats.work_fps >= MIN_SCROLL_FPS
            && stats.work_p95_ms <= MAX_SCROLL_P95_MS
            && stats.work_stalled_50ms_frames == 0,
        "default Markdown visual drag frames exceeded the 120Hz work budget: work_fps={:.1}, work_p95_ms={:.2}, work_missed_120hz={}, work_stalls_50ms={}, recompositions={}, total_frames={}",
        stats.work_fps,
        stats.work_p95_ms,
        stats.work_missed_120hz_budget,
        stats.work_stalled_50ms_frames,
        stats.recompositions,
        stats.frame_count
    );
}

fn log_render_stats(robot: &cranpose::Robot, step: usize) {
    if std::env::var_os(RENDER_STATS_ENV).is_none() {
        return;
    }

    match robot.get_render_stats() {
        Ok(Some(stats)) => {
            println!(
                "render_stats step={step} submits={} passes={} uploads={} text_hits={} text_misses={} glyph_hits={} glyph_misses={} glyph_miss_pixels={} text_hit_pixels={} text_miss_pixels={} text_raster_bytes={} image_passes={} text_passes={} text_cache_size={}",
                stats.submit_count,
                stats.pass_count,
                stats.upload_bytes,
                stats.text_image_cache_hits,
                stats.text_image_cache_misses,
                stats.text_glyph_atlas_hits,
                stats.text_glyph_atlas_misses,
                stats.text_glyph_atlas_miss_pixels,
                stats.text_image_cache_hit_pixels,
                stats.text_image_cache_miss_pixels,
                stats.text_image_raster_bytes,
                stats.image_passes,
                stats.text_passes,
                stats.text_cache_size,
            );
        }
        Ok(None) => println!("render_stats step={step} unavailable"),
        Err(err) => println!("render_stats step={step} error={err}"),
    }
}

fn assert_presented_scroll_frame_has_visible_markdown(
    robot: &cranpose::Robot,
    label: &str,
    step: usize,
    viewport: (f32, f32, f32, f32),
    top_fraction: f32,
    bottom_fraction: f32,
    max_blank_ratio: f32,
) {
    let window_id = find_window_id(WINDOW_TITLE);
    let capture_path = output_paths::diagnostic_path(&format!(
        "markdown_default_visual_{label}_frame_{step:02}.png"
    ));
    let image = capture_x11_window(&window_id, &capture_path);
    let metrics = viewport_ink_metrics(&image, viewport, top_fraction, bottom_fraction);
    println!(
        "{label}_frame_metrics step={step} ink_pixels={} ink_rows={} blank_row_run={} sampled_rows={}",
        metrics.ink_pixels, metrics.ink_rows, metrics.blank_row_run, metrics.sampled_rows
    );
    let visible_texts = visible_text_nodes(robot, viewport, 0.16, 0.94);
    assert_visible_text_nodes_do_not_leave_blank_tails(
        &image,
        viewport,
        &capture_path,
        &visible_texts,
    );
    assert_visible_text_flow_has_no_large_gaps(viewport, &capture_path, &visible_texts);
    assert_complexity_block_reaches_code_heading(viewport, &capture_path, &visible_texts);
    assert_viewport_has_visible_markdown(metrics, &capture_path, max_blank_ratio);
}

fn assert_active_drag_frames_have_visible_markdown(
    robot: &cranpose::Robot,
    viewport: (f32, f32, f32, f32),
) {
    let center_x = viewport.0 + viewport.2 * 0.5;
    let drag_start_y = viewport.1 + viewport.3 * 0.82;
    let drag_end_y = viewport.1 + viewport.3 * 0.22;

    for gesture in 0..ACTIVE_DRAG_GESTURES {
        robot
            .mouse_move(center_x, drag_start_y)
            .expect("move to Markdown drag start");
        std::thread::sleep(Duration::from_millis(30));
        robot.mouse_down().expect("press Markdown viewport");

        for step in 1..=ACTIVE_DRAG_STEPS {
            let t = step as f32 / ACTIVE_DRAG_STEPS as f32;
            let y = drag_start_y + (drag_end_y - drag_start_y) * t;
            robot
                .mouse_move(center_x, y)
                .expect("drag Markdown viewport");
            robot.pump_frames(1).expect("present active drag frame");
            assert_presented_scroll_frame_has_visible_markdown(
                robot,
                &format!("drag_g{gesture:02}"),
                step,
                viewport,
                0.16,
                0.94,
                ACTIVE_DRAG_MAX_BLANK_ROW_RUN_RATIO,
            );
        }

        robot.mouse_up().expect("release Markdown viewport");
        std::thread::sleep(Duration::from_millis(60));
    }
}

fn click_button(robot: &cranpose::Robot, label: &str) {
    let Some(bounds) = find_button_exact_in_semantics(robot, label) else {
        panic!("button {label:?} not found");
    };
    let x = bounds.0 + bounds.2 * 0.5;
    let y = bounds.1 + bounds.3 * 0.5;
    robot.click(x, y).expect("click button");
    std::thread::sleep(Duration::from_millis(150));
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

fn assert_visible_text_nodes_do_not_leave_blank_tails(
    image: &RgbaImage,
    viewport: (f32, f32, f32, f32),
    capture_path: &Path,
    visible_texts: &[((f32, f32, f32, f32), String)],
) {
    let content_band = content_band_viewport(viewport, 0.16, 0.94);

    for (bounds, text) in visible_texts {
        let fully_visible =
            bounds.1 >= content_band.1 && bounds.1 + bounds.3 <= content_band.1 + content_band.3;
        if !fully_visible || bounds.3 < 80.0 || text.trim().len() < 24 {
            continue;
        }

        let metrics = text_bounds_ink_metrics(image, viewport, *bounds);
        if metrics.sampled_rows < 80 || metrics.ink_rows < 4 {
            continue;
        }
        assert!(
            metrics.blank_row_run <= MAX_VISIBLE_TEXT_BOTTOM_BLANK_PX as usize,
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

fn assert_visible_text_flow_has_no_large_gaps(
    viewport: (f32, f32, f32, f32),
    capture_path: &Path,
    visible_texts: &[((f32, f32, f32, f32), String)],
) {
    let content_band = content_band_viewport(viewport, 0.16, 0.94);

    let mut flow_texts = visible_texts
        .iter()
        .filter(|(bounds, text)| {
            let trimmed = text.trim();
            !trimmed.is_empty()
                && bounds.2 >= 12.0
                && bounds.3 >= 8.0
                && bounds.1 >= content_band.1
                && bounds.1 + bounds.3 <= content_band.1 + content_band.3
        })
        .cloned()
        .collect::<Vec<_>>();
    flow_texts.sort_by(|(a, _), (b, _)| a.1.total_cmp(&b.1).then_with(|| a.0.total_cmp(&b.0)));

    let mut previous: Option<((f32, f32, f32, f32), String)> = None;
    for (bounds, text) in flow_texts {
        let Some((previous_bounds, previous_text)) = previous.replace((bounds, text.clone()))
        else {
            continue;
        };
        let previous_bottom = previous_bounds.1 + previous_bounds.3;
        if bounds.1 <= previous_bottom + 2.0 {
            continue;
        }
        let gap = bounds.1 - previous_bottom;
        assert!(
            gap <= MAX_VISIBLE_TEXT_FLOW_GAP_PX,
            "Markdown left a large blank vertical gap between visible text nodes: gap={gap:.1}, previous_bounds=({:.1},{:.1},{:.1},{:.1}) next_bounds=({:.1},{:.1},{:.1},{:.1}) previous_text={:?} next_text={:?} capture={}",
            previous_bounds.0,
            previous_bounds.1,
            previous_bounds.2,
            previous_bounds.3,
            bounds.0,
            bounds.1,
            bounds.2,
            bounds.3,
            previous_text.chars().take(96).collect::<String>(),
            text.chars().take(96).collect::<String>(),
            capture_path.display()
        );
    }
}

fn visible_text_nodes(
    robot: &cranpose::Robot,
    viewport: (f32, f32, f32, f32),
    top_fraction: f32,
    bottom_fraction: f32,
) -> Vec<((f32, f32, f32, f32), String)> {
    let semantics = robot
        .get_semantics()
        .unwrap_or_else(|err| panic!("failed to get Markdown semantics: {err}"));
    let content_band = content_band_viewport(viewport, top_fraction, bottom_fraction);
    let mut visible_texts = Vec::new();
    for root in &semantics {
        collect_visible_text_nodes(root, content_band, &mut visible_texts);
    }
    visible_texts
}

fn assert_complexity_block_reaches_code_heading(
    viewport: (f32, f32, f32, f32),
    capture_path: &Path,
    visible_texts: &[((f32, f32, f32, f32), String)],
) {
    const MAX_COMPLEXITY_TO_CODE_GAP_PX: f32 = 96.0;
    const MIN_COMPLEXITY_FOLLOWUP_ROOM_PX: f32 = 96.0;

    let content_band = content_band_viewport(viewport, 0.16, 0.94);

    let Some(complexity_bounds) = visible_texts
        .iter()
        .filter(|(_, text)| text.trim() == "Complexity")
        .map(|(bounds, _)| *bounds)
        .max_by(|a, b| a.1.total_cmp(&b.1))
    else {
        return;
    };

    let content_bottom = content_band.1 + content_band.3;
    let room_below_complexity = content_bottom - (complexity_bounds.1 + complexity_bounds.3);
    if room_below_complexity < MIN_COMPLEXITY_FOLLOWUP_ROOM_PX {
        return;
    }

    let Some(space_complexity_bounds) = visible_texts
        .iter()
        .filter(|(_, text)| text.contains("Space complexity"))
        .map(|(bounds, _)| *bounds)
        .max_by(|a, b| a.1.total_cmp(&b.1))
    else {
        let visible_debug = visible_texts
            .iter()
            .map(|(bounds, text)| {
                format!(
                    "({:.1},{:.1},{:.1},{:.1}) {:?}",
                    bounds.0,
                    bounds.1,
                    bounds.2,
                    bounds.3,
                    text.chars().take(96).collect::<String>()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        panic!(
            "Markdown Complexity block is visible but Space complexity row is missing: capture={}\nvisible_texts:\n{}",
            capture_path.display(),
            visible_debug
        );
    };

    let Some(code_bounds) = visible_texts
        .iter()
        .filter(|(_, text)| text.trim() == "Code")
        .map(|(bounds, _)| *bounds)
        .min_by(|a, b| a.1.total_cmp(&b.1))
    else {
        panic!(
            "Markdown Complexity block is visible but Code heading is missing below it: capture={}",
            capture_path.display()
        );
    };

    let gap = code_bounds.1 - (space_complexity_bounds.1 + space_complexity_bounds.3);
    assert!(
        gap <= MAX_COMPLEXITY_TO_CODE_GAP_PX,
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

fn collect_visible_text_nodes(
    elem: &cranpose::SemanticElement,
    viewport: (f32, f32, f32, f32),
    out: &mut Vec<((f32, f32, f32, f32), String)>,
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
        collect_visible_text_nodes(child, viewport, out);
    }
}

fn intersects_viewport(bounds: (f32, f32, f32, f32), viewport: (f32, f32, f32, f32)) -> bool {
    let (x, y, w, h) = bounds;
    let (vx, vy, vw, vh) = viewport;
    x < vx + vw && x + w > vx && y < vy + vh && y + h > vy
}

fn content_band_viewport(
    viewport: (f32, f32, f32, f32),
    top_fraction: f32,
    bottom_fraction: f32,
) -> (f32, f32, f32, f32) {
    let top = viewport.1 + viewport.3 * top_fraction;
    let bottom = viewport.1 + viewport.3 * bottom_fraction;
    (viewport.0, top, viewport.2, bottom - top)
}

fn text_bounds_ink_metrics(
    image: &RgbaImage,
    viewport: (f32, f32, f32, f32),
    bounds: (f32, f32, f32, f32),
) -> ViewportInkMetrics {
    let scale_x = image.width() as f32 / WINDOW_WIDTH as f32;
    let scale_y = image.height() as f32 / WINDOW_HEIGHT as f32;
    let left = (bounds.0.max(viewport.0 + 24.0) * scale_x).floor().max(0.0) as u32;
    let right = ((bounds.0 + bounds.2).min(viewport.0 + viewport.2 - 64.0) * scale_x)
        .ceil()
        .min(image.width() as f32) as u32;
    let top = (bounds.1 * scale_y).floor().max(0.0) as u32;
    let bottom = ((bounds.1 + bounds.3) * scale_y)
        .ceil()
        .min(image.height() as f32) as u32;
    ink_metrics_in_pixel_rect(image, left, top, right, bottom)
}

fn viewport_ink_metrics(
    image: &RgbaImage,
    viewport: (f32, f32, f32, f32),
    top_fraction: f32,
    bottom_fraction: f32,
) -> ViewportInkMetrics {
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

fn ink_metrics_in_pixel_rect(
    image: &RgbaImage,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
) -> ViewportInkMetrics {
    let mut ink_pixels = 0usize;
    let mut ink_rows = 0usize;
    let mut current_blank_run = 0usize;
    let mut blank_row_run = 0usize;
    let sampled_rows = bottom.saturating_sub(top) as usize;
    for y in top..bottom {
        let mut row_ink = 0usize;
        for x in left..right {
            if is_markdown_ink(image.get_pixel(x, y).0) {
                row_ink += 1;
            }
        }
        ink_pixels += row_ink;
        if row_ink >= MIN_ROW_INK_PIXELS {
            ink_rows += 1;
        }
        if row_ink >= MIN_CONTINUITY_ROW_INK_PIXELS {
            current_blank_run = 0;
        } else {
            current_blank_run += 1;
            blank_row_run = blank_row_run.max(current_blank_run);
        }
    }

    ViewportInkMetrics {
        ink_pixels,
        ink_rows,
        blank_row_run,
        sampled_rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_markdown_code_rows_preserve_viewport_continuity() {
        let mut image = RgbaImage::new(8, 8);
        for y in 0..8 {
            image.put_pixel(0, y, image::Rgba([220, 224, 235, 255]));
        }

        let metrics = ink_metrics_in_pixel_rect(&image, 0, 0, 8, 8);

        assert_eq!(
            metrics.blank_row_run, 0,
            "a one-glyph code row is rendered content and must not extend a blank band"
        );
        assert_eq!(
            metrics.ink_rows, 0,
            "sparse rows should not inflate the dense text-row count"
        );
    }
}

fn is_markdown_ink(pixel: [u8; 4]) -> bool {
    let [r, g, b, a] = pixel;
    if a < 180 {
        return false;
    }
    let bright_text = r > 170 && g > 170 && b > 180;
    let link_blue = b > 145 && g > 115 && r < 175;
    let math_yellow = r > 170 && g > 170 && b < 95;
    bright_text || link_blue || math_yellow
}

fn assert_viewport_has_visible_markdown(
    metrics: ViewportInkMetrics,
    capture_path: &Path,
    max_blank_ratio: f32,
) {
    let max_blank_run = ((metrics.sampled_rows as f32 * max_blank_ratio).ceil() as usize)
        .min(MAX_ABSOLUTE_BLANK_ROW_RUN);
    assert!(
        metrics.ink_pixels >= MIN_INK_PIXELS
            && metrics.ink_rows >= MIN_INK_ROWS
            && metrics.blank_row_run <= max_blank_run,
        "default Markdown viewport lost rendered content after scroll: metrics={metrics:?}, max_blank_run={max_blank_run}, capture={}",
        capture_path.display()
    );
}
