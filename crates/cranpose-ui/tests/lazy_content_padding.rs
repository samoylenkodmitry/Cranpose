use cranpose_foundation::lazy::{LazyItems, LazyListScope, rememberLazyListState};
use cranpose_ui::{
    Box, BoxSpec, LayoutDirection, LayoutTree, LazyColumn, LazyColumnSpec, LazyRow, LazyRowSpec,
    Modifier, PaddingValues, ProvideLayoutDirection, Size,
};

use crate::{directional_layout::tagged, text_measured_layout_integration::layout_composition};

/// Start 12, top 8, end 4, bottom 6: every edge differs, so a test sees
/// which edge each one lands on.
const PADDING: PaddingValues = PaddingValues::new(12.0, 8.0, 4.0, 6.0);
const VIEWPORT: Size = Size {
    width: 200.0,
    height: 100.0,
};

fn lay_out(direction: LayoutDirection, list: impl Fn()) -> LayoutTree {
    let mut composition = cranpose_ui::run_test_composition(|| {
        ProvideLayoutDirection(direction, &list);
    });
    layout_composition(&mut composition, VIEWPORT)
}

fn tagged_box(tag: &'static str, modifier: Modifier) {
    Box(modifier.content_description(tag), BoxSpec::default(), || {});
}

/// A column of a full-width row and a 20 × 20 one.
fn column(direction: LayoutDirection, spec: LazyColumnSpec) -> LayoutTree {
    lay_out(direction, || {
        LazyColumn(
            Modifier::empty()
                .size_points(VIEWPORT.width, VIEWPORT.height)
                .content_description("list"),
            rememberLazyListState(),
            spec.clone(),
            |scope| {
                scope.items(LazyItems::new(2), |index| {
                    if index == 0 {
                        tagged_box("wide", Modifier::empty().fill_max_width().height(20.0));
                    } else {
                        tagged_box("small", Modifier::empty().size_points(20.0, 20.0));
                    }
                });
            },
        );
    })
}

/// A row of a full-height item and a 30 × 20 one.
fn row(direction: LayoutDirection, spec: LazyRowSpec) -> LayoutTree {
    lay_out(direction, || {
        LazyRow(
            Modifier::empty()
                .size_points(VIEWPORT.width, VIEWPORT.height)
                .content_description("list"),
            rememberLazyListState(),
            spec.clone(),
            |scope| {
                scope.items(LazyItems::new(2), |index| {
                    if index == 0 {
                        tagged_box("tall", Modifier::empty().fill_max_height().width(30.0));
                    } else {
                        tagged_box("small", Modifier::empty().size_points(30.0, 20.0));
                    }
                });
            },
        );
    })
}

/// The tagged box's x, y, width and height.
fn rect(layout: &LayoutTree, tag: &str) -> (f32, f32, f32, f32) {
    let rect = tagged(layout.root(), tag).rect;
    (rect.x, rect.y, rect.width, rect.height)
}

#[test]
fn a_column_insets_its_items_by_the_start_and_end_padding() {
    let layout = column(
        LayoutDirection::Ltr,
        LazyColumnSpec::new().content_padding(PADDING),
    );
    assert_eq!(rect(&layout, "wide"), (12.0, 8.0, 184.0, 20.0));
    assert_eq!(rect(&layout, "small"), (12.0, 28.0, 20.0, 20.0));
}

#[test]
fn a_right_to_left_column_puts_the_start_padding_on_the_right() {
    let layout = column(
        LayoutDirection::Rtl,
        LazyColumnSpec::new().content_padding(PADDING),
    );
    assert_eq!(rect(&layout, "wide"), (4.0, 8.0, 184.0, 20.0));
    assert_eq!(rect(&layout, "small"), (168.0, 28.0, 20.0, 20.0));
}

#[test]
fn a_reversed_column_starts_above_the_bottom_padding() {
    let layout = column(
        LayoutDirection::Ltr,
        LazyColumnSpec::new()
            .content_padding(PADDING)
            .reverse_layout(true),
    );
    assert_eq!(rect(&layout, "wide"), (12.0, 74.0, 184.0, 20.0));
    assert_eq!(rect(&layout, "small"), (12.0, 54.0, 20.0, 20.0));
}

#[test]
fn content_padding_all_pads_all_four_edges() {
    let layout = column(
        LayoutDirection::Ltr,
        LazyColumnSpec::new().content_padding_all(8.0),
    );
    assert_eq!(rect(&layout, "wide"), (8.0, 8.0, 184.0, 20.0));
}

#[test]
fn a_row_insets_its_items_by_the_top_and_bottom_padding() {
    let layout = row(
        LayoutDirection::Ltr,
        LazyRowSpec::new().content_padding(PADDING),
    );
    assert_eq!(rect(&layout, "tall"), (12.0, 8.0, 30.0, 86.0));
    assert_eq!(rect(&layout, "small"), (42.0, 8.0, 30.0, 20.0));
}

#[test]
fn a_right_to_left_row_starts_inside_the_start_padding_on_the_right() {
    let layout = row(
        LayoutDirection::Rtl,
        LazyRowSpec::new().content_padding(PADDING),
    );
    assert_eq!(rect(&layout, "tall"), (158.0, 8.0, 30.0, 86.0));
    assert_eq!(rect(&layout, "small"), (128.0, 8.0, 30.0, 20.0));
}

#[test]
fn a_reversed_row_starts_inside_the_end_padding() {
    let layout = row(
        LayoutDirection::Ltr,
        LazyRowSpec::new()
            .content_padding(PADDING)
            .reverse_layout(true),
    );
    assert_eq!(rect(&layout, "tall"), (166.0, 8.0, 30.0, 86.0));
    assert_eq!(rect(&layout, "small"), (136.0, 8.0, 30.0, 20.0));
}

#[test]
fn a_screen_reader_learns_the_padding_on_the_physical_edges() {
    let ranges = |layout: &LayoutTree| {
        let semantics = tagged(layout.root(), "list")
            .node_data
            .semantics()
            .expect("list semantics");
        (semantics.vertical_scroll, semantics.horizontal_scroll)
    };
    let reversed = column(
        LayoutDirection::Ltr,
        LazyColumnSpec::new()
            .content_padding(PADDING)
            .reverse_layout(true),
    );
    let range = ranges(&reversed).0.expect("vertical scroll range");
    assert_eq!(
        (range.content_padding_start, range.content_padding_end),
        (8.0, 6.0),
        "top, then bottom"
    );
    let rtl = row(
        LayoutDirection::Rtl,
        LazyRowSpec::new().content_padding(PADDING),
    );
    let range = ranges(&rtl).1.expect("horizontal scroll range");
    assert_eq!(
        (range.content_padding_start, range.content_padding_end),
        (4.0, 12.0),
        "left, then right"
    );
}
