//! Lazy List Demo Tab - demonstrates LazyColumn virtualization
//!
//! This module contains the lazy list demonstration for the desktop-demo app.

use compose_foundation::lazy::{LazyListIntervalContent, LazyListScope, LazyListState};
use compose_ui::widgets::{LazyColumn, LazyColumnSpec};
use compose_ui::{
    composable, Brush, Button, Color, Column, ColumnSpec, CornerRadii, LinearArrangement, Modifier, Row,
    RowSpec, Size, Spacer, Text, VerticalAlignment,
};

#[composable]
pub fn lazy_list_example() {
    // Create state using remember
    let list_state = compose_core::remember(LazyListState::new)
        .with(|s| s.clone());
    let item_count = compose_core::useState(|| 100usize);

    Column(
        Modifier::empty()
            .padding(32.0)
            .background(Color(0.08, 0.10, 0.18, 1.0))
            .rounded_corners(24.0)
            .padding(20.0),
        ColumnSpec::default(),
        move || {
            Text(
                "Lazy List Demo",
                Modifier::empty()
                    .padding(12.0)
                    .background(Color(1.0, 1.0, 1.0, 0.08))
                    .rounded_corners(16.0),
            );

            Spacer(Size { width: 0.0, height: 16.0 });

            // Show info
            let count = item_count.get();
            Text(
                format!("Virtualized list with {} items", count),
                Modifier::empty()
                    .padding(8.0)
                    .background(Color(0.2, 0.3, 0.4, 0.7))
                    .rounded_corners(12.0),
            );

            Spacer(Size { width: 0.0, height: 8.0 });

            // Stats from LazyListState
            // Note: on first frame, stats are 0 because measure happens after compose
            // Subsequent frames will show actual counts from measurement
            let stats = list_state.stats();
            let visible = if stats.items_in_use > 0 { stats.items_in_use } else { 7.min(count) };
            let cached = if stats.items_in_use > 0 { stats.items_in_pool } else { 7 };
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(16.0)),
                move || {
                    Text(
                        format!("{} visible", visible),
                        Modifier::empty()
                            .padding(8.0)
                            .background(Color(0.2, 0.5, 0.3, 0.8))
                            .rounded_corners(8.0),
                    );
                    Text(
                        format!("{} cached", cached),
                        Modifier::empty()
                            .padding(8.0)
                            .background(Color(0.5, 0.4, 0.2, 0.8))
                            .rounded_corners(8.0),
                    );
                },
            );

            Spacer(Size { width: 0.0, height: 16.0 });

            // Controls row
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
                move || {
                    Button(
                        Modifier::empty()
                            .rounded_corners(8.0)
                            .draw_behind(|scope| {
                                scope.draw_round_rect(
                                    Brush::solid(Color(0.2, 0.5, 0.3, 1.0)),
                                    CornerRadii::uniform(8.0),
                                );
                            })
                            .padding(10.0),
                        {
                            let count_state = item_count;
                            move || {
                                count_state.set(count_state.get().saturating_add(10));
                            }
                        },
                        || {
                            Text("Add 10 items", Modifier::empty().padding(4.0));
                        },
                    );

                    Button(
                        Modifier::empty()
                            .rounded_corners(8.0)
                            .draw_behind(|scope| {
                                scope.draw_round_rect(
                                    Brush::solid(Color(0.6, 0.2, 0.2, 1.0)),
                                    CornerRadii::uniform(8.0),
                                );
                            })
                            .padding(10.0),
                        {
                            let count_state = item_count;
                            move || {
                                count_state.set(count_state.get().saturating_sub(10).max(10));
                            }
                        },
                        || {
                            Text("Remove 10", Modifier::empty().padding(4.0));
                        },
                    );
                },
            );
            Spacer(Size { width: 0.0, height: 8.0 });

            // Extreme demo row
            let list_state_for_button = list_state.clone();
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
                move || {
                    // Set to MAX button
                    Button(
                        Modifier::empty()
                            .rounded_corners(8.0)
                            .draw_behind(|scope| {
                                scope.draw_round_rect(
                                    Brush::solid(Color(0.6, 0.3, 0.6, 1.0)),
                                    CornerRadii::uniform(8.0),
                                );
                            })
                            .padding(10.0),
                        {
                            let count_state = item_count;
                            move || {
                                count_state.set(usize::MAX);
                            }
                        },
                        || {
                            Text("Set usize::MAX", Modifier::empty().padding(4.0));
                        },
                    );

                    // Scroll to middle button
                    Button(
                        Modifier::empty()
                            .rounded_corners(8.0)
                            .draw_behind(|scope| {
                                scope.draw_round_rect(
                                    Brush::solid(Color(0.3, 0.4, 0.6, 1.0)),
                                    CornerRadii::uniform(8.0),
                                );
                            })
                            .padding(10.0),
                        {
                            let state = list_state_for_button.clone();
                            let count_state = item_count;
                            move || {
                                let count = count_state.get();
                                let middle = count / 2;
                                state.scroll_to_item(middle, 0.0);
                            }
                        },
                        || {
                            Text("Jump to Middle", Modifier::empty().padding(4.0));
                        },
                    );
                },
            );

            Spacer(Size { width: 0.0, height: 16.0 });

            // Build LazyColumn content
            let mut content = LazyListIntervalContent::new();
            let count = item_count.get();
            
            // Add items to lazy content
            content.items(
                count,
                None::<fn(usize) -> u64>,  // Auto-generate keys from index
                None::<fn(usize) -> u64>,  // Default content type
                move |i| {
                    let bg_color = if i % 2 == 0 {
                        Color(0.15, 0.18, 0.25, 1.0)
                    } else {
                        Color(0.12, 0.15, 0.22, 1.0)
                    };
                    
                    // Variable height based on index % 5 (48, 56, 64, 72, 80 pixels)
                    let item_height = 48.0 + (i % 5) as f32 * 8.0;
                    
                    Row(
                        Modifier::empty()
                            .fill_max_width()
                            .height(item_height)
                            .padding(12.0)
                            .background(bg_color)
                            .rounded_corners(8.0),
                        RowSpec::new()
                            .horizontal_arrangement(LinearArrangement::SpaceBetween)
                            .vertical_alignment(VerticalAlignment::CenterVertically),
                        move || {
                            Text(
                                format!("Item #{}", i),
                                Modifier::empty().padding(4.0),
                            );
                            Text(
                                format!("h: {:.0}px", item_height),
                                Modifier::empty()
                                    .padding(6.0)
                                    .background(Color(0.3, 0.3, 0.5, 0.5))
                                    .rounded_corners(6.0),
                            );
                        },
                    );
                },
            );

            // The actual LazyColumn with virtualization
            // LazyListState handles scroll internally (matching JC API)
            LazyColumn(
                Modifier::empty()
                    .fill_max_width()
                    .height(400.0)
                    .background(Color(0.06, 0.08, 0.14, 1.0))
                    .rounded_corners(12.0),
                list_state.clone(),
                LazyColumnSpec::new()
                    .vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                content,
            );
        },
    );
}
