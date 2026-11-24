use std::{cell::Cell, rc::Rc};

use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState, rememberLazyListState};
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, LayoutDirection, LayoutTree, LazyColumn, LazyColumnSpec,
    LazyRow, LazyRowSpec, LinearArrangement, Modifier, PaddingValues, ProvideLayoutDirection, Row,
    RowSpec, ScrollState, Size,
    widgets::{
        GridCells, LazyVerticalGrid, LazyVerticalGridSpec, LazyVerticalStaggeredGrid,
        LazyVerticalStaggeredGridSpec, rememberLazyGridState, rememberLazyStaggeredGridState,
    },
};

use crate::{directional_layout::tagged, text_measured_layout_integration::layout_composition};

const VIEWPORT: Size = Size {
    width: 300.0,
    height: 200.0,
};

fn lay_out(direction: LayoutDirection, content: impl Fn() + 'static) -> LayoutTree {
    let mut composition = cranpose_ui::run_test_composition(move || {
        ProvideLayoutDirection(direction, &content);
    });
    layout_composition(&mut composition, VIEWPORT)
}

fn lay_out_lazy_column(
    spec: LazyColumnSpec,
    build: impl Fn(&mut cranpose_foundation::lazy::LazyListIntervalContent) + 'static,
) -> LayoutTree {
    lay_out(LayoutDirection::Ltr, move || {
        LazyColumn(
            Modifier::empty().fill_max_size(),
            rememberLazyListState(),
            spec.clone(),
            |scope| build(scope),
        );
    })
}

fn rect(layout: &LayoutTree, tag: &str) -> (f32, f32, f32, f32) {
    let rect = tagged(layout.root(), tag).rect;
    (rect.x, rect.y, rect.width, rect.height)
}

fn tagged_box(tag: impl Into<String>, width: f32, height: f32) {
    Box(
        Modifier::empty()
            .size_points(width, height)
            .content_description(tag),
        BoxSpec::default(),
        || {},
    );
}

#[test]
fn lazy_row_without_height_inside_lazy_column_wraps_visible_items() {
    let layout = lay_out_lazy_column(
        LazyColumnSpec::new()
            .content_padding_all(4.0)
            .vertical_arrangement(LinearArrangement::spaced_by(4.0)),
        |scope| {
            scope.item_keyed(Some(1), None, || {
                let row = rememberLazyListState();
                let heights: Rc<[f32]> = Rc::from([16.0, 28.0, 20.0, 24.0]);
                LazyRow(
                    Modifier::empty()
                        .fill_max_width()
                        .content_description("wrapped_row"),
                    row,
                    LazyRowSpec::new()
                        .content_padding(PaddingValues::symmetric(5.0, 3.0))
                        .horizontal_arrangement(LinearArrangement::spaced_by(4.0)),
                    move |items| {
                        let heights = Rc::clone(&heights);
                        items.items(LazyItems::new(heights.len()), move |i| {
                            tagged_box(format!("w{i}"), 42.0, heights[i]);
                        });
                    },
                );
            });
            scope.item_keyed(Some(2), None, || {
                tagged_box("after_row", 80.0, 18.0);
            });
        },
    );

    assert_eq!(rect(&layout, "wrapped_row"), (4.0, 4.0, 292.0, 34.0));
    assert_eq!(rect(&layout, "after_row"), (4.0, 42.0, 80.0, 18.0));
}

#[test]
fn lazy_row_without_height_inside_vertical_scroll_wraps_visible_items() {
    let layout = lay_out(LayoutDirection::Ltr, || {
        let scroll = cranpose_core::remember(|| ScrollState::new(0.0)).with(Clone::clone);
        Column(
            Modifier::empty()
                .fill_max_size()
                .vertical_scroll(scroll, false)
                .padding(3.0),
            ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(4.0)),
            || {
                let row = rememberLazyListState();
                let heights: Rc<[f32]> = Rc::from([16.0, 24.0, 18.0, 22.0]);
                LazyRow(
                    Modifier::empty()
                        .fill_max_width()
                        .content_description("row_in_vscroll"),
                    row,
                    LazyRowSpec::new()
                        .content_padding_all(3.0)
                        .horizontal_arrangement(LinearArrangement::spaced_by(4.0)),
                    move |items| {
                        let heights = Rc::clone(&heights);
                        items.items(LazyItems::new(heights.len()), move |i| {
                            tagged_box(format!("vs{i}"), 44.0, heights[i]);
                        });
                    },
                );
                tagged_box("tail", 54.0, 16.0);
            },
        );
    });

    assert_eq!(rect(&layout, "row_in_vscroll"), (3.0, 3.0, 294.0, 30.0));
    assert_eq!(rect(&layout, "tail"), (3.0, 37.0, 54.0, 16.0));
}

#[test]
fn lazy_column_with_width_in_inside_lazy_row_wraps_content_width() {
    let layout = lay_out(LayoutDirection::Ltr, || {
        let outer = rememberLazyListState();
        LazyRow(
            Modifier::empty().fill_max_width().height(90.0),
            outer,
            LazyRowSpec::new()
                .content_padding_all(4.0)
                .horizontal_arrangement(LinearArrangement::spaced_by(6.0)),
            |scope| {
                scope.item_keyed(Some(1), None, || {
                    let col = rememberLazyListState();
                    let widths: Rc<[f32]> = Rc::from([52.0, 86.0, 68.0, 60.0]);
                    LazyColumn(
                        Modifier::empty()
                            .width_in(56.0, 104.0)
                            .fill_max_height()
                            .content_description("wrapped_col"),
                        col,
                        LazyColumnSpec::new()
                            .content_padding_all(3.0)
                            .vertical_arrangement(LinearArrangement::spaced_by(3.0)),
                        move |items| {
                            let widths = Rc::clone(&widths);
                            items.items(LazyItems::new(widths.len()), move |i| {
                                tagged_box(format!("c{i}"), widths[i], 18.0);
                            });
                        },
                    );
                });
                scope.item_keyed(Some(2), None, || {
                    tagged_box("next_sibling", 40.0, 20.0);
                });
            },
        );
    });

    assert_eq!(rect(&layout, "wrapped_col"), (4.0, 4.0, 92.0, 82.0));
    assert_eq!(rect(&layout, "next_sibling"), (102.0, 4.0, 40.0, 20.0));
}

#[test]
fn lazy_list_cross_axis_clamps_to_min_and_max_bounds() {
    let layout = lay_out(LayoutDirection::Ltr, || {
        Column(
            Modifier::empty().fill_max_size(),
            ColumnSpec::default(),
            || {
                let narrow = rememberLazyListState();
                LazyColumn(
                    Modifier::empty()
                        .width_in(56.0, 104.0)
                        .height(40.0)
                        .content_description("clamped_min"),
                    narrow,
                    LazyColumnSpec::new().content_padding_all(3.0),
                    |items| {
                        items.items(LazyItems::new(2), |i| {
                            tagged_box(format!("min_{i}"), 20.0, 16.0);
                        });
                    },
                );

                let tall = rememberLazyListState();
                LazyRow(
                    Modifier::empty()
                        .fill_max_width()
                        .size_in(Size::new(0.0, 24.0), Size::new(f32::INFINITY, 32.0))
                        .content_description("clamped_max"),
                    tall,
                    LazyRowSpec::new(),
                    |items| {
                        items.items(LazyItems::new(2), |i| {
                            let h = if i == 0 { 16.0 } else { 40.0 };
                            tagged_box(format!("max_{i}"), 40.0, h);
                        });
                    },
                );
            },
        );
    });

    assert_eq!(rect(&layout, "clamped_min"), (0.0, 0.0, 56.0, 40.0));
    assert_eq!(rect(&layout, "clamped_max"), (0.0, 40.0, 300.0, 32.0));
}

#[test]
fn empty_lazy_lists_resolve_finite_cross_axis_with_padding_and_min_bounds() {
    let layout = lay_out_lazy_column(LazyColumnSpec::default(), |scope| {
        scope.item_keyed(Some(1), None, || {
            LazyRow(
                Modifier::empty()
                    .fill_max_width()
                    .content_description("empty_padded_row"),
                rememberLazyListState(),
                LazyRowSpec::new().content_padding(PaddingValues::symmetric(5.0, 3.0)),
                |_| {},
            );
        });
        scope.item_keyed(Some(2), None, || {
            LazyRow(
                Modifier::empty()
                    .fill_max_width()
                    .content_description("empty_plain_row"),
                rememberLazyListState(),
                LazyRowSpec::new(),
                |_| {},
            );
        });
        scope.item_keyed(Some(3), None, || {
            LazyColumn(
                Modifier::empty()
                    .width_in(56.0, 104.0)
                    .height(20.0)
                    .content_description("empty_min_col"),
                rememberLazyListState(),
                LazyColumnSpec::new().content_padding_all(3.0),
                |_| {},
            );
        });
        scope.item_keyed(Some(4), None, || {
            tagged_box("sentinel", 30.0, 12.0);
        });
    });

    assert_eq!(rect(&layout, "empty_padded_row"), (0.0, 0.0, 300.0, 6.0));
    assert_eq!(rect(&layout, "empty_plain_row"), (0.0, 6.0, 300.0, 0.0));
    assert_eq!(rect(&layout, "empty_min_col"), (0.0, 7.0, 56.0, 20.0));
    assert_eq!(rect(&layout, "sentinel"), (0.0, 27.0, 30.0, 12.0));
}

#[test]
fn rtl_lazy_column_aligns_items_against_wrapped_cross_axis_width() {
    let bounded = lay_out(LayoutDirection::Rtl, || {
        LazyColumn(
            Modifier::empty()
                .width_in(56.0, 104.0)
                .height(80.0)
                .content_description("rtl_bounded"),
            rememberLazyListState(),
            LazyColumnSpec::new().content_padding(PaddingValues::new(4.0, 0.0, 2.0, 0.0)),
            |items| {
                items.items(LazyItems::new(2), |i| {
                    let w = if i == 0 { 80.0 } else { 50.0 };
                    tagged_box(format!("rtl_b{i}"), w, 18.0);
                });
            },
        );
    });

    assert_eq!(rect(&bounded, "rtl_bounded"), (0.0, 0.0, 86.0, 80.0));
    assert_eq!(rect(&bounded, "rtl_b0"), (2.0, 0.0, 80.0, 18.0));
    assert_eq!(rect(&bounded, "rtl_b1"), (32.0, 18.0, 50.0, 18.0));

    let unbounded = lay_out(LayoutDirection::Rtl, || {
        let scroll = cranpose_core::remember(|| ScrollState::new(0.0)).with(Clone::clone);
        Row(
            Modifier::empty()
                .fill_max_size()
                .horizontal_scroll(scroll, false),
            RowSpec::default(),
            || {
                LazyColumn(
                    Modifier::empty()
                        .height(80.0)
                        .content_description("rtl_unbounded"),
                    rememberLazyListState(),
                    LazyColumnSpec::new().content_padding(PaddingValues::new(4.0, 0.0, 2.0, 0.0)),
                    |items| {
                        items.items(LazyItems::new(2), |i| {
                            let w = if i == 0 { 80.0 } else { 50.0 };
                            tagged_box(format!("rtl_u{i}"), w, 18.0);
                        });
                    },
                );
            },
        );
    });

    assert_eq!(rect(&unbounded, "rtl_unbounded"), (214.0, 0.0, 86.0, 80.0));
    assert_eq!(rect(&unbounded, "rtl_u0"), (216.0, 0.0, 80.0, 18.0));
    assert_eq!(rect(&unbounded, "rtl_u1"), (246.0, 18.0, 50.0, 18.0));
}

#[test]
fn retained_scroll_pass_preserves_and_updates_wrapped_cross_axis_height() {
    let captured_state: Rc<Cell<Option<LazyListState>>> = Rc::new(Cell::new(None));
    let capture = Rc::clone(&captured_state);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let scroll = cranpose_core::remember(|| ScrollState::new(0.0)).with(Clone::clone);
        let row_state = rememberLazyListState();
        capture.set(Some(row_state));
        Column(
            Modifier::empty()
                .fill_max_size()
                .vertical_scroll(scroll, false),
            ColumnSpec::default(),
            move || {
                LazyRow(
                    Modifier::empty()
                        .fill_max_width()
                        .content_description("scroll_row"),
                    row_state,
                    LazyRowSpec::new().content_padding(PaddingValues::symmetric(0.0, 3.0)),
                    |items| {
                        items.items(LazyItems::new(12), |i| {
                            let h = if i < 8 { 20.0 } else { 44.0 };
                            tagged_box(format!("item_{i}"), 60.0, h);
                        });
                    },
                );
                tagged_box("below", 40.0, 10.0);
            },
        );
    });

    let initial = layout_composition(&mut composition, VIEWPORT);
    assert_eq!(rect(&initial, "scroll_row"), (0.0, 0.0, 300.0, 26.0));
    assert_eq!(rect(&initial, "below"), (0.0, 26.0, 40.0, 10.0));

    let state = captured_state.get().expect("captured LazyListState");
    state.scroll_to_item(0, 12.0);
    let nudged = layout_composition(&mut composition, VIEWPORT);
    assert_eq!(rect(&nudged, "scroll_row"), (0.0, 0.0, 300.0, 26.0));
    assert_eq!(rect(&nudged, "below"), (0.0, 26.0, 40.0, 10.0));

    let retained_before = cranpose_ui::retained_measure_count();
    state.scroll_to_item(0, 18.0);
    let retained_pass = layout_composition(&mut composition, VIEWPORT);
    assert!(
        cranpose_ui::retained_measure_count() > retained_before,
        "small scroll inside already-measured items uses the retained measure path"
    );
    assert_eq!(rect(&retained_pass, "scroll_row"), (0.0, 0.0, 300.0, 26.0));
    assert_eq!(rect(&retained_pass, "below"), (0.0, 26.0, 40.0, 10.0));

    state.scroll_to_item(8, 0.0);
    let scrolled_to_tall = layout_composition(&mut composition, VIEWPORT);
    assert_eq!(
        rect(&scrolled_to_tall, "scroll_row"),
        (0.0, 0.0, 300.0, 50.0)
    );
    assert_eq!(rect(&scrolled_to_tall, "below"), (0.0, 50.0, 40.0, 10.0));
}

#[test]
fn lazy_grids_keep_bounded_cross_axis_extent_under_loose_constraints() {
    let layout = lay_out(LayoutDirection::Ltr, || {
        Box(
            Modifier::empty().size_points(200.0, 120.0),
            BoxSpec::default(),
            || {
                LazyVerticalGrid(
                    Modifier::empty()
                        .height(50.0)
                        .content_description("loose_grid"),
                    rememberLazyGridState(),
                    LazyVerticalGridSpec::new(GridCells::Fixed(2)),
                    |items| {
                        items.items(LazyItems::new(1), |_| {
                            tagged_box("grid_cell", 40.0, 20.0);
                        });
                    },
                );
            },
        );
    });

    assert_eq!(rect(&layout, "loose_grid"), (0.0, 0.0, 200.0, 50.0));

    let staggered = lay_out(LayoutDirection::Ltr, || {
        Box(
            Modifier::empty().size_points(200.0, 120.0),
            BoxSpec::default(),
            || {
                LazyVerticalStaggeredGrid(
                    Modifier::empty()
                        .height(50.0)
                        .content_description("loose_staggered"),
                    rememberLazyStaggeredGridState(),
                    LazyVerticalStaggeredGridSpec::new(GridCells::Fixed(2)),
                    |items| {
                        items.items(LazyItems::new(2), |i| {
                            tagged_box(format!("stag_{i}"), 40.0, 20.0);
                        });
                    },
                );
            },
        );
    });

    assert_eq!(rect(&staggered, "loose_staggered"), (0.0, 0.0, 200.0, 50.0));
}

#[test]
fn lazy_row_modifier_order_padding_before_size_in_constrains_items_and_adds_outer_padding() {
    let layout = lay_out_lazy_column(LazyColumnSpec::default(), |scope| {
        scope.item_keyed(Some(1), None, || {
            LazyRow(
                Modifier::empty()
                    .fill_max_width()
                    .padding(3.0)
                    .size_in(Size::new(0.0, 24.0), Size::new(f32::INFINITY, 32.0))
                    .content_description("padded_size_in"),
                rememberLazyListState(),
                LazyRowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(4.0)),
                |items| {
                    items.items(LazyItems::new(2), |i| {
                        let h = if i == 0 { 16.0 } else { 40.0 };
                        tagged_box(format!("pb{i}"), 40.0, h);
                    });
                },
            );
        });
        scope.item_keyed(Some(2), None, || {
            tagged_box("after_perm", 50.0, 12.0);
        });
    });

    assert_eq!(rect(&layout, "padded_size_in"), (0.0, 0.0, 300.0, 38.0));
    assert_eq!(rect(&layout, "pb0"), (3.0, 3.0, 40.0, 16.0));
    assert_eq!(rect(&layout, "pb1"), (47.0, 3.0, 40.0, 32.0));
    assert_eq!(rect(&layout, "after_perm"), (0.0, 38.0, 50.0, 12.0));
}

#[test]
fn reversed_lazy_row_with_spare_space_honors_spaced_by_and_end_arrangements() {
    let layout = lay_out(LayoutDirection::Ltr, || {
        Column(
            Modifier::empty().fill_max_size(),
            ColumnSpec::default(),
            || {
                LazyRow(
                    Modifier::empty()
                        .fill_max_width()
                        .content_description("rev_spaced"),
                    rememberLazyListState(),
                    LazyRowSpec::new()
                        .content_padding(PaddingValues::new(10.0, 2.0, 4.0, 2.0))
                        .reverse_layout(true)
                        .horizontal_arrangement(LinearArrangement::spaced_by(4.0)),
                    |items| {
                        items.items(LazyItems::new(3), |i| {
                            let w = match i {
                                0 => 20.0,
                                1 => 30.0,
                                _ => 50.0,
                            };
                            tagged_box(format!("rs{i}"), w, 16.0);
                        });
                    },
                );
                LazyRow(
                    Modifier::empty()
                        .fill_max_width()
                        .content_description("rev_end"),
                    rememberLazyListState(),
                    LazyRowSpec::new()
                        .content_padding(PaddingValues::new(10.0, 2.0, 4.0, 2.0))
                        .reverse_layout(true)
                        .horizontal_arrangement(LinearArrangement::End),
                    |items| {
                        items.items(LazyItems::new(2), |i| {
                            let w = if i == 0 { 20.0 } else { 50.0 };
                            tagged_box(format!("re{i}"), w, 16.0);
                        });
                    },
                );
                LazyRow(
                    Modifier::empty().content_description("rev_loose"),
                    rememberLazyListState(),
                    LazyRowSpec::new().reverse_layout(true),
                    |items| {
                        items.items(LazyItems::new(2), |i| {
                            tagged_box(format!("rl{i}"), 40.0, 16.0);
                        });
                    },
                );
            },
        );
    });

    // Reverse row has start=10 (after), end=4 (before); SpacedBy packs visual [2, 1, 0] from left=10.
    assert_eq!(rect(&layout, "rev_spaced"), (0.0, 0.0, 300.0, 20.0));
    assert_eq!(rect(&layout, "rs2"), (10.0, 2.0, 50.0, 16.0));
    assert_eq!(rect(&layout, "rs1"), (64.0, 2.0, 30.0, 16.0));
    assert_eq!(rect(&layout, "rs0"), (98.0, 2.0, 20.0, 16.0));

    // End packs visual [1, 0] against right=300-4=296.
    assert_eq!(rect(&layout, "rev_end"), (0.0, 20.0, 300.0, 20.0));
    assert_eq!(rect(&layout, "re0"), (276.0, 22.0, 20.0, 16.0));
    assert_eq!(rect(&layout, "re1"), (226.0, 22.0, 50.0, 16.0));

    // Loose reverse row wraps width to 80 and places items inside [0, 80].
    assert_eq!(rect(&layout, "rev_loose"), (0.0, 40.0, 80.0, 16.0));
    assert_eq!(rect(&layout, "rl0"), (40.0, 40.0, 40.0, 16.0));
    assert_eq!(rect(&layout, "rl1"), (0.0, 40.0, 40.0, 16.0));
}
