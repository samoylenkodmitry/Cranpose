//! Robot test for Recursive Layout tab - validates rects stay within the viewport.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_recursive_layout --features robot-app
//! ```

use cranpose_app::{AppLauncher, SemanticElement};
use cranpose_testing::{
    find_button_in_semantics, find_text_by_prefix_in_semantics, find_text_in_semantics,
};
use desktop_app::app;
use std::time::Duration;

const WINDOW_WIDTH: f32 = 1200.0;
const WINDOW_HEIGHT: f32 = 800.0;
const VIEWPORT_TAG: &str = "RecursiveLayoutViewport";

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

fn collect_descendants<'a>(elem: &'a SemanticElement, results: &mut Vec<&'a SemanticElement>) {
    for child in &elem.children {
        results.push(child);
        collect_descendants(child, results);
    }
}

fn validate_within_bounds(
    elements: &[&SemanticElement],
    bounds: (f32, f32, f32, f32),
    issues: &mut Vec<String>,
) {
    let (bx, by, bw, bh) = bounds;
    let tolerance = 1.0;

    for elem in elements {
        let b = &elem.bounds;
        let text = elem.text.as_deref().unwrap_or("");

        if !b.x.is_finite() || !b.y.is_finite() || !b.width.is_finite() || !b.height.is_finite() {
            issues.push(format!(
                "Non-finite bounds role={} text=\"{}\"",
                elem.role, text
            ));
            continue;
        }

        if b.width < 0.0 || b.height < 0.0 {
            issues.push(format!(
                "Negative size role={} text=\"{}\" ({:.1} x {:.1})",
                elem.role, text, b.width, b.height
            ));
        }

        if !text.is_empty() && (b.width < 1.0 || b.height < 1.0) {
            issues.push(format!(
                "Zero-size content role={} text=\"{}\" ({:.1} x {:.1})",
                elem.role, text, b.width, b.height
            ));
        }

        let right = b.x + b.width;
        let bottom = b.y + b.height;
        if b.x < bx - tolerance
            || b.y < by - tolerance
            || right > bx + bw + tolerance
            || bottom > by + bh + tolerance
        {
            issues.push(format!(
                "Out of viewport role={} text=\"{}\" ({:.1},{:.1},{:.1},{:.1})",
                elem.role, text, b.x, b.y, b.width, b.height
            ));
        }
    }
}

fn print_semantics_with_bounds(elem: &SemanticElement, indent: usize) {
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
    for child in &elem.children {
        print_semantics_with_bounds(child, indent + 1);
    }
}

fn validate_recursive_layout(robot: &cranpose_app::Robot, label: &str) -> Vec<String> {
    let mut issues = Vec::new();
    let Ok(semantics) = robot.get_semantics() else {
        return vec!["Failed to fetch semantics".to_string()];
    };

    let viewport = find_element_by_text(&semantics, VIEWPORT_TAG);
    let Some(viewport) = viewport else {
        return vec![format!("Missing viewport tag: {VIEWPORT_TAG}")];
    };

    let viewport_bounds = (
        viewport.bounds.x,
        viewport.bounds.y,
        viewport.bounds.width,
        viewport.bounds.height,
    );

    if viewport_bounds.2 < 1.0 || viewport_bounds.3 < 1.0 {
        issues.push(format!(
            "Viewport has invalid size ({:.1} x {:.1})",
            viewport_bounds.2, viewport_bounds.3
        ));
    }

    let mut descendants = Vec::new();
    collect_descendants(viewport, &mut descendants);
    validate_within_bounds(&descendants, viewport_bounds, &mut issues);

    if !issues.is_empty() {
        println!("\n--- {label}: Semantics Tree ---");
        for elem in &semantics {
            print_semantics_with_bounds(elem, 0);
        }
    }

    issues
}

fn main() {
    env_logger::init();
    println!("=== Recursive Layout Robot Test (rect validation) ===");

    AppLauncher::new()
        .with_title("Recursive Layout Test")
        .with_size(WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
        .with_headless(true)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(400));

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

            println!("\n--- Step 1: Navigate to 'Recursive Layout' tab ---");
            if !click_button("Recursive Layout") {
                println!("FATAL: Could not find 'Recursive Layout' tab button");
                robot.exit().ok();
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(400));

            println!("\n--- Step 2: Verify Recursive Layout header + controls ---");
            if find_text_in_semantics(&robot, "Recursive Layout Playground").is_none() {
                println!("  ✗ Missing Recursive Layout header");
                robot.exit().ok();
                std::process::exit(1);
            }

            let mut issues = Vec::new();
            if find_button_in_semantics(&robot, "Increase depth").is_none() {
                issues.push("Missing Increase depth button".to_string());
            }
            if find_button_in_semantics(&robot, "Decrease depth").is_none() {
                issues.push("Missing Decrease depth button".to_string());
            }
            if find_text_by_prefix_in_semantics(&robot, "Current depth:").is_none() {
                issues.push("Missing Current depth label".to_string());
            }

            println!("\n--- Step 3: Increase depth to 5 ---");
            click_button("Increase depth");
            click_button("Increase depth");
            std::thread::sleep(Duration::from_millis(250));

            issues.extend(validate_recursive_layout(&robot, "Depth 5"));

            if !issues.is_empty() {
                println!("\n=== FAILURE ===");
                for issue in &issues {
                    println!("✗ {issue}");
                }
                robot.exit().ok();
                std::process::exit(1);
            }

            println!("\n=== SUCCESS ===");
            println!("✓ Recursive Layout rects are within viewport");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
