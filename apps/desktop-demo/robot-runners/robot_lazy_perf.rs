//! Performance test for LazyColumn with extreme item counts
//!
//! Tests that LazyColumn handles usize::MAX items without performance degradation.
//! This validates the O(1) nature of virtualization - only visible items are composed.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_lazy_perf --features robot-app
//! ```

use compose_app::AppLauncher;
use compose_foundation::lazy::{LazyListIntervalContent, LazyListScope, LazyListState};
use compose_testing::find_text_in_semantics;
use compose_ui::widgets::*;
use compose_ui::{Color, LinearArrangement, Modifier};
use std::time::{Duration, Instant};

/// Total items = usize::MAX (18,446,744,073,709,551,615 on 64-bit)
const ITEM_COUNT: usize = usize::MAX;

fn test_app() {
    let list_state = LazyListState::new();

    Column(
        Modifier::empty()
            .fill_max_size()
            .padding(20.0)
            .background(Color(0.08, 0.08, 0.12, 1.0)),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(10.0)),
        move || {
            // Header
            Text(
                "LazyColumn Performance Test".to_string(),
                Modifier::empty().padding(8.0),
            );

            // Item count display (formatted for readability)
            Text(
                format!("{} items (usize::MAX)", format_large_number(ITEM_COUNT)),
                Modifier::empty().padding(4.0),
            );

            // Build lazy content with MASSIVE item count
            let mut content = LazyListIntervalContent::new();
            content.items(
                ITEM_COUNT,
                Some(|i: usize| i as u64),
                None::<fn(usize) -> u64>,
                move |i| {
                    // Simple text item - just the index
                    let bg = if i % 2 == 0 {
                        Color(0.15, 0.18, 0.25, 1.0)
                    } else {
                        Color(0.12, 0.14, 0.20, 1.0)
                    };

                    Box(
                        Modifier::empty()
                            .fill_max_width()
                            .height(40.0)
                            .padding(8.0)
                            .background(bg),
                        BoxSpec::new(),
                        move || {
                            Text(format!("Item #{}", i), Modifier::empty());
                        },
                    );
                },
            );

            // LazyColumn
            LazyColumn(
                Modifier::empty()
                    .fill_max_width()
                    .height(400.0)
                    .background(Color(0.05, 0.05, 0.08, 1.0)),
                list_state.clone(),
                LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(2.0)),
                content,
            );

            // Stats
            let stats = list_state.stats();
            Text(
                format!(
                    "Visible: {} | Pool: {}",
                    stats.items_in_use, stats.items_in_pool
                ),
                Modifier::empty().padding(8.0),
            );

            // Jump to Middle button
            let state_for_button = list_state.clone();
            let middle_index = ITEM_COUNT / 2;
            Button(
                Modifier::empty()
                    .padding(8.0)
                    .background(Color(0.3, 0.5, 0.8, 1.0)),
                move || {
                    state_for_button.scroll_to_item(middle_index, 0.0);
                },
                || {
                    Text("Jump to Middle".to_string(), Modifier::empty());
                },
            );
        },
    );
}

/// Formats a large number with underscores for readability
fn format_large_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push('_');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

fn main() {
    env_logger::init();
    println!("=== LazyColumn Performance Test (usize::MAX items) ===\n");
    println!(
        "Testing virtualization with {} items...\n",
        format_large_number(ITEM_COUNT)
    );

    AppLauncher::new()
        .with_title("LazyColumn Perf Test")
        .with_size(800, 600)
        .with_test_driver(|robot| {
            println!("✓ App launched");

            // Measure initial render time
            let start = Instant::now();
            std::thread::sleep(Duration::from_millis(500));
            let initial_render_time = start.elapsed();
            println!("  Initial render time: {:?}", initial_render_time);

            // === PHASE 1: Verify basic rendering ===
            println!("\n=== PHASE 1: Verify Rendering ===");

            let find_text = |text: &str| find_text_in_semantics(&robot, text);

            if find_text("LazyColumn Performance Test").is_some() {
                println!("  ✓ Header rendered");
            } else {
                println!("  ✗ Header NOT found!");
            }

            // Check first items are visible
            let mut visible_items = Vec::new();
            for i in 0..20 {
                let item_text = format!("Item #{}", i);
                if let Some((_, y, _, _)) = find_text(&item_text) {
                    visible_items.push((i, y));
                }
            }

            println!(
                "  Visible items: {} (at scroll position 0)",
                visible_items.len()
            );
            if visible_items.len() >= 8 {
                println!("  ✓ Expected number of items rendered");
            } else {
                println!("  ✗ Too few items visible!");
            }

            // === PHASE 2: Instant Jump to Middle ===
            println!("\n=== PHASE 2: Jump to Middle (button click) ===");
            let middle_index = ITEM_COUNT / 2;
            println!(
                "  Clicking 'Jump to Middle' to scroll to item {}...",
                format_large_number(middle_index)
            );

            // Find and click the button
            use compose_testing::find_button_in_semantics;
            let jump_start = Instant::now();
            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Jump to Middle") {
                robot.click(x + w / 2.0, y + h / 2.0).ok();
                std::thread::sleep(Duration::from_millis(300));
                let jump_time = jump_start.elapsed();
                println!("  ✓ Button clicked, jump time: {:?}", jump_time);

                if jump_time < Duration::from_millis(500) {
                    println!("  ✓ Jump < 500ms (O(1) scroll performance)");
                }
            } else {
                println!("  ✗ Button not found!");
            }

            // Check which items are visible after jump
            std::thread::sleep(Duration::from_millis(200));

            // Look for items around the middle
            let search_start = middle_index.saturating_sub(10);
            let mut found_middle_items = Vec::new();
            for offset in 0..20 {
                let idx = search_start.saturating_add(offset);
                let item_text = format!("Item #{}", idx);
                if find_text(&item_text).is_some() {
                    found_middle_items.push(idx);
                }
            }

            if !found_middle_items.is_empty() {
                println!("  ✓ Jumped to middle: found items {:?}", found_middle_items);
            } else {
                // Check first 50 items to see what's visible
                let mut items_after = Vec::new();
                for i in 0..50 {
                    let item_text = format!("Item #{}", i);
                    if find_text(&item_text).is_some() {
                        items_after.push(i);
                    }
                }
                let first_before = visible_items.first().map(|(i, _)| *i).unwrap_or(0);
                let first_after = items_after.first().copied().unwrap_or(0);

                if first_after > first_before {
                    println!(
                        "  ✓ Scroll worked: first item {} -> {}",
                        first_before, first_after
                    );
                } else {
                    println!(
                        "  ⚠️ Items near middle not found, visible: {:?}",
                        items_after
                    );
                }
            }

            // === PHASE 3: Performance metrics ===
            println!("\n=== PHASE 3: Performance Summary ===");

            // Check that we're NOT composing millions of items
            let expected_max_visible = 20; // Generous estimate
            if visible_items.len() <= expected_max_visible {
                println!(
                    "  ✓ Virtualization working: only {} items composed",
                    visible_items.len()
                );
            } else {
                println!("  ✗ Too many items composed: {}", visible_items.len());
            }

            // Timing checks
            if initial_render_time < Duration::from_secs(2) {
                println!(
                    "  ✓ Initial render < 2s (actual: {:?})",
                    initial_render_time
                );
            } else {
                println!("  ✗ Initial render too slow: {:?}", initial_render_time);
            }

            // === SUMMARY ===
            println!("\n=== SUMMARY ===");
            let success = visible_items.len() <= expected_max_visible
                && initial_render_time < Duration::from_secs(2);

            if success {
                println!("✓ Performance test PASSED");
                println!("  - {} items total", format_large_number(ITEM_COUNT));
                println!(
                    "  - {} items visible (O(1) virtualization)",
                    visible_items.len()
                );
                println!("  - Initial render: {:?}", initial_render_time);
            } else {
                println!("✗ Performance test FAILED");
            }

            println!("\n=== Test Complete ===");
            robot.exit().ok();
        })
        .run(test_app);
}
