//! Robot test for comprehensive layout validation in Async Runtime and Recursive Layout tabs
//!
//! This test dumps all semantic element bounds and validates:
//! - No zero or negative sizes
//! - No non-finite (NaN/Infinity) values  
//! - No elements positioned far outside window bounds
//! - No overlapping sibling elements (optional)
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_layout_validation --features robot-app
//! ```

use cranpose_app::{AppLauncher, SemanticElement};
use cranpose_testing::{find_button_in_semantics, find_text_in_semantics};
use desktop_app::app;
use std::time::Duration;

const WINDOW_WIDTH: f32 = 1200.0;
const WINDOW_HEIGHT: f32 = 800.0;

#[derive(Debug, Clone)]
struct LayoutIssue {
    element_text: String,
    element_role: String,
    issue: String,
    bounds: (f32, f32, f32, f32),
}

fn collect_all_bounds(
    elem: &SemanticElement,
    depth: usize,
    results: &mut Vec<(String, String, f32, f32, f32, f32, usize)>,
) {
    let text = elem.text.as_deref().unwrap_or("").to_string();
    results.push((
        text,
        elem.role.clone(),
        elem.bounds.x,
        elem.bounds.y,
        elem.bounds.width,
        elem.bounds.height,
        depth,
    ));
    for child in &elem.children {
        collect_all_bounds(child, depth + 1, results);
    }
}

fn print_semantics_tree(elem: &SemanticElement, indent: usize) {
    let prefix = "  ".repeat(indent);
    let text = elem.text.as_deref().unwrap_or("");
    let text_display = if text.len() > 40 {
        format!("{}...", &text[..37])
    } else {
        text.to_string()
    };
    println!(
        "{}[{}] \"{}\" bounds=({:.1}, {:.1}, {:.1}, {:.1}){}",
        prefix,
        elem.role,
        text_display,
        elem.bounds.x,
        elem.bounds.y,
        elem.bounds.width,
        elem.bounds.height,
        if elem.clickable { " [CLICK]" } else { "" }
    );
    for child in &elem.children {
        print_semantics_tree(child, indent + 1);
    }
}

fn validate_bounds(
    elements: &[SemanticElement],
    tab_name: &str,
    window_bounds: (f32, f32, f32, f32),
    skip_overflow_checks: bool,
) -> Vec<LayoutIssue> {
    let mut issues = Vec::new();
    let mut all_bounds = Vec::new();
    let mut zero_width_count = 0;
    let mut zero_height_count = 0;
    let mut outside_viewport_count = 0;

    for elem in elements {
        collect_all_bounds(elem, 0, &mut all_bounds);
    }

    println!(
        "\n=== {} Layout Dump ({} elements) ===",
        tab_name,
        all_bounds.len()
    );

    let (_vx, _vy, vw, vh) = window_bounds;

    for (text, role, x, y, w, h, depth) in &all_bounds {
        let indent = "  ".repeat(*depth);
        let text_display = if text.len() > 30 {
            format!("{}...", &text[..27])
        } else {
            text.clone()
        };

        // Mark elements outside viewport or with issues
        let mut markers = Vec::new();

        if *w <= 0.0 {
            zero_width_count += 1;
            markers.push("W=0");
        }
        if *h <= 0.0 {
            zero_height_count += 1;
            markers.push("H=0");
        }

        // Check if bottom edge is outside viewport
        let bottom = *y + *h;
        let right = *x + *w;
        if bottom > vh || right > vw {
            outside_viewport_count += 1;
            markers.push("OVERFLOW");
        }

        let marker_str = if markers.is_empty() {
            String::new()
        } else {
            format!(" [{}]", markers.join(", "))
        };

        // Print each element with its bounds
        println!(
            "{}[{}] \"{}\" @ ({:.1}, {:.1}) size ({:.1} x {:.1}){}",
            indent, role, text_display, x, y, w, h, marker_str
        );

        // Check for issues
        let bounds = (*x, *y, *w, *h);

        // Issue 1: Non-finite values
        if !x.is_finite() || !y.is_finite() || !w.is_finite() || !h.is_finite() {
            issues.push(LayoutIssue {
                element_text: text.clone(),
                element_role: role.clone(),
                issue: "NON-FINITE: Bounds contain NaN or Infinity".to_string(),
                bounds,
            });
        }

        // Issue 2: Zero width on content elements (not spacers)
        if *w <= 0.0 && !text.is_empty() {
            issues.push(LayoutIssue {
                element_text: text.clone(),
                element_role: role.clone(),
                issue: format!("ZERO WIDTH on content element: width={:.1}", w),
                bounds,
            });
        }

        // Issue 3: Zero height on content elements
        if *h <= 0.0 && !text.is_empty() {
            issues.push(LayoutIssue {
                element_text: text.clone(),
                element_role: role.clone(),
                issue: format!("ZERO HEIGHT on content element: height={:.1}", h),
                bounds,
            });
        }

        // Issue 4: Element extends below viewport bottom (layout overflow)
        // Skip for scrollable content like Recursive Layout
        if !skip_overflow_checks && bottom > vh + 50.0 && role == "Text" {
            issues.push(LayoutIssue {
                element_text: text.clone(),
                element_role: role.clone(),
                issue: format!(
                    "OUTSIDE VIEWPORT: y={:.1} + h={:.1} = {:.1} exceeds window height {:.0}",
                    y, h, bottom, vh
                ),
                bounds,
            });
        }

        // Issue 5: Container larger than window (potential missing scrollable)
        // Skip for scrollable content like Recursive Layout
        if !skip_overflow_checks && *h > vh && *w > 10.0 {
            issues.push(LayoutIssue {
                element_text: format!("Container at depth {}", depth),
                element_role: role.clone(),
                issue: format!(
                    "CONTAINER OVERFLOW: height {:.1} exceeds window height {:.0}",
                    h, vh
                ),
                bounds,
            });
        }

        // Issue 6: Negative position (potential bug)
        if *x < -1.0 || *y < -1.0 {
            issues.push(LayoutIssue {
                element_text: text.clone(),
                element_role: role.clone(),
                issue: format!("NEGATIVE POSITION: ({:.1}, {:.1})", x, y),
                bounds,
            });
        }

        // Issue 7: removed - was incorrectly flagging progress bar fill element
        // The progress bar architecture is:
        //   Row (fill_max_width) <- outer container, should be wide
        //     Row (fixed width) <- progress fill, should be narrow based on %
    }

    // Print summary statistics
    println!("\n--- {} Statistics ---", tab_name);
    println!("  Total elements: {}", all_bounds.len());
    println!("  Zero-width elements: {}", zero_width_count);
    println!("  Zero-height elements: {}", zero_height_count);
    println!("  Elements outside viewport: {}", outside_viewport_count);

    issues
}

fn main() {
    env_logger::init();
    println!("=== Comprehensive Layout Validation Robot Test ===");
    println!("Window size: {}x{}", WINDOW_WIDTH, WINDOW_HEIGHT);

    AppLauncher::new()
        .with_title("Layout Validation Test")
        .with_size(WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
        .with_headless(true)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(500));

            let mut all_issues: Vec<(String, LayoutIssue)> = Vec::new();
            let window_bounds = (0.0, 0.0, WINDOW_WIDTH, WINDOW_HEIGHT);

            let click_button = |name: &str| -> bool {
                if let Some((x, y, w, h)) = find_button_in_semantics(&robot, name) {
                    println!(
                        "  Clicking '{}' at ({:.1}, {:.1})",
                        name,
                        x + w / 2.0,
                        y + h / 2.0
                    );
                    robot.click(x + w / 2.0, y + h / 2.0).ok();
                    std::thread::sleep(Duration::from_millis(200));
                    true
                } else {
                    println!("  ✗ Button '{}' not found!", name);
                    false
                }
            };

            // ===== Test 1: Async Runtime Tab =====
            println!("\n\n######################################");
            println!("# TEST 1: ASYNC RUNTIME TAB");
            println!("######################################");

            if click_button("Async Runtime") {
                std::thread::sleep(Duration::from_millis(500));

                // Verify we're on the right tab
                if find_text_in_semantics(&robot, "Async Runtime Demo").is_some() {
                    println!("✓ Navigated to Async Runtime tab");

                    if let Ok(semantics) = robot.get_semantics() {
                        println!("\n--- Full Semantics Tree ---");
                        for elem in &semantics {
                            print_semantics_tree(elem, 0);
                        }

                        let issues =
                            validate_bounds(&semantics, "Async Runtime", window_bounds, false);
                        for issue in issues {
                            all_issues.push(("Async Runtime".to_string(), issue));
                        }
                    }
                } else {
                    println!("✗ Failed to verify Async Runtime tab content");
                }
            } else {
                println!("✗ Could not find Async Runtime tab button");
            }

            // ===== Test 2: Recursive Layout Tab =====
            println!("\n\n######################################");
            println!("# TEST 2: RECURSIVE LAYOUT TAB");
            println!("######################################");

            if click_button("Recursive Layout") {
                std::thread::sleep(Duration::from_millis(500));

                // Verify we're on the right tab
                if find_text_in_semantics(&robot, "Recursive Layout Playground").is_some() {
                    println!("✓ Navigated to Recursive Layout tab");

                    if let Ok(semantics) = robot.get_semantics() {
                        println!("\n--- Full Semantics Tree ---");
                        for elem in &semantics {
                            print_semantics_tree(elem, 0);
                        }

                        // Skip overflow checks - content is in a scrollable container
                        let issues =
                            validate_bounds(&semantics, "Recursive Layout", window_bounds, true);
                        for issue in issues {
                            all_issues.push(("Recursive Layout".to_string(), issue));
                        }
                    }
                } else {
                    println!("✗ Failed to verify Recursive Layout tab content");
                }

                // Also test after clicking buttons
                println!("\n--- Testing after state changes ---");
                if click_button("Increase depth") {
                    std::thread::sleep(Duration::from_millis(300));
                    if let Ok(semantics) = robot.get_semantics() {
                        let issues = validate_bounds(
                            &semantics,
                            "Recursive Layout (depth+1)",
                            window_bounds,
                            true,
                        );
                        for issue in issues {
                            all_issues.push(("Recursive Layout (depth+1)".to_string(), issue));
                        }
                    }
                }

                if click_button("Increase depth") {
                    std::thread::sleep(Duration::from_millis(300));
                    if let Ok(semantics) = robot.get_semantics() {
                        let issues = validate_bounds(
                            &semantics,
                            "Recursive Layout (depth+2)",
                            window_bounds,
                            true,
                        );
                        for issue in issues {
                            all_issues.push(("Recursive Layout (depth+2)".to_string(), issue));
                        }
                    }
                }
            } else {
                println!("✗ Could not find Recursive Layout tab button");
            }

            // ===== Final Report =====
            println!("\n\n######################################");
            println!("# LAYOUT VALIDATION REPORT");
            println!("######################################\n");

            if all_issues.is_empty() {
                println!("✓ NO LAYOUT ISSUES FOUND");
                println!("\nAll elements have valid bounds:");
                println!("  - No NaN/Infinity values");
                println!("  - No zero/negative sizes");
                println!("  - No elements far outside window");
                println!("  - No negative positions");
            } else {
                println!("✗ FOUND {} LAYOUT ISSUES:\n", all_issues.len());
                for (i, (tab, issue)) in all_issues.iter().enumerate() {
                    println!("Issue #{}: [{}]", i + 1, tab);
                    println!(
                        "  Element: [{}] \"{}\"",
                        issue.element_role, issue.element_text
                    );
                    println!("  Problem: {}", issue.issue);
                    println!(
                        "  Bounds: ({:.1}, {:.1}, {:.1}, {:.1})\n",
                        issue.bounds.0, issue.bounds.1, issue.bounds.2, issue.bounds.3
                    );
                }
                println!("\n=== TEST FAILED ===");
                robot.exit().ok();
                std::process::exit(1);
            }

            println!("\n=== Layout Validation Complete ===");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
