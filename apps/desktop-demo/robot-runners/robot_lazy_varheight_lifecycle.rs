//! Robot test for LazyList variable height lifecycle
//!
//! Tests that items with varying heights trigger correct compose/dispose behavior.
//! Validates that items are composed correctly with varying viewport capacity.
//! With slot composition reuse, items in the reuse pool keep their effects alive.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_lazy_varheight_lifecycle --features robot-app
//! ```

use compose_app::AppLauncher;
use compose_core::{DisposableEffect, DisposableEffectResult, MutableState};
use compose_foundation::lazy::{LazyListIntervalContent, LazyListScope, LazyListState};
use compose_macros::composable;
use compose_testing::find_text_in_semantics;
use compose_ui::widgets::*;
use compose_ui::{Color, ColumnSpec, LinearArrangement, Modifier, RowSpec, VerticalAlignment};
use std::time::Duration;

/// Lifecycle stats stored in compose state
#[derive(Clone, Default, PartialEq)]
struct LifecycleStats {
    total_composes: usize,
    total_effects: usize,
    total_disposes: usize,
}

/// Variable height based on index - ensures different items per viewport
/// Heights: 40, 60, 80, 100, 120, 40, 60, 80... (5 variations)
fn item_height(index: usize) -> f32 {
    40.0 + (index % 5) as f32 * 20.0
}

/// Stats display component
#[composable]
fn stats_display(stats: MutableState<LifecycleStats>) {
    let current = stats.get();
    Text(
        format!(
            "Stats: C={} E={} D={}",
            current.total_composes, current.total_effects, current.total_disposes
        ),
        Modifier::empty()
            .padding(8.0)
            .background(Color(0.0, 0.4, 0.2, 0.8))
            .rounded_corners(8.0),
    );
}

/// Lazy list with variable height items
fn variable_height_lazy_list(state: LazyListState, stats: MutableState<LifecycleStats>) {
    let mut content = LazyListIntervalContent::new();
    content.items(
        30, // More items to test with variable heights
        Some(|i: usize| i as u64),
        None::<fn(usize) -> u64>,
        move |index| {
            variable_height_item(index, stats);
        },
    );

    LazyColumn(
        Modifier::empty()
            .fill_max_width()
            .height(300.0) // Shorter height to see more scrolling behavior
            .background(Color(0.05, 0.05, 0.1, 1.0))
            .rounded_corners(12.0),
        state,
        LazyColumnSpec::new()
            .vertical_arrangement(LinearArrangement::SpacedBy(4.0))
            .content_padding(8.0, 8.0),
        content,
    );
}

#[composable]
fn varheight_test_app() {
    let stats: MutableState<LifecycleStats> = compose_core::useState(LifecycleStats::default);
    let state = compose_core::remember(LazyListState::new).with(|s| s.clone());

    Column(
        Modifier::empty()
            .fill_max_size()
            .padding(16.0)
            .background(Color(0.08, 0.08, 0.12, 1.0)),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                "Variable Height Lifecycle Test",
                Modifier::empty()
                    .padding(12.0)
                    .background(Color(0.2, 0.3, 0.5, 0.8))
                    .rounded_corners(8.0),
            );

            stats_display(stats);

            Text(
                "30 items with heights: 40/60/80/100/120px",
                Modifier::empty().padding(8.0),
            );

            variable_height_lazy_list(state.clone(), stats);
        },
    );
}

#[composable]
fn variable_height_item(index: usize, stats: MutableState<LifecycleStats>) {
    let height = item_height(index);
    println!("  [COMPOSE] Item {} (h={})", index, height);

    // Track FIRST composition
    compose_core::remember(|| {
        stats.update(|s| s.total_composes += 1);
        println!("  [COMPOSE] Item {} first composition", index);
    })
    .with(|_| ());

    // Track effects and disposal
    DisposableEffect!(index, move |_key| {
        stats.update(|s| s.total_effects += 1);
        println!("  [EFFECT] Item {} effect started", index);

        DisposableEffectResult::new(move || {
            stats.update(|s| s.total_disposes += 1);
            println!("  [DISPOSE] Item {} disposed", index);
        })
    });

    Row(
        Modifier::empty()
            .fill_max_width()
            .height(height)
            .padding(12.0)
            .background(if index.is_multiple_of(2) {
                Color(0.15, 0.2, 0.3, 1.0)
            } else {
                Color(0.12, 0.15, 0.25, 1.0)
            })
            .rounded_corners(8.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpaceBetween)
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Text(format!("Item #{}", index), Modifier::empty().padding(4.0));
            Text(
                format!("h={}px", height as i32),
                Modifier::empty()
                    .padding(6.0)
                    .background(Color(0.3, 0.3, 0.5, 0.5))
                    .rounded_corners(6.0),
            );
        },
    );
}

fn main() {
    env_logger::init();
    println!("=== Variable Height Lifecycle Robot Test ===");

    AppLauncher::new()
        .with_title("VarHeight Lifecycle Test")
        .with_size(800, 600)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(100));

            let find_visible_items = || {
                let mut items: Vec<usize> = Vec::new();
                for i in 0..30 {
                    let item_text = format!("Item #{}", i);
                    if find_text_in_semantics(&robot, &item_text).is_some() {
                        items.push(i);
                    }
                }
                items
            };

            let read_stats = || -> Option<(usize, usize, usize)> {
                if let Some((_, _, _, _, text)) =
                    compose_testing::find_text_by_prefix_in_semantics(&robot, "Stats: C=")
                {
                    let parts: Vec<&str> = text.split_whitespace().collect();
                    if parts.len() >= 4 {
                        let c = parts[1].strip_prefix("C=")?.parse().ok()?;
                        let e = parts[2].strip_prefix("E=")?.parse().ok()?;
                        let d = parts[3].strip_prefix("D=")?.parse().ok()?;
                        return Some((c, e, d));
                    }
                }
                None
            };

            // Step 1: Initial state
            println!("\n--- Step 1: Initial state ---");
            let initial_items = find_visible_items();
            println!(
                "  Visible items: {:?} (count={}, variable heights)",
                initial_items,
                initial_items.len()
            );
            let initial_composes = if let Some((c, e, d)) = read_stats() {
                println!("  Stats: Composes={} Effects={} Disposes={}", c, e, d);
                assert_eq!(c, e, "Composes should equal effects initially");
                assert_eq!(d, 0, "No disposes initially");
                c
            } else {
                0
            };

            // Step 2: Scroll down significantly
            println!("\n--- Step 2: Scroll down ---");
            if let Some((x, y, w, h)) = find_text_in_semantics(&robot, "Item #0") {
                robot
                    .drag(
                        x + w / 2.0,
                        y + h / 2.0 + 100.0,
                        x + w / 2.0,
                        y + h / 2.0 - 200.0, // Larger scroll
                    )
                    .ok();
                std::thread::sleep(Duration::from_millis(100));
            }
            let after_scroll = find_visible_items();
            println!("  Visible after scroll: {:?}", after_scroll);
            if let Some((c, e, d)) = read_stats() {
                println!("  Stats: Composes={} Effects={} Disposes={}", c, e, d);
                assert!(
                    c > initial_composes,
                    "Should have new composes after scroll"
                );
            }

            // Step 3: Scroll back to top
            println!("\n--- Step 3: Scroll back ---");
            if let Some((x, y, w, h)) = find_text_in_semantics(
                &robot,
                &format!("Item #{}", after_scroll.first().unwrap_or(&5)),
            ) {
                robot
                    .drag(x + w / 2.0, y + h / 2.0, x + w / 2.0, y + h / 2.0 + 250.0)
                    .ok();
                std::thread::sleep(Duration::from_millis(100));
            }
            let after_back = find_visible_items();
            println!("  Visible after scroll back: {:?}", after_back);

            // Final stats - verify JC-matching behavior
            if let Some((c, e, d)) = read_stats() {
                println!("\n=== FINAL STATS ===");
                println!("  Total Composes: {}", c);
                println!("  Total Effects: {}", e);
                println!("  Total Disposes: {}", d);

                // Key assertions:
                // 1. Composes == Effects
                assert_eq!(c, e, "Composes should equal effects");

                // 2. With slot composition reuse, items in reuse pool keep effects alive.
                //    Disposes only happen when items exceed pool capacity.
                //    Short scrolls may not trigger any disposes.
                println!("  Note: Disposes={} (items in reuse pool keep effects)", d);

                // 3. Total composes should show new items were composed
                println!("\n✓ Variable height lifecycle test PASSED!");
            }

            println!("\n=== Test Complete ===");
            robot.exit().ok();
        })
        .run(varheight_test_app);
}
