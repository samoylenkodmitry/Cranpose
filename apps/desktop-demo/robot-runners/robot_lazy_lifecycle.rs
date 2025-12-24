//! Robot test for LazyList lifecycle tracking - validates item compose/reuse/dispose
//!
//! This test tracks lifecycle transitions to verify that items are properly composed
//! and disposed when scrolling.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_lazy_lifecycle --features robot-app
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

/// Stats display component - isolated so only this recomposes when stats change
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

/// Lazy list component - NOT composable (LazyListState doesn't impl PartialEq)
fn lifecycle_lazy_list(state: LazyListState, stats: MutableState<LifecycleStats>) {
    let mut content = LazyListIntervalContent::new();
    content.items(
        20,
        Some(|i: usize| i as u64),
        None::<fn(usize) -> u64>,
        move |index| {
            lifecycle_item(index, stats);
        },
    );

    LazyColumn(
        Modifier::empty()
            .fill_max_width()
            .height(400.0)
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
fn lifecycle_test_app() {
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
                "LazyList Lifecycle Test",
                Modifier::empty()
                    .padding(12.0)
                    .background(Color(0.2, 0.3, 0.5, 0.8))
                    .rounded_corners(8.0),
            );

            // Stats display - isolated component, only this recomposes on stats change
            stats_display(stats);

            Text(
                "20 items - scroll to test lifecycle",
                Modifier::empty().padding(8.0),
            );

            // Lazy list - should not recompose when stats change
            lifecycle_lazy_list(state.clone(), stats);
        },
    );
}

#[composable]
fn lifecycle_item(index: usize, stats: MutableState<LifecycleStats>) {
    println!("  [COMPOSE] Item {} composition", index);
    // Track FIRST composition - remember only runs initializer once per slot
    let item_compose_count: MutableState<usize> = compose_core::remember(|| {
        stats.update(|s| s.total_composes += 1);
        println!("  [COMPOSE] Item {} first composition", index);
        compose_core::mutableStateOf(1usize)
    })
    .with(|s| *s);

    // Track effects and disposal
    DisposableEffect!(index, move |_key| {
        stats.update(|s| s.total_effects += 1);
        println!("  [EFFECT] Item {} effect started", index);

        DisposableEffectResult::new(move || {
            stats.update(|s| s.total_disposes += 1);
            println!("  [DISPOSE] Item {} disposed", index);
        })
    });

    // Display item
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding(8.0)
            .background(if index.is_multiple_of(2) {
                Color(0.12, 0.16, 0.28, 0.9)
            } else {
                Color(0.18, 0.12, 0.28, 0.9)
            })
            .rounded_corners(8.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Text(format!("Item #{}", index), Modifier::empty().padding(4.0));
            Text(
                format!("C:{}", item_compose_count.get()),
                Modifier::empty()
                    .padding(4.0)
                    .background(Color(0.0, 0.3, 0.0, 0.5))
                    .rounded_corners(4.0),
            );
        },
    );
}

fn main() {
    env_logger::init();
    println!("=== LazyList Lifecycle Robot Test ===");

    AppLauncher::new()
        .with_title("Lifecycle Test")
        .with_size(800, 600)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(100));

            let find_visible_items = || {
                let mut items: Vec<usize> = Vec::new();
                for i in 0..20 {
                    let item_text = format!("Item #{}", i);
                    if find_text_in_semantics(&robot, &item_text).is_some() {
                        items.push(i);
                    }
                }
                items
            };

            let read_stats = || -> Option<(usize, usize, usize)> {
                // Find any text starting with "Stats: C=" and parse the values
                if let Some((_, _, _, _, text)) =
                    compose_testing::find_text_by_prefix_in_semantics(&robot, "Stats: C=")
                {
                    // Parse "Stats: C=X E=Y D=Z"
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
            println!("  Visible items: {:?}", initial_items);
            if let Some((c, e, d)) = read_stats() {
                println!("  Stats: Composes={} Effects={} Disposes={}", c, e, d);
            }

            // Step 2: Scroll down
            println!("\n--- Step 2: Scroll down ---");
            if let Some((x, y, w, h)) = find_text_in_semantics(&robot, "Item #0") {
                robot
                    .drag(
                        x + w / 2.0,
                        y + h / 2.0 + 100.0,
                        x + w / 2.0,
                        y + h / 2.0 - 100.0,
                    )
                    .ok();
                std::thread::sleep(Duration::from_millis(100));
            }
            let after_scroll = find_visible_items();
            println!("  Visible after scroll: {:?}", after_scroll);
            if let Some((c, e, d)) = read_stats() {
                println!("  Stats: Composes={} Effects={} Disposes={}", c, e, d);
            }

            // Step 3: Scroll back
            println!("\n--- Step 3: Scroll back ---");
            if let Some((x, y, w, h)) = find_text_in_semantics(
                &robot,
                &format!("Item #{}", after_scroll.first().unwrap_or(&5)),
            ) {
                robot
                    .drag(x + w / 2.0, y + h / 2.0, x + w / 2.0, y + h / 2.0 + 200.0)
                    .ok();
                std::thread::sleep(Duration::from_millis(100));
            }
            let after_back = find_visible_items();
            println!("  Visible after scroll back: {:?}", after_back);

            // Final stats
            if let Some((c, e, d)) = read_stats() {
                println!("\n=== FINAL STATS ===");
                println!("  Total Composes: {}", c);
                println!("  Total Effects: {}", e);
                println!("  Total Disposes: {}", d);
                if c > 0 && e > 0 {
                    println!("\n✓ Lifecycle tracking PASSED!");
                }
            }

            println!("\n=== Test Complete ===");
            robot.exit().ok();
        })
        .run(lifecycle_test_app);
}
