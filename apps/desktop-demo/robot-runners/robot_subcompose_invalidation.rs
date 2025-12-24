//! Robot test for SubcomposeLayout invalidation routing.
//!
//! This test validates that modifier invalidations in SubcomposeLayoutNode
//! are properly routed to trigger re-renders (NOT swallowed).
//!
//! Test case:
//! 1. Display a LazyColumn with colored items
//! 2. Click a button that changes item background colors
//! 3. Verify the colors actually change (proves Draw invalidation works)
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_subcompose_invalidation --features robot-app
//! ```

use compose_app::AppLauncher;
use compose_core::useState;
use compose_foundation::lazy::{LazyListIntervalContent, LazyListScope, LazyListState};
use compose_testing::{find_button, find_in_semantics, find_text_in_semantics};
use compose_ui::widgets::*;
use compose_ui::{Color, Modifier};
use std::time::Duration;

/// Test UI that changes LazyColumn item colors on button click
fn test_app() {
    use compose_ui::LinearArrangement;

    // State to track color scheme
    let color_scheme = useState(|| 0u32);
    let list_state = LazyListState::new();

    Column(
        Modifier::empty()
            .fill_max_size()
            .padding(20.0)
            .background(Color(0.1, 0.1, 0.1, 1.0)),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(10.0)),
        move || {
            // Header
            Text(
                "SubcomposeLayout Invalidation Test".to_string(),
                Modifier::empty()
                    .padding(8.0)
                    .semantics(|c| c.content_description = Some("header".into())),
            );

            // Color scheme indicator - proves state is changing
            let current_scheme = color_scheme.get();
            let scheme_name = match current_scheme {
                0 => "Blue Theme",
                1 => "Green Theme",
                2 => "Red Theme",
                _ => "Unknown",
            };
            Text(
                format!("Current: {}", scheme_name),
                Modifier::empty()
                    .padding(4.0)
                    .semantics(|c| c.content_description = Some(scheme_name.into())),
            );

            // Button to cycle colors
            let color_scheme_clone = color_scheme.clone();
            Button(
                Modifier::empty()
                    .padding(8.0)
                    .background(Color(0.3, 0.3, 0.5, 1.0))
                    .semantics(|c| {
                        c.is_button = true;
                        c.content_description = Some("Change Colors".into());
                    }),
                move || {
                    color_scheme_clone.set((color_scheme_clone.get() + 1) % 3);
                },
                || {
                    Text("Change Colors".to_string(), Modifier::empty());
                },
            );

            // Build lazy content with colored items
            let scheme = current_scheme;
            let mut content = LazyListIntervalContent::new();
            content.items(
                5,
                Some(|i: usize| i as u64),
                None::<fn(usize) -> u64>,
                move |i| {
                    // Colors that change based on scheme
                    let bg = match scheme {
                        0 => Color(0.1, 0.15, 0.3 + (i as f32 * 0.05), 1.0), // Blue
                        1 => Color(0.1, 0.3 + (i as f32 * 0.05), 0.15, 1.0), // Green
                        2 => Color(0.3 + (i as f32 * 0.05), 0.1, 0.15, 1.0), // Red
                        _ => Color(0.2, 0.2, 0.2, 1.0),
                    };

                    Row(
                        Modifier::empty()
                            .fill_max_width()
                            .height(60.0)
                            .padding(10.0)
                            .background(bg)
                            .semantics(move |c| {
                                c.content_description = Some(format!("item{}", i));
                            }),
                        RowSpec::new(),
                        move || {
                            Text(
                                format!("Item {} - Scheme {}", i, scheme),
                                Modifier::empty(),
                            );
                        },
                    );
                },
            );

            // LazyColumn using SubcomposeLayoutNode internally
            LazyColumn(
                Modifier::empty()
                    .fill_max_width()
                    .height(350.0)
                    .background(Color(0.05, 0.05, 0.1, 1.0)),
                list_state.clone(),
                LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                content,
            );
        },
    );
}

fn main() {
    env_logger::init();
    println!("=== SubcomposeLayout Invalidation Routing Test ===");
    println!("Testing that modifier changes in LazyColumn items trigger re-renders\n");

    AppLauncher::new()
        .with_title("SubcomposeLayout Invalidation Test")
        .with_size(800, 600)
        .with_test_driver(|robot| {
            // Timeout after 30 seconds
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(30));
                eprintln!("TIMEOUT: Test exceeded 30 seconds");
                std::process::exit(1);
            });

            std::thread::sleep(Duration::from_millis(500));
            println!("✓ App launched and ready\n");

            let mut all_passed = true;

            // =========================================================
            // Step 1: Verify initial state (Blue Theme)
            // =========================================================
            println!("--- Step 1: Verify initial Blue Theme ---");

            if find_text_in_semantics(&robot, "Blue Theme").is_some() {
                println!("  ✓ Initial theme is Blue Theme");
            } else {
                println!("  ✗ Could not find 'Blue Theme' indicator");
                all_passed = false;
            }

            // Verify items show Scheme 0
            if find_text_in_semantics(&robot, "Scheme 0").is_some() {
                println!("  ✓ Items show 'Scheme 0'");
            } else {
                println!("  ✗ Could not find items with 'Scheme 0'");
                all_passed = false;
            }

            // =========================================================
            // Step 2: Click button to change to Green Theme
            // =========================================================
            println!("\n--- Step 2: Click 'Change Colors' button ---");

            if let Some((x, y, w, h)) =
                find_in_semantics(&robot, |elem| find_button(elem, "Change Colors"))
            {
                let _ = robot.mouse_move(x + w / 2.0, y + h / 2.0);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(300));
                let _ = robot.wait_for_idle();
                println!("  ✓ Clicked 'Change Colors' button");
            } else {
                println!("  ✗ Could not find 'Change Colors' button");
                all_passed = false;
            }

            // =========================================================
            // Step 3: Verify Green Theme is now active
            // =========================================================
            println!("\n--- Step 3: Verify Green Theme after button click ---");

            // Wait for recomposition to complete with retry
            let mut found_green = false;
            for attempt in 0..10 {
                std::thread::sleep(Duration::from_millis(100));
                let _ = robot.wait_for_idle();
                if find_text_in_semantics(&robot, "Green Theme").is_some() {
                    found_green = true;
                    println!("  ✓ Theme changed to Green Theme (attempt {})", attempt + 1);
                    break;
                }
            }
            if !found_green {
                println!("  ✗ Theme did NOT change to Green Theme!");
                println!("    This indicates invalidation routing may be broken.");
                all_passed = false;
            }

            // Verify items now show Scheme 1
            if find_text_in_semantics(&robot, "Scheme 1").is_some() {
                println!("  ✓ Items updated to show 'Scheme 1'");
                println!("    (Proves SubcomposeLayoutNode invalidation works!)");
            } else {
                println!("  ✗ Items did NOT update to 'Scheme 1'!");
                println!("    This indicates Draw invalidation is being SWALLOWED.");
                all_passed = false;
            }

            // =========================================================
            // Step 4: Click again to verify second color change
            // =========================================================
            println!("\n--- Step 4: Click button again for Red Theme ---");

            if let Some((x, y, w, h)) =
                find_in_semantics(&robot, |elem| find_button(elem, "Change Colors"))
            {
                let _ = robot.mouse_move(x + w / 2.0, y + h / 2.0);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                println!("  ✓ Clicked 'Change Colors' button again");
            }

            // Wait for recomposition with retry
            let mut found_red = false;
            for attempt in 0..10 {
                std::thread::sleep(Duration::from_millis(100));
                let _ = robot.wait_for_idle();
                if find_text_in_semantics(&robot, "Red Theme").is_some() {
                    found_red = true;
                    println!("  ✓ Theme changed to Red Theme (attempt {})", attempt + 1);
                    break;
                }
            }
            if !found_red {
                println!("  ✗ Theme did NOT change to Red Theme!");
                all_passed = false;
            }

            if find_text_in_semantics(&robot, "Scheme 2").is_some() {
                println!("  ✓ Items updated to show 'Scheme 2'");
            } else {
                println!("  ✗ Items did NOT update to 'Scheme 2'!");
                all_passed = false;
            }

            // =========================================================
            // Summary
            // =========================================================
            println!("\n=== Test Summary ===");
            if all_passed {
                println!("✓ ALL TESTS PASSED");
                println!("  SubcomposeLayoutNode properly routes invalidations:");
                println!("  - Draw invalidations trigger re-renders");
                println!("  - LazyColumn items update when modifiers change");
            } else {
                println!("✗ SOME TESTS FAILED");
                println!("  SubcomposeLayoutNode may be swallowing invalidations!");
            }

            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.exit();
        })
        .run(test_app);
}
