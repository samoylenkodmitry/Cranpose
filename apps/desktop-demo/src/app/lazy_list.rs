//! Lazy List Demo Tab - demonstrates LazyColumn virtualization
//!
//! This module contains the lazy list demonstration for the desktop-demo app.

use compose_core::{DisposableEffect, DisposableEffectResult, MutableState};
use compose_foundation::lazy::{remember_lazy_list_state, LazyListScope};
use compose_foundation::SemanticsConfiguration;
use compose_ui::widgets::{LazyColumn, LazyColumnSpec};
use compose_ui::{
    composable, Brush, Button, Color, Column, ColumnSpec, CornerRadii, LinearArrangement, Modifier,
    Row, RowSpec, Size, Spacer, Text, VerticalAlignment,
};

#[derive(Clone, Default, PartialEq)]
struct LifecycleStats {
    total_composes: usize,
    total_effects: usize,
    total_disposes: usize,
}

fn item_height(index: usize) -> f32 {
    48.0 + (index % 5) as f32 * 8.0
}

fn item_background(index: usize) -> Color {
    if index.is_multiple_of(2) {
        Color(0.15, 0.18, 0.25, 1.0)
    } else {
        Color(0.12, 0.15, 0.22, 1.0)
    }
}

#[allow(non_snake_case)]
#[composable]
fn LifecycleStatsDisplay(stats: MutableState<LifecycleStats>) {
    let current = stats.get();
    Text(
        format!(
            "Lifecycle totals: C={} E={} D={}",
            current.total_composes, current.total_effects, current.total_disposes
        ),
        Modifier::empty()
            .padding(8.0)
            .background(Color(0.0, 0.4, 0.2, 0.8))
            .rounded_corners(8.0),
    );
}

/// Displays the visible and cached item counts from LazyListState.
/// This is in its own composable scope to isolate stats() reactivity.
/// The reactive read happens INSIDE this function, not at the call site.
#[allow(non_snake_case)]
#[composable]
fn LazyListStatsDisplay(list_state: compose_foundation::lazy::LazyListState) {
    // Reactive read happens here - isolated from parent scope
    let stats = list_state.stats();
    let visible = stats.items_in_use;
    let cached = stats.items_in_pool;
    println!("Compose stats text, visible={visible}");

    Row(
        Modifier::empty(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(16.0)),
        move || {
            println!("Row content closure running, visible={visible}");
            Text(
                format!("Visible: {}", visible),
                Modifier::empty()
                    .padding(8.0)
                    .background(Color(0.2, 0.5, 0.3, 0.8))
                    .rounded_corners(8.0),
            );
            Text(
                format!("Cached: {}", cached),
                Modifier::empty()
                    .padding(8.0)
                    .background(Color(0.5, 0.4, 0.2, 0.8))
                    .rounded_corners(8.0),
            );
        },
    );
}

/// Displays the first visible item index from LazyListState.
/// This is in its own composable scope to isolate first_visible_item_index() reactivity.
/// The reactive read happens INSIDE this function, not at the call site.
#[allow(non_snake_case)]
#[composable]
fn FirstVisibleIndexDisplay(list_state: compose_foundation::lazy::LazyListState) {
    // Reactive read happens here - isolated from parent scope
    let first_index = list_state.first_visible_item_index();
    println!("Compose ISOLATED FirstIndex text: {}", first_index);
    Text(
        format!("FirstIndex: {}", first_index),
        Modifier::empty()
            .padding(8.0)
            .background(Color(0.4, 0.3, 0.5, 0.8))
            .rounded_corners(8.0),
    );
}

#[allow(non_snake_case)]
#[composable]
fn LifecycleListItem(index: usize, stats: MutableState<LifecycleStats>) {
    println!("Compose item id={index}");
    let stats_for_compose = stats;
    compose_core::remember(move || {
        stats_for_compose.update(|current| current.total_composes += 1);
    })
    .with(|_| ());

    let stats_for_effect = stats;
    DisposableEffect!(index, move |_key| {
        stats_for_effect.update(|current| current.total_effects += 1);
        let stats_for_dispose = stats_for_effect;
        DisposableEffectResult::new(move || {
            println!("Dispose item id={index}");
            stats_for_dispose.update(|current| current.total_disposes += 1);
        })
    });

    let item_height = item_height(index);
    let bg_color = item_background(index);
    let item_label = format!("Item #{}", index);
    let item_label_for_semantics = format!("ItemRow #{}", index);

    Row(
        Modifier::empty()
            .semantics(move |config: &mut SemanticsConfiguration| {
                config.content_description = Some(item_label_for_semantics.clone());
            })
            .fill_max_width()
            .height(item_height)
            .padding(12.0)
            .background(bg_color)
            .rounded_corners(8.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpaceBetween)
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Text(item_label.clone(), Modifier::empty().padding(4.0));

            // Add i%5 colored boxes to visualize content type groups
            let box_count = index % 5;
            Row(
                Modifier::empty(),
                RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(4.0)),
                move || {
                    let colors = [
                        Color(0.9, 0.3, 0.3, 1.0), // Red
                        Color(0.3, 0.9, 0.3, 1.0), // Green
                        Color(0.3, 0.3, 0.9, 1.0), // Blue
                        Color(0.9, 0.9, 0.3, 1.0), // Yellow
                        Color(0.9, 0.3, 0.9, 1.0), // Magenta
                    ];
                    for i in 0..box_count {
                        Spacer(Size {
                            width: 12.0,
                            height: 12.0,
                        });
                        // Color each box based on its position
                        let color = colors[i % colors.len()];
                        Text(
                            "■",
                            Modifier::empty()
                                .background(color)
                                .rounded_corners(2.0)
                                .padding(2.0),
                        );
                    }
                },
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
}

#[composable]
pub fn lazy_list_example() {
    let list_state = remember_lazy_list_state();
    let item_count = compose_core::useState(|| 100usize);
    let lifecycle_stats = compose_core::useState(LifecycleStats::default);

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

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            // Show info
            let count = item_count.get();
            Text(
                format!("Virtualized list with {} items", count),
                Modifier::empty()
                    .padding(8.0)
                    .background(Color(0.2, 0.3, 0.4, 0.7))
                    .rounded_corners(12.0),
            );

            Spacer(Size {
                width: 0.0,
                height: 8.0,
            });

            LifecycleStatsDisplay(lifecycle_stats);

            Spacer(Size {
                width: 0.0,
                height: 8.0,
            });

            // Stats from LazyListState - in its own isolated composable scope
            // Reactive read happens INSIDE LazyListStatsDisplay, not here
            LazyListStatsDisplay(list_state);

            Spacer(Size {
                width: 0.0,
                height: 8.0,
            });

            // First visible item index - in its own isolated composable scope
            // Reactive read happens INSIDE FirstVisibleIndexDisplay, not here
            FirstVisibleIndexDisplay(list_state);

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

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
            Spacer(Size {
                width: 0.0,
                height: 8.0,
            });

            // Extreme demo row
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
                            let count_state = item_count;
                            move || {
                                let count = count_state.get();
                                let middle = count / 2;
                                list_state.scroll_to_item(middle, 0.0);
                            }
                        },
                        || {
                            Text("Jump to Middle", Modifier::empty().padding(4.0));
                        },
                    );

                    // Jump to Start button
                    Button(
                        Modifier::empty()
                            .rounded_corners(8.0)
                            .draw_behind(|scope| {
                                scope.draw_round_rect(
                                    Brush::solid(Color(0.2, 0.5, 0.5, 1.0)),
                                    CornerRadii::uniform(8.0),
                                );
                            })
                            .padding(10.0),
                        {
                            move || {
                                list_state.scroll_to_item(0, 0.0);
                            }
                        },
                        || {
                            Text("⏫ Start", Modifier::empty().padding(4.0));
                        },
                    );

                    // Jump to End button
                    Button(
                        Modifier::empty()
                            .rounded_corners(8.0)
                            .draw_behind(|scope| {
                                scope.draw_round_rect(
                                    Brush::solid(Color(0.5, 0.4, 0.2, 1.0)),
                                    CornerRadii::uniform(8.0),
                                );
                            })
                            .padding(10.0),
                        {
                            let count_state = item_count;
                            move || {
                                let count = count_state.get();
                                if count > 0 {
                                    list_state.scroll_to_item(count - 1, 0.0);
                                }
                            }
                        },
                        || {
                            Text("⏬ End", Modifier::empty().padding(4.0));
                        },
                    );
                },
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            // The actual LazyColumn with virtualization using the DSL
            let count = item_count.get();
            LazyColumn(
                Modifier::empty()
                    .semantics(|config: &mut SemanticsConfiguration| {
                        config.content_description = Some("LazyListViewport".to_string());
                    })
                    .fill_max_width()
                    .height(400.0)
                    .background(Color(0.06, 0.08, 0.14, 1.0))
                    .rounded_corners(12.0),
                list_state,
                LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                |scope| {
                    scope.items(
                        count,
                        None::<fn(usize) -> u64>, // Auto-generate keys from index
                        // Content type = index % 5 to match height groups
                        Some(|index: usize| (index % 5) as u64),
                        move |index| {
                            LifecycleListItem(index, lifecycle_stats);
                            Text(
                                format!("Hello #{}", index),
                                Modifier::empty()
                                    .padding(8.0)
                                    .background(Color(0.3, 0.3, 0.4, 0.4))
                                    .rounded_corners(8.0),
                            );
                        },
                    );
                },
            );
        },
    );
}
