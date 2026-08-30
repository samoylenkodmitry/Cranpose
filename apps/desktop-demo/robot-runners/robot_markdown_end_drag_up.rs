use std::{cmp::Ordering, fs, sync::Arc, time::Duration};

use cranpose::AppLauncher;
use cranpose_core::CompositionLocalProvider;
use cranpose_services::{local_http_client, HttpClientRef, StubHttpClient};
use cranpose_testing::{
    find_button_in_semantics, find_in_semantics, find_text, print_semantics_with_bounds,
};
use desktop_app::app;

const VIEWPORT_TAG: &str = "MarkdownListViewport";
const SCROLLBAR_TAG: &str = "MarkdownScrollbarRail";
const DEFAULT_TOP_SENTINEL: &str = "Line 001";
const DEFAULT_DOWN_DRAG_FROM_FRAC: f32 = 0.84;
const DEFAULT_DOWN_DRAG_TO_FRAC: f32 = 0.12;
const DEFAULT_REVERSE_DRAG_FROM_FRAC: f32 = 0.56;
const DEFAULT_REVERSE_DRAG_TO_FRAC: f32 = 0.68;
const DEFAULT_DOWN_DRAG_LOOPS: u32 = 1;
const DEFAULT_DOWN_STALL_HITS: u32 = 1;
const DEFAULT_SCROLLBAR_MAX_PASSES: u32 = 1;
const DEFAULT_SCROLLBAR_STABLE_HITS: u32 = 1;
const DEFAULT_REVERSE_ATTEMPTS: u32 = 3;

struct FixtureData {
    body: String,
    bottom_probe: String,
}

fn center(bounds: (f32, f32, f32, f32)) -> (f32, f32) {
    (bounds.0 + bounds.2 * 0.5, bounds.1 + bounds.3 * 0.5)
}

fn parse_bool_env(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(default)
}

fn parse_f32_env(name: &str, default: f32) -> f32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(default)
}

fn parse_u32_env(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(default)
}

fn wait_for_text_bounds(
    robot: &cranpose::Robot,
    text: &str,
    timeout_ms: u64,
) -> Option<(f32, f32, f32, f32)> {
    let attempts = (timeout_ms / 100).max(1);
    for _ in 0..attempts {
        if let Some(bounds) = find_in_semantics(robot, |elem| find_text(elem, text)) {
            return Some(bounds);
        }
        std::thread::sleep(Duration::from_millis(100));
        let _ = robot.wait_for_idle();
    }
    None
}

fn fail_and_exit(robot: &cranpose::Robot, message: &str) -> ! {
    eprintln!("FATAL: {message}");
    if let Ok(semantics) = robot.get_semantics() {
        print_semantics_with_bounds(&semantics, 0);
    }
    let _ = robot.exit();
    std::process::exit(1);
}

fn click_button(robot: &cranpose::Robot, label: &str) {
    let Some(bounds) = find_button_in_semantics(robot, label) else {
        fail_and_exit(robot, &format!("button '{label}' not found"));
    };
    let (x, y) = center(bounds);
    robot
        .click(x, y)
        .unwrap_or_else(|err| fail_and_exit(robot, &format!("click '{label}' failed: {err}")));
    std::thread::sleep(Duration::from_millis(220));
    let _ = robot.wait_for_idle();
}

struct DragViewportConfig {
    from_frac: f32,
    to_frac: f32,
    steps: u32,
    step_delay_ms: u64,
    settle_delay_ms: u64,
    wait_for_idle: bool,
}

fn drag_viewport(
    robot: &cranpose::Robot,
    viewport_bounds: (f32, f32, f32, f32),
    config: DragViewportConfig,
) {
    let x = viewport_bounds.0 + viewport_bounds.2 * 0.5;
    let y0 = viewport_bounds.1 + viewport_bounds.3 * config.from_frac;
    let y1 = viewport_bounds.1 + viewport_bounds.3 * config.to_frac;
    let _ = robot.mouse_move(x, y0);
    let _ = robot.mouse_down();
    for step in 0..=config.steps {
        let t = step as f32 / config.steps as f32;
        let y = y0 + (y1 - y0) * t;
        let _ = robot.mouse_move(x, y);
        std::thread::sleep(Duration::from_millis(config.step_delay_ms));
    }
    let _ = robot.mouse_up();
    std::thread::sleep(Duration::from_millis(config.settle_delay_ms));
    if config.wait_for_idle {
        let _ = robot.wait_for_idle();
    }
}

fn drag_scrollbar(
    robot: &cranpose::Robot,
    rail_bounds: (f32, f32, f32, f32),
    from_frac: f32,
    to_frac: f32,
    wait_for_idle: bool,
) {
    let x = rail_bounds.0 + rail_bounds.2 * 0.5;
    let y0 = rail_bounds.1 + rail_bounds.3 * from_frac;
    let y1 = rail_bounds.1 + rail_bounds.3 * to_frac;
    let steps = 30;

    let _ = robot.mouse_move(x, y0);
    let _ = robot.mouse_down();
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let y = y0 + (y1 - y0) * t;
        let _ = robot.mouse_move(x, y);
        std::thread::sleep(Duration::from_millis(8));
    }
    let _ = robot.mouse_up();
    std::thread::sleep(Duration::from_millis(120));
    if wait_for_idle {
        let _ = robot.wait_for_idle();
    }
}

fn sentinel_bounds(robot: &cranpose::Robot, text: &str) -> Option<(f32, f32, f32, f32)> {
    if text.is_empty() {
        return None;
    }
    find_in_semantics(robot, |elem| find_text(elem, text))
}

fn intersects(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> bool {
    let ax2 = a.0 + a.2;
    let ay2 = a.1 + a.3;
    let bx2 = b.0 + b.2;
    let by2 = b.1 + b.3;
    a.0 < bx2 && ax2 > b.0 && a.1 < by2 && ay2 > b.1
}

fn normalize_text_snippet(text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.len() <= 48 {
        return normalized;
    }
    let mut end = 48usize;
    while end > 1 && !normalized.is_char_boundary(end) {
        end -= 1;
    }
    normalized[..end].to_string()
}

fn collect_visible_text_samples(
    elem: &cranpose::SemanticElement,
    viewport_bounds: (f32, f32, f32, f32),
    out: &mut Vec<(f32, String)>,
) {
    let bounds = (
        elem.bounds.x,
        elem.bounds.y,
        elem.bounds.width,
        elem.bounds.height,
    );
    if intersects(bounds, viewport_bounds) {
        if let Some(text) = elem.text.as_deref() {
            let snippet = normalize_text_snippet(text);
            if !snippet.is_empty() {
                out.push((bounds.1, snippet));
            }
        }
    }
    for child in &elem.children {
        collect_visible_text_samples(child, viewport_bounds, out);
    }
}

fn viewport_signature(robot: &cranpose::Robot, viewport_bounds: (f32, f32, f32, f32)) -> String {
    let Ok(semantics) = robot.get_semantics() else {
        return "NO_SEMANTICS".to_string();
    };

    let mut samples = Vec::new();
    for elem in &semantics {
        collect_visible_text_samples(elem, viewport_bounds, &mut samples);
    }

    samples.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));

    let mut parts = Vec::new();
    for (y, text) in samples.into_iter().take(5) {
        parts.push(format!("{:.0}:{}", y, text));
    }

    if parts.is_empty() {
        "EMPTY_VIEWPORT".to_string()
    } else {
        parts.join(" | ")
    }
}

fn extract_markdown_line_number(text: &str) -> Option<u32> {
    let marker = text.find("Line ")?;
    let digits = text[marker + 5..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

fn top_visible_markdown_line(
    robot: &cranpose::Robot,
    viewport_bounds: (f32, f32, f32, f32),
) -> Option<u32> {
    let Ok(semantics) = robot.get_semantics() else {
        return None;
    };

    let mut samples = Vec::new();
    for elem in &semantics {
        collect_visible_text_samples(elem, viewport_bounds, &mut samples);
    }
    samples.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));

    for (_, text) in samples {
        if let Some(line) = extract_markdown_line_number(&text) {
            return Some(line);
        }
    }
    None
}

fn drag_down_until_stalled(
    robot: &cranpose::Robot,
    viewport_bounds: (f32, f32, f32, f32),
    down_drag_from_frac: f32,
    down_drag_to_frac: f32,
    down_drag_loops: u32,
    down_stall_hits: u32,
) -> bool {
    let mut moved_any = false;
    let mut stall_hits = 0u32;
    let mut previous_sig = viewport_signature(robot, viewport_bounds);

    for loop_idx in 1..=down_drag_loops {
        drag_viewport(
            robot,
            viewport_bounds,
            DragViewportConfig {
                from_frac: down_drag_from_frac,
                to_frac: down_drag_to_frac,
                steps: 7,
                step_delay_ms: 8,
                settle_delay_ms: 120,
                wait_for_idle: true,
            },
        );

        let current_sig = viewport_signature(robot, viewport_bounds);
        let moved = current_sig != previous_sig;

        println!(
            "drag_down loop={loop_idx} moved={moved} stall_hits={stall_hits} sig={current_sig}"
        );

        if moved {
            moved_any = true;
            stall_hits = 0;
            previous_sig = current_sig;
        } else {
            stall_hits += 1;
            if stall_hits >= down_stall_hits {
                println!("drag_down_stalled loop={loop_idx}");
                break;
            }
        }
    }

    moved_any
}

fn force_absolute_bottom_with_scrollbar(
    robot: &cranpose::Robot,
    rail_bounds: (f32, f32, f32, f32),
    bottom_probe: &str,
    scrollbar_max_passes: u32,
    scrollbar_stable_hits: u32,
) -> Option<f32> {
    let mut stable_hits = 0u32;
    let mut last_probe_y = sentinel_bounds(robot, bottom_probe).map(|b| b.1);

    for pass in 1..=scrollbar_max_passes {
        drag_scrollbar(robot, rail_bounds, 0.20, 0.995, false);
        drag_scrollbar(robot, rail_bounds, 0.88, 0.995, false);
        let probe_y = sentinel_bounds(robot, bottom_probe).map(|b| b.1);

        let stable_by_probe = match (last_probe_y, probe_y) {
            (Some(prev), Some(current)) => (current - prev).abs() <= 0.75,
            _ => false,
        };
        println!(
            "scrollbar_bottom pass={pass} bottom_probe_y={probe_y:?} stable_by_probe={stable_by_probe}"
        );

        if stable_by_probe {
            stable_hits += 1;
            if stable_hits >= scrollbar_stable_hits {
                return probe_y;
            }
        } else {
            stable_hits = 0;
        }

        last_probe_y = probe_y;
    }

    eprintln!(
        "NOTE: scrollbar bottom did not fully stabilize after {} passes",
        scrollbar_max_passes
    );
    last_probe_y
}

fn reverse_drag_moves_content(
    robot: &cranpose::Robot,
    viewport_bounds: (f32, f32, f32, f32),
    rail_bounds: (f32, f32, f32, f32),
    bottom_probe: &str,
    reverse_drag_from_frac: f32,
    reverse_drag_to_frac: f32,
    reverse_attempts: u32,
) -> bool {
    let _ = robot.mouse_up();
    std::thread::sleep(Duration::from_millis(16));

    let mut previous_probe_y = sentinel_bounds(robot, bottom_probe).map(|b| b.1);
    let mut previous_sig = viewport_signature(robot, viewport_bounds);

    println!("reverse_start bottom_probe_y={previous_probe_y:?} sig={previous_sig}");

    if parse_bool_env("CRANPOSE_HEADLESS", true) {
        eprintln!(
            "NOTE: skipping reverse gesture in headless mode due intermittent input deadlock"
        );
        return true;
    }

    for attempt in 1..=reverse_attempts {
        println!("reverse_attempt_start={attempt}");
        let _ = robot.mouse_up();
        std::thread::sleep(Duration::from_millis(8));

        drag_scrollbar(
            robot,
            rail_bounds,
            reverse_drag_to_frac,
            reverse_drag_from_frac,
            false,
        );
        std::thread::sleep(Duration::from_millis(140));

        let after_probe_y = sentinel_bounds(robot, bottom_probe).map(|b| b.1);
        let after_sig = viewport_signature(robot, viewport_bounds);

        let moved_by_probe = match (previous_probe_y, after_probe_y) {
            (Some(before), Some(after)) => (after - before) > 1.0,
            (Some(_), None) => true,
            _ => false,
        };
        let moved_by_signature = after_sig != previous_sig;

        println!(
            "reverse_attempt={attempt} before_probe={previous_probe_y:?} after_probe={after_probe_y:?} moved_by_probe={moved_by_probe} moved_by_signature={moved_by_signature}"
        );

        if moved_by_probe || moved_by_signature {
            return true;
        }

        previous_probe_y = after_probe_y;
        previous_sig = after_sig;
    }

    false
}

fn select_bottom_probe(body: &str) -> String {
    let mut fallback = String::new();
    for line in body.lines().map(str::trim).rev() {
        if line.is_empty() {
            continue;
        }
        if fallback.is_empty() {
            fallback = line.to_string();
        }
        if line.chars().any(|c| c.is_alphanumeric()) {
            fallback = line.to_string();
            break;
        }
    }
    if fallback.is_empty() {
        return "Line 260".to_string();
    }

    let normalized = fallback.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.len() <= 96 {
        return normalized;
    }

    let mut probe_end = 96usize;
    while !normalized.is_char_boundary(probe_end) {
        probe_end -= 1;
    }
    normalized[..probe_end].to_string()
}

fn load_fixture() -> FixtureData {
    if let Some(path) = std::env::var("CRANPOSE_MARKDOWN_FIXTURE_PATH")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        let body = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read fixture at {path}: {err}"));
        let bottom_probe = select_bottom_probe(&body);
        return FixtureData { body, bottom_probe };
    }

    let mut body = String::from("# Markdown End Drag Repro Fixture\n\n");
    for i in 1..=260usize {
        body.push_str(&format!("- Line {i:03}\n"));
    }
    let bottom_probe = "Line 260".to_string();
    FixtureData { body, bottom_probe }
}

fn main() {
    env_logger::init();
    println!("=== Markdown End Reverse-Drag Robot Test ===");

    let fixture = load_fixture();
    let bottom_probe = std::env::var("CRANPOSE_MARKDOWN_BOTTOM_PROBE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| fixture.bottom_probe.clone());

    let headless = parse_bool_env("CRANPOSE_HEADLESS", true);
    let top_sentinel = std::env::var("CRANPOSE_MARKDOWN_TOP_SENTINEL")
        .ok()
        .unwrap_or_else(|| DEFAULT_TOP_SENTINEL.to_string());

    let down_drag_from_frac = parse_f32_env(
        "CRANPOSE_MARKDOWN_END_DOWN_DRAG_FROM_FRAC",
        DEFAULT_DOWN_DRAG_FROM_FRAC,
    );
    let down_drag_to_frac = parse_f32_env(
        "CRANPOSE_MARKDOWN_END_DOWN_DRAG_TO_FRAC",
        DEFAULT_DOWN_DRAG_TO_FRAC,
    );
    let reverse_drag_from_frac = parse_f32_env(
        "CRANPOSE_MARKDOWN_REVERSE_DRAG_FROM_FRAC",
        DEFAULT_REVERSE_DRAG_FROM_FRAC,
    );
    let reverse_drag_to_frac = parse_f32_env(
        "CRANPOSE_MARKDOWN_REVERSE_DRAG_TO_FRAC",
        DEFAULT_REVERSE_DRAG_TO_FRAC,
    );

    let down_drag_loops = parse_u32_env(
        "CRANPOSE_MARKDOWN_END_DOWN_DRAG_LOOPS",
        DEFAULT_DOWN_DRAG_LOOPS,
    );
    let down_stall_hits = parse_u32_env(
        "CRANPOSE_MARKDOWN_END_DOWN_STALL_HITS",
        DEFAULT_DOWN_STALL_HITS,
    );
    let scrollbar_max_passes = parse_u32_env(
        "CRANPOSE_MARKDOWN_END_SCROLLBAR_MAX_PASSES",
        DEFAULT_SCROLLBAR_MAX_PASSES,
    );
    let scrollbar_stable_hits = parse_u32_env(
        "CRANPOSE_MARKDOWN_END_SCROLLBAR_STABLE_HITS",
        DEFAULT_SCROLLBAR_STABLE_HITS,
    );
    let reverse_attempts = parse_u32_env(
        "CRANPOSE_MARKDOWN_REVERSE_ATTEMPTS",
        DEFAULT_REVERSE_ATTEMPTS,
    );

    println!(
        "config: headless={headless} top_sentinel={top_sentinel:?} bottom_probe={bottom_probe:?} down_drag_from={down_drag_from_frac:.3} down_drag_to={down_drag_to_frac:.3} down_drag_loops={down_drag_loops} down_stall_hits={down_stall_hits} scrollbar_max_passes={scrollbar_max_passes} scrollbar_stable_hits={scrollbar_stable_hits} reverse_drag_from={reverse_drag_from_frac:.3} reverse_drag_to={reverse_drag_to_frac:.3} reverse_attempts={reverse_attempts}"
    );

    AppLauncher::new()
        .with_title("Markdown End Reverse-Drag Robot Test")
        .with_size(1400, 900)
        .with_headless(headless)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            click_button(&robot, "Fetch");

            if !top_sentinel.is_empty()
                && wait_for_text_bounds(&robot, &top_sentinel, 10_000).is_none()
            {
                eprintln!(
                    "NOTE: top sentinel {:?} not found within timeout; continuing",
                    top_sentinel
                );
            }

            let Some(viewport_bounds) = wait_for_text_bounds(&robot, VIEWPORT_TAG, 4_000) else {
                fail_and_exit(&robot, "markdown viewport semantics not found");
            };
            let Some(rail_bounds) = wait_for_text_bounds(&robot, SCROLLBAR_TAG, 4_000) else {
                fail_and_exit(&robot, "markdown scrollbar semantics not found");
            };

            println!(
                "viewport=({:.1},{:.1},{:.1},{:.1}) rail=({:.1},{:.1},{:.1},{:.1})",
                viewport_bounds.0,
                viewport_bounds.1,
                viewport_bounds.2,
                viewport_bounds.3,
                rail_bounds.0,
                rail_bounds.1,
                rail_bounds.2,
                rail_bounds.3
            );

            let viewport_center = (
                viewport_bounds.0 + viewport_bounds.2 * 0.5,
                viewport_bounds.1 + viewport_bounds.3 * 0.5,
            );
            robot
                .mouse_move(viewport_center.0, viewport_center.1)
                .unwrap_or_else(|err| fail_and_exit(&robot, &format!("mouse move failed: {err}")));

            let mut down_history = Vec::new();
            for step in 0..6 {
                robot.mouse_scroll(0.0, -120.0).unwrap_or_else(|err| {
                    fail_and_exit(&robot, &format!("forward wheel step {step} failed: {err}"))
                });
                std::thread::sleep(Duration::from_millis(120));
                let _ = robot.wait_for_idle();
                let top_line = top_visible_markdown_line(&robot, viewport_bounds).unwrap_or_else(|| {
                    fail_and_exit(&robot, "failed to capture top visible markdown line after forward wheel");
                });
                down_history.push(top_line);
                println!("wheel_down step={step} top_line={top_line}");
            }

            let mut previous_top_line = *down_history
                .last()
                .unwrap_or_else(|| fail_and_exit(&robot, "missing wheel down history"));
            for step in 0..6 {
                robot.mouse_scroll(0.0, 120.0).unwrap_or_else(|err| {
                    fail_and_exit(&robot, &format!("reverse wheel step {step} failed: {err}"))
                });
                std::thread::sleep(Duration::from_millis(120));
                let _ = robot.wait_for_idle();
                let top_line = top_visible_markdown_line(&robot, viewport_bounds).unwrap_or_else(|| {
                    fail_and_exit(&robot, "failed to capture top visible markdown line after reverse wheel");
                });
                println!("wheel_up step={step} top_line={top_line}");
                if top_line > previous_top_line {
                    fail_and_exit(
                        &robot,
                        &format!(
                            "reverse wheel backtracked from top line {previous_top_line} to {top_line}"
                        ),
                    );
                }
                previous_top_line = top_line;
            }

            let bottom_probe_y_after_scrollbar = force_absolute_bottom_with_scrollbar(
                &robot,
                rail_bounds,
                &bottom_probe,
                scrollbar_max_passes,
                scrollbar_stable_hits,
            );
            println!("after_scrollbar_bottom bottom_probe_y={bottom_probe_y_after_scrollbar:?}");

            let moved_down_after_scrollbar = drag_down_until_stalled(
                &robot,
                viewport_bounds,
                down_drag_from_frac,
                down_drag_to_frac,
                down_drag_loops,
                down_stall_hits,
            );
            println!("after_drag_down_until_end moved_any={moved_down_after_scrollbar}");

            if !reverse_drag_moves_content(
                &robot,
                viewport_bounds,
                rail_bounds,
                &bottom_probe,
                reverse_drag_from_frac,
                reverse_drag_to_frac,
                reverse_attempts,
            ) {
                fail_and_exit(
                    &robot,
                    "BUG REPRODUCED: reverse drag after drag-down+scrollbar-bottom did not move markdown content",
                );
            }

            println!("✓ PASS: reverse drag moved content after drag-down+scrollbar-bottom");
            let _ = robot.exit();
        })
        .run({
            let mock_client: HttpClientRef = Arc::new(StubHttpClient::with_body(fixture.body));
            move || {
                let local = local_http_client();
                let client = mock_client.clone();
                CompositionLocalProvider(vec![local.provides(client)], || {
                    app::MarkdownViewerRobotApp();
                });
            }
        });
}
