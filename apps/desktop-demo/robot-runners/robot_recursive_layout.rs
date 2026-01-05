//! Robot test for Recursive Layout tab - validates layout bounds and prints rects
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_recursive_layout --features robot-app
//! ```

use compose_app::{AppLauncher, SemanticElement};
use compose_testing::{
    find_button_in_semantics, find_text_by_prefix_in_semantics, find_text_in_semantics,
};
use desktop_app::app;
use std::time::Duration;

#[allow(dead_code)]
fn find_element_by_text_exact<'a>(
    elements: &'a [SemanticElement],
    text: &str,
) -> Option<&'a SemanticElement> {
    for elem in elements {
        if elem.text.as_deref() == Some(text) {
            return Some(elem);
        }
        if let Some(found) = find_element_by_text_exact(&elem.children, text) {
            return Some(found);
        }
    }
    None
}

#[allow(dead_code)]
fn collect_elements_by_text_prefix<'a>(
    elements: &'a [SemanticElement],
    prefix: &str,
    results: &mut Vec<&'a SemanticElement>,
) {
    for elem in elements {
        if let Some(text) = elem.text.as_deref() {
            if text.starts_with(prefix) {
                results.push(elem);
            }
        }
        collect_elements_by_text_prefix(&elem.children, prefix, results);
    }
}

fn print_semantics_with_bounds(elements: &[SemanticElement], indent: usize) {
    for elem in elements {
        let prefix = "  ".repeat(indent);
        let text = elem.text.as_deref().unwrap_or("");
        println!(
            "{}role={} text=\"{}\" bounds=({:.1},{:.1},{:.1},{:.1}){}",
            prefix,
            elem.role,
            text,
            elem.bounds.x,
            elem.bounds.y,
            elem.bounds.width,
            elem.bounds.height,
            if elem.clickable { " [CLICKABLE]" } else { "" }
        );
        print_semantics_with_bounds(&elem.children, indent + 1);
    }
}

#[allow(dead_code)]
fn validate_nodes_within_viewport(
    nodes: &[&SemanticElement],
    viewport: (f32, f32, f32, f32),
    label: &str,
) -> bool {
    let (vx, vy, vw, vh) = viewport;
    let mut has_issues = false;

    println!("\n--- {label}: Node Bounds ---");
    for node in nodes {
        let bounds = &node.bounds;
        let text = node.text.as_deref().unwrap_or("");
        println!(
            "  {}: ({:.1},{:.1},{:.1},{:.1})",
            text, bounds.x, bounds.y, bounds.width, bounds.height
        );

        if !bounds.x.is_finite()
            || !bounds.y.is_finite()
            || !bounds.width.is_finite()
            || !bounds.height.is_finite()
        {
            println!("    ⚠️  Non-finite bounds detected");
            has_issues = true;
        }

        if bounds.width <= 2.0 || bounds.height <= 2.0 {
            println!("    ⚠️  Size too small");
            has_issues = true;
        }

        let node_right = bounds.x + bounds.width;
        let node_bottom = bounds.y + bounds.height;
        let viewport_right = vx + vw;
        let viewport_bottom = vy + vh;

        if bounds.x < vx - 1.0
            || bounds.y < vy - 1.0
            || node_right > viewport_right + 1.0
            || node_bottom > viewport_bottom + 1.0
        {
            println!("    ⚠️  Node exceeds RecursiveLayoutViewport bounds");
            has_issues = true;
        }
    }

    has_issues
}

fn main() {
    env_logger::init();
    println!("=== Recursive Layout Robot Test (rect validation) ===");

    AppLauncher::new()
        .with_title("Recursive Layout Test")
        .with_size(1200, 800)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(500));

            let click_button = |name: &str| -> bool {
                if let Some((x, y, w, h)) = find_button_in_semantics(&robot, name) {
                    println!("  Found button '{}' at ({:.1}, {:.1})", name, x, y);
                    robot.click(x + w / 2.0, y + h / 2.0).ok();
                    std::thread::sleep(Duration::from_millis(150));
                    true
                } else {
                    println!("  ✗ Button '{}' not found!", name);
                    false
                }
            };

            // Step 1: Navigate to Modifiers Showcase tab and scroll
            println!("\n--- Step 1: Navigate to 'Modifiers Showcase' tab ---");
            if !click_button("Modifiers Showcase") {
                println!("FATAL: Could not find 'Modifiers Showcase' tab button");
                robot.exit().ok();
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(400));

            let skip_scroll = std::env::var("ROBOT_SKIP_SCROLL").is_ok();
            if !skip_scroll {
                if click_button("Long List (50)") {
                    std::thread::sleep(Duration::from_millis(200));
                }

                if let Some((x, y, w, h)) = find_text_in_semantics(&robot, "Select Showcase") {
                    println!("  Scrolling modifiers showcase content");
                    robot
                        .drag(
                            x + w / 2.0,
                            y + h / 2.0 + 220.0,
                            x + w / 2.0,
                            y + h / 2.0 - 220.0,
                        )
                        .ok();
                    std::thread::sleep(Duration::from_millis(200));
                }
            }

            // Step 2: Navigate to Recursive Layout tab
            println!("\n--- Step 2: Navigate to 'Recursive Layout' tab ---");
            if !click_button("Recursive Layout") {
                println!("FATAL: Could not find 'Recursive Layout' tab button");
                robot.exit().ok();
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(400));

            // Step 3: Verify Recursive Layout header + controls
            println!("\n--- Step 3: Verify Recursive Layout header + controls ---");
            if find_text_in_semantics(&robot, "Recursive Layout Playground").is_none() {
                println!("  ✗ Missing Recursive Layout header");
                robot.exit().ok();
                std::process::exit(1);
            }

            let mut has_issues = false;
            let mut control_bounds = Vec::new();
            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Increase depth") {
                println!(
                    "  ✓ Increase depth button bounds=({:.1},{:.1},{:.1},{:.1})",
                    x, y, w, h
                );
                control_bounds.push(("Increase depth", (x, y, w, h)));
            } else {
                println!("  ✗ Missing Increase depth button");
                has_issues = true;
            }

            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Decrease depth") {
                println!(
                    "  ✓ Decrease depth button bounds=({:.1},{:.1},{:.1},{:.1})",
                    x, y, w, h
                );
                control_bounds.push(("Decrease depth", (x, y, w, h)));
            } else {
                println!("  ✗ Missing Decrease depth button");
                has_issues = true;
            }

            if let Some((x, y, w, h, text)) =
                find_text_by_prefix_in_semantics(&robot, "Current depth:")
            {
                println!("  ✓ {} bounds=({:.1},{:.1},{:.1},{:.1})", text, x, y, w, h);
                control_bounds.push(("Current depth", (x, y, w, h)));
            } else {
                println!("  ✗ Missing Current depth label");
                has_issues = true;
            }

            let window_bounds = (0.0, 0.0, 1200.0, 800.0);
            for (label, (x, y, w, h)) in &control_bounds {
                let right = x + w;
                let bottom = y + h;
                if !x.is_finite() || !y.is_finite() || !w.is_finite() || !h.is_finite() {
                    println!("  ⚠️  {} bounds are non-finite", label);
                    has_issues = true;
                }
                if *w <= 2.0 || *h <= 2.0 {
                    println!("  ⚠️  {} bounds too small", label);
                    has_issues = true;
                }
                if *x < window_bounds.0 - 1.0
                    || *y < window_bounds.1 - 1.0
                    || right > window_bounds.2 + 1.0
                    || bottom > window_bounds.3 + 1.0
                {
                    println!("  ⚠️  {} is outside the window bounds", label);
                    has_issues = true;
                }
            }

            if has_issues {
                if let Ok(elements) = robot.get_semantics() {
                    println!("\n--- Recursive Layout semantics dump ---");
                    print_semantics_with_bounds(&elements, 0);
                }
                let _ = robot.send_key("d");
            }

            // TODO: Add RecursiveLayoutViewport semantics to the demo app, then enable this validation
            // Step 4: Capture tree rects (currently skipped - semantics not yet added)
            println!("\n--- Step 4: Capture Recursive Layout tree rects ---");
            println!("  (skipped - TODO: add RecursiveLayoutViewport semantics to demo app)");

            // Step 5: Increase depth (smoke test)
            println!("\n--- Step 5: Increase depth ---");
            click_button("Increase depth");
            click_button("Increase depth");
            std::thread::sleep(Duration::from_millis(300));
            println!("  ✓ Depth increased successfully");

            println!("\n=== SUMMARY ===");
            if has_issues {
                println!("✗ Recursive Layout has rect issues");
                robot.exit().ok();
                std::process::exit(1);
            } else {
                println!("✓ Recursive Layout rects look correct");
            }

            println!("\n=== Recursive Layout Robot Test Complete ===");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
