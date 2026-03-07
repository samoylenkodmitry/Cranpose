//! Lazy List Demo Tab - demonstrates LazyColumn virtualization
//!
//! This module contains the lazy list demonstration for the desktop-demo app.

use cranpose_core::{DisposableEffect, DisposableEffectResult, MutableState};
use cranpose_foundation::lazy::{remember_lazy_list_state, LazyListScope, LazyListState};
use cranpose_foundation::SemanticsConfiguration;
use cranpose_ui::widgets::{LazyColumn, LazyColumnSpec};
use cranpose_ui::{
    composable, Box, BoxSpec, Brush, Button, Color, Column, ColumnSpec, CornerRadii,
    LinearArrangement, Modifier, Row, RowSpec, Size, Spacer, Text, TextStyle, VerticalAlignment,
};

#[derive(Clone, Default, PartialEq)]
struct LifecycleStats {
    total_composes: usize,
    total_effects: usize,
    total_disposes: usize,
}

const MIN_DEMO_ITEM_COUNT: usize = 10;
const DEFAULT_DEMO_ITEM_COUNT: usize = 100;
const MAX_DEMO_ITEM_COUNT: usize = 10_000;

fn clamp_demo_item_count(count: usize) -> usize {
    count.clamp(MIN_DEMO_ITEM_COUNT, MAX_DEMO_ITEM_COUNT)
}

fn update_demo_item_count(
    item_count: MutableState<usize>,
    list_state: LazyListState,
    next_count: usize,
) {
    let clamped_count = clamp_demo_item_count(next_count);
    item_count.set(clamped_count);

    let last_index = clamped_count.saturating_sub(1);
    if list_state.first_visible_item_index() > last_index {
        list_state.scroll_to_item(last_index, 0.0);
    }
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
        TextStyle::default(),
    );
}

/// Displays the visible and cached item counts from LazyListState.
/// This is in its own composable scope to isolate stats() reactivity.
/// The reactive read happens INSIDE this function, not at the call site.
#[allow(non_snake_case)]
#[composable]
fn LazyListStatsDisplay(list_state: cranpose_foundation::lazy::LazyListState) {
    // Reactive read happens here - isolated from parent scope
    let stats = list_state.stats();
    let visible = stats.items_in_use;
    let cached = stats.items_in_pool;
    println!("Cranpose stats text, visible={visible}");

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
                TextStyle::default(),
            );
            Text(
                format!("Cached: {}", cached),
                Modifier::empty()
                    .padding(8.0)
                    .background(Color(0.5, 0.4, 0.2, 0.8))
                    .rounded_corners(8.0),
                TextStyle::default(),
            );
        },
    );
}

/// Displays the first visible item index from LazyListState.
/// This is in its own composable scope to isolate first_visible_item_index() reactivity.
/// The reactive read happens INSIDE this function, not at the call site.
#[allow(non_snake_case)]
#[composable]
fn FirstVisibleIndexDisplay(list_state: cranpose_foundation::lazy::LazyListState) {
    // Reactive read happens here - isolated from parent scope
    let first_index = list_state.first_visible_item_index();
    println!("Cranpose ISOLATED FirstIndex text: {}", first_index);
    Text(
        format!("FirstIndex: {}", first_index),
        Modifier::empty()
            .padding(8.0)
            .background(Color(0.4, 0.3, 0.5, 0.8))
            .rounded_corners(8.0),
        TextStyle::default(),
    );
}

#[allow(non_snake_case)]
#[composable]
fn DemoActionButton<F>(label: &'static str, background: Color, on_click: F)
where
    F: FnMut() + 'static,
{
    Button(
        Modifier::empty()
            .rounded_corners(8.0)
            .draw_behind(move |scope| {
                scope.draw_round_rect(Brush::solid(background), CornerRadii::uniform(8.0));
            })
            .padding(8.0),
        on_click,
        move || {
            Text(label, Modifier::empty().padding(4.0), TextStyle::default());
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn LifecycleListItem(index: usize, stats: MutableState<LifecycleStats>) {
    println!("Cranpose item id={index}");

    DisposableEffect!(index, move |_key| {
        stats.update(|current| {
            current.total_composes += 1;
            current.total_effects += 1;
        });
        DisposableEffectResult::new(move || {
            println!("Dispose item id={index}");
            stats.update(|current| current.total_disposes += 1);
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
            Text(
                item_label.clone(),
                Modifier::empty().padding(4.0),
                TextStyle::default(),
            );

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
                            TextStyle::default(),
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
                TextStyle::default(),
            );
        },
    );
}

#[composable]
pub fn lazy_list_example() {
    let list_state = remember_lazy_list_state();
    let item_count = cranpose_core::useState(|| DEFAULT_DEMO_ITEM_COUNT);
    let lifecycle_stats =
        cranpose_core::remember(|| cranpose_core::mutableStateOf(LifecycleStats::default()))
            .with(|state| *state);

    Column(
        Modifier::empty()
            .padding(24.0)
            .background(Color(0.08, 0.10, 0.18, 1.0))
            .rounded_corners(24.0)
            .padding(16.0),
        ColumnSpec::default(),
        move || {
            Text(
                "Lazy List Demo",
                Modifier::empty()
                    .padding(12.0)
                    .background(Color(1.0, 1.0, 1.0, 0.08))
                    .rounded_corners(16.0),
                TextStyle::default(),
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            let count = clamp_demo_item_count(item_count.get());
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(16.0))
                    .vertical_alignment(VerticalAlignment::Top),
                move || {
                    Column(
                        Modifier::empty().weight(1.0),
                        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                        move || {
                            Text(
                                format!("Virtualized list with {} items", count),
                                Modifier::empty()
                                    .padding(8.0)
                                    .background(Color(0.2, 0.3, 0.4, 0.7))
                                    .rounded_corners(12.0),
                                TextStyle::default(),
                            );

                            LifecycleStatsDisplay(lifecycle_stats);

                            LazyListStatsDisplay(list_state);

                            FirstVisibleIndexDisplay(list_state);
                        },
                    );

                    Column(
                        Modifier::empty(),
                        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                        move || {
                            Row(
                                Modifier::empty(),
                                RowSpec::new()
                                    .horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
                                move || {
                                    DemoActionButton(
                                        "Add 10 items",
                                        Color(0.2, 0.5, 0.3, 1.0),
                                        move || {
                                            update_demo_item_count(
                                                item_count,
                                                list_state,
                                                item_count.get().saturating_add(10),
                                            );
                                        },
                                    );

                                    DemoActionButton(
                                        "Remove 10",
                                        Color(0.6, 0.2, 0.2, 1.0),
                                        move || {
                                            update_demo_item_count(
                                                item_count,
                                                list_state,
                                                item_count.get().saturating_sub(10),
                                            );
                                        },
                                    );

                                    DemoActionButton(
                                        "Set usize::MAX",
                                        Color(0.6, 0.3, 0.6, 1.0),
                                        move || {
                                            update_demo_item_count(
                                                item_count,
                                                list_state,
                                                MAX_DEMO_ITEM_COUNT,
                                            );
                                            list_state.scroll_to_item(0, 0.0);
                                        },
                                    );
                                },
                            );

                            Row(
                                Modifier::empty(),
                                RowSpec::new()
                                    .horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
                                move || {
                                    DemoActionButton(
                                        "Jump to Middle",
                                        Color(0.3, 0.4, 0.6, 1.0),
                                        move || {
                                            let middle =
                                                clamp_demo_item_count(item_count.get()) / 2;
                                            list_state.scroll_to_item(middle, 0.0);
                                        },
                                    );

                                    DemoActionButton(
                                        "⏫ Start",
                                        Color(0.2, 0.5, 0.5, 1.0),
                                        move || {
                                            list_state.scroll_to_item(0, 0.0);
                                        },
                                    );

                                    DemoActionButton(
                                        "⏬ End",
                                        Color(0.5, 0.4, 0.2, 1.0),
                                        move || {
                                            let last_index =
                                                clamp_demo_item_count(item_count.get())
                                                    .saturating_sub(1);
                                            list_state.scroll_to_item(last_index, 0.0);
                                        },
                                    );
                                },
                            );
                        },
                    );
                },
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            // The actual LazyColumn with virtualization using the DSL
            let count = clamp_demo_item_count(item_count.get());
            let list_container_modifier = Modifier::empty()
                .semantics(|config: &mut SemanticsConfiguration| {
                    config.content_description = Some("LazyListViewport".to_string());
                })
                .fill_max_width()
                .weight(1.0)
                .background(Color(0.06, 0.08, 0.14, 1.0))
                .rounded_corners(12.0);

            Box(list_container_modifier, BoxSpec::default(), move || {
                LazyColumn(
                    Modifier::empty().fill_max_size(),
                    list_state,
                    LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                    {
                        move |scope| {
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
                                        TextStyle::default(),
                                    );
                                },
                            );
                        }
                    },
                );
            });
        },
    );
}

#[cfg(test)]
mod tests {
    use super::{
        clamp_demo_item_count, DEFAULT_DEMO_ITEM_COUNT, MAX_DEMO_ITEM_COUNT, MIN_DEMO_ITEM_COUNT,
    };

    #[test]
    fn clamp_demo_item_count_limits_values() {
        assert_eq!(clamp_demo_item_count(0), MIN_DEMO_ITEM_COUNT);
        assert_eq!(
            clamp_demo_item_count(DEFAULT_DEMO_ITEM_COUNT),
            DEFAULT_DEMO_ITEM_COUNT
        );
        assert_eq!(
            clamp_demo_item_count(MAX_DEMO_ITEM_COUNT + 1),
            MAX_DEMO_ITEM_COUNT
        );
    }
}
