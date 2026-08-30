use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_core::rememberMutableStateOf;
use cranpose_testing::find_text_in_semantics;
use cranpose_ui::{
    composable, Box, BoxSpec, Button, ButtonSpec, Column, ColumnSpec, Modifier, Row, RowSpec, Text,
    TextStyle,
};

#[composable]
fn reproduction_app() {
    let toggle = rememberMutableStateOf(|| false);

    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Button(
            Modifier::empty(),
            ButtonSpec::default(),
            move || toggle.set(!toggle.get()),
            || {
                Text("Toggle Parent", Modifier::empty(), TextStyle::default());
            },
        );

        if toggle.get() {
            Row(Modifier::empty(), RowSpec::default(), || {
                Text("Persistent Child", Modifier::empty(), TextStyle::default());
            });
        } else {
            Box(Modifier::empty(), BoxSpec::default(), || {
                Text("Persistent Child", Modifier::empty(), TextStyle::default());
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
        .with_headless(true)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(500));

            if find_text_in_semantics(&robot, "Persistent Child").is_some() {
                println!("✓ Found child initially");
            } else {
                println!("✗ Child missing initially!");
                let _ = robot.exit();
            }

            let (tx, ty, tw, th) =
                cranpose_testing::find_button_in_semantics(&robot, "Toggle Parent")
                    .expect("Toggle button not found");

            println!("Clicking toggle...");
            robot.click(tx + tw / 2.0, ty + th / 2.0).ok();
            std::thread::sleep(Duration::from_millis(500));

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
                let _ = robot.exit();
            }

            println!("✓ Test Passed (No regression found?)");
            robot.exit().ok();
        })
        .run(reproduction_app);
}
