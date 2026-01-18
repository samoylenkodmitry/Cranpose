//! Robot test for Async Runtime progress bar layout validation.
//!
//! This test renders only the Async Runtime tab content with static progress
//! values and validates every semantic element rect along with the progress
//! fill sizing.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_progress_bar --features robot-app
//! ```

use cranpose_app::{AppLauncher, SemanticElement};
use cranpose_testing::find_button_in_semantics;
use cranpose_ui::{Button, Column, ColumnSpec, Modifier, Size, Spacer, Text};
use desktop_app::app::{AnimationState, AsyncRuntimeTabContent, FrameStats};
use std::time::Duration;

const WINDOW_WIDTH: f32 = 900.0;
const WINDOW_HEIGHT: f32 = 700.0;
const TRACK_TAG: &str = "AsyncProgressBarTrack";
const FILL_TAG: &str = "AsyncProgressBarFill";
const TEST_PCTS: [i32; 5] = [0, 25, 50, 75, 100];

fn find_element_by_text<'a>(
    elements: &'a [SemanticElement],
    text: &str,
) -> Option<&'a SemanticElement> {
    for elem in elements {
        if elem.text.as_deref() == Some(text) {
            return Some(elem);
        }
        if let Some(found) = find_element_by_text(&elem.children, text) {
            return Some(found);
        }
    }
    None
}

fn collect_semantic_issues(
    elem: &SemanticElement,
    window_bounds: (f32, f32, f32, f32),
    issues: &mut Vec<String>,
) {
    let (vx, vy, vw, vh) = window_bounds;
    let bounds = &elem.bounds;
    let text = elem.text.as_deref().unwrap_or("");

    if !bounds.x.is_finite()
        || !bounds.y.is_finite()
        || !bounds.width.is_finite()
        || !bounds.height.is_finite()
    {
        issues.push(format!(
            "Non-finite bounds for role={} text=\"{}\"",
            elem.role, text
        ));
    }

    if bounds.width < 0.0 || bounds.height < 0.0 {
        issues.push(format!(
            "Negative size for role={} text=\"{}\" ({:.1} x {:.1})",
            elem.role, text, bounds.width, bounds.height
        ));
    }

    if !text.is_empty() && (bounds.width < 1.0 || bounds.height < 1.0) {
        issues.push(format!(
            "Zero-size content for role={} text=\"{}\" ({:.1} x {:.1})",
            elem.role, text, bounds.width, bounds.height
        ));
    }

    let right = bounds.x + bounds.width;
    let bottom = bounds.y + bounds.height;
    let tolerance = 1.0;
    if bounds.x < vx - tolerance
        || bounds.y < vy - tolerance
        || right > vx + vw + tolerance
        || bottom > vy + vh + tolerance
    {
        issues.push(format!(
            "Out of window bounds role={} text=\"{}\" ({:.1},{:.1},{:.1},{:.1})",
            elem.role, text, bounds.x, bounds.y, bounds.width, bounds.height
        ));
    }

    for child in &elem.children {
        collect_semantic_issues(child, window_bounds, issues);
    }
}

fn print_tree(elem: &SemanticElement, indent: usize) {
    let prefix = "  ".repeat(indent);
    let text = elem.text.as_deref().unwrap_or("");
    println!(
        "{}[{}] \"{}\" @ ({:.1},{:.1}) size ({:.1} x {:.1})",
        prefix,
        elem.role,
        text,
        elem.bounds.x,
        elem.bounds.y,
        elem.bounds.width,
        elem.bounds.height
    );
    for child in &elem.children {
        print_tree(child, indent + 1);
    }
}

fn validate_progress_bar(elements: &[SemanticElement], percent: i32, issues: &mut Vec<String>) {
    let track = find_element_by_text(elements, TRACK_TAG);
    let fill = find_element_by_text(elements, FILL_TAG);

    let Some(track) = track else {
        issues.push("Progress bar track element not found".to_string());
        return;
    };

    let track_bounds = &track.bounds;
    if track_bounds.width < 1.0 || track_bounds.height < 1.0 {
        issues.push(format!(
            "Progress bar track has invalid size ({:.1} x {:.1})",
            track_bounds.width, track_bounds.height
        ));
        return;
    }

    let tolerance = 2.0;
    let fraction = (percent as f32 / 100.0).clamp(0.0, 1.0);

    if percent == 0 {
        if let Some(fill) = fill {
            if fill.bounds.width > tolerance {
                issues.push(format!(
                    "Fill should be empty at 0% but is {:.1}px wide",
                    fill.bounds.width
                ));
            }
        }
        return;
    }

    let Some(fill) = fill else {
        issues.push(format!("Progress fill missing at {}%", percent));
        return;
    };

    let expected_width = track_bounds.width * fraction;
    let width_diff = (fill.bounds.width - expected_width).abs();
    if width_diff > tolerance {
        issues.push(format!(
            "Fill width mismatch at {}%: got {:.1}px, expected {:.1}px",
            percent, fill.bounds.width, expected_width
        ));
    }

    if (fill.bounds.height - track_bounds.height).abs() > tolerance {
        issues.push(format!(
            "Fill height mismatch at {}%: got {:.1}px, expected {:.1}px",
            percent, fill.bounds.height, track_bounds.height
        ));
    }

    if (fill.bounds.x - track_bounds.x).abs() > tolerance {
        issues.push(format!(
            "Fill x mismatch at {}%: got {:.1}px, expected {:.1}px",
            percent, fill.bounds.x, track_bounds.x
        ));
    }
}

fn main() {
    env_logger::init();
    println!("=== Async Runtime Progress Bar Layout Test ===");
    println!("Window size: {}x{}", WINDOW_WIDTH, WINDOW_HEIGHT);

    AppLauncher::new()
        .with_title("Async Progress Layout")
        .with_size(WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(400));

            let mut issues = Vec::new();
            for (index, percent) in TEST_PCTS.iter().enumerate() {
                robot.wait_for_idle().ok();
                std::thread::sleep(Duration::from_millis(200));

                if let Ok(semantics) = robot.get_semantics() {
                    for elem in &semantics {
                        collect_semantic_issues(
                            elem,
                            (0.0, 0.0, WINDOW_WIDTH, WINDOW_HEIGHT),
                            &mut issues,
                        );
                    }
                    validate_progress_bar(&semantics, *percent, &mut issues);

                    if !issues.is_empty() {
                        println!("\n--- Semantics Tree ({}%) ---", percent);
                        for elem in &semantics {
                            print_tree(elem, 0);
                        }
                    } else {
                        println!("✓ Layout rects + progress sizing OK for {}%", percent);
                    }
                } else {
                    issues.push("Failed to fetch semantics".to_string());
                }

                if index + 1 < TEST_PCTS.len() {
                    if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Next") {
                        robot.click(x + w / 2.0, y + h / 2.0).ok();
                        robot.wait_for_idle().ok();
                        std::thread::sleep(Duration::from_millis(200));
                    } else {
                        issues.push("Next button not found".to_string());
                    }
                }
            }

            if !issues.is_empty() {
                println!("\n=== FAILURE ===");
                for issue in &issues {
                    println!("✗ {issue}");
                }
                robot.exit().ok();
                std::process::exit(1);
            }

            println!("\n=== SUCCESS ===");
            println!("✓ Async Runtime progress layout validated");
            robot.exit().ok();
        })
        .run(|| {
            let step_state = cranpose_core::useState(|| 0usize);
            let initial_step = step_state.get();
            let percent = TEST_PCTS[initial_step];
            let progress = (percent as f32 / 100.0).clamp(0.0, 1.0);
            let animation = cranpose_core::useState(|| AnimationState {
                progress,
                direction: 1.0,
            });
            let stats = cranpose_core::useState(|| FrameStats {
                frames: 120,
                last_frame_ms: 16.0,
            });
            let is_running = cranpose_core::useState(|| false);
            let reset_signal = cranpose_core::useState(|| 0u64);

            Column(
                Modifier::empty().fill_max_size(),
                ColumnSpec::default(),
                move || {
                    let step = step_state.get();
                    Text(
                        format!("Test percent: {}%", TEST_PCTS[step]),
                        Modifier::empty().padding(6.0),
                    );
                    Button(
                        Modifier::empty().padding(6.0),
                        {
                            let step_state = step_state;
                            let animation_state = animation;
                            let stats_state = stats;
                            move || {
                                let last = TEST_PCTS.len().saturating_sub(1);
                                let next = (step_state.get() + 1).min(last);
                                if next != step_state.get() {
                                    step_state.set(next);
                                    let pct = TEST_PCTS[next];
                                    let progress_value = (pct as f32 / 100.0).clamp(0.0, 1.0);
                                    animation_state.set(AnimationState {
                                        progress: progress_value,
                                        direction: 1.0,
                                    });
                                    stats_state.set(FrameStats {
                                        frames: 120,
                                        last_frame_ms: 16.0,
                                    });
                                }
                            }
                        },
                        || {
                            Text("Next", Modifier::empty().padding(4.0));
                        },
                    );
                    Spacer(Size {
                        width: 0.0,
                        height: 8.0,
                    });
                    AsyncRuntimeTabContent(animation, stats, is_running, reset_signal);
                },
            );
        });
}
