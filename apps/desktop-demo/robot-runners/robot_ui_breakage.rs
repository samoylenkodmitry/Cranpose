//! Reproduction test for UI breakage during parent recreation
//!
//! This test demonstrates a regression where reusing a child node when its parent
//! is recreated leads to the child being destroyed (orphaned) because the
//! parent's removal recursively removes children.
//!
//! Scenario:
//! 1. Render `Column -> Box -> Text("Persistent")`.
//! 2. Toggle state to change `Box` to `Row` (or just force recreation).
//!    Structure becomes `Column -> Row -> Text("Persistent")`.
//! 3. Verify `Text("Persistent")` is still visible and has valid bounds.

use compose_core::useState;
use compose_ui::{
    composable, Box, BoxSpec, Button, Column, ColumnSpec, Modifier, Row, RowSpec, Text,
};
// use desktop_app::app;
use compose_app::AppLauncher;
use compose_testing::find_text_in_semantics;
use std::time::Duration;

#[composable]
fn reproduction_app() {
    let toggle = useState(|| false);

    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Button(
            Modifier::empty(),
            move || toggle.set(!toggle.get()),
            || {
                Text("Toggle Parent", Modifier::empty());
            },
        );

        if toggle.get() {
            // State B: Row parent
            Row(Modifier::empty(), RowSpec::default(), || {
                Text("Persistent Child", Modifier::empty());
            });
        } else {
            // State A: Box parent
            Box(Modifier::empty(), BoxSpec::default(), || {
                Text("Persistent Child", Modifier::empty());
            });
        }
    });
}

fn main() {
    env_logger::init();
    println!("=== Robot UI Breakage Reproduction ===");

    AppLauncher::new()
        .with_title("UI Breakage Repro")
        .with_size(400, 300)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(500));

            // Initial check
            if find_text_in_semantics(&robot, "Persistent Child").is_some() {
                println!("✓ Found child initially");
            } else {
                println!("✗ Child missing initially!");
                let _ = robot.exit();
            }

            // Toggle to trigger parent change
            // Find toggle button
            let (tx, ty, tw, th) =
                compose_testing::find_button_in_semantics(&robot, "Toggle Parent")
                    .expect("Toggle button not found");

            println!("Clicking toggle...");
            robot.click(tx + tw / 2.0, ty + th / 2.0).ok();
            std::thread::sleep(Duration::from_millis(500));

            // Check if child survived
            if let Some((x, y, w, h)) = find_text_in_semantics(&robot, "Persistent Child") {
                println!(
                    "✓ Child survived parent recreation! Bounds: {:.1},{:.1} {}x{}",
                    x, y, w, h
                );
                if w <= 0.0 || h <= 0.0 {
                    println!("✗ Child has zero size - Layout broken!");
                    let _ = robot.exit();
                }
            } else {
                println!("✗ Child DISAPPEARED after parent recreation!");
                println!("  This confirms the regression: 'UI breaks when going between tabs'");
                let _ = robot.exit(); // Fail
            }

            println!("✓ Test Passed (No regression found?)");
            robot.exit().ok();
        })
        .run(reproduction_app);
}
