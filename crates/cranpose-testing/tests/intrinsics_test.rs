//! `Modifier::width_intrinsic` and `height_intrinsic` size a layout to its
//! content's intrinsic size and measure the content at that size, as
//! Compose's `Modifier.width(IntrinsicSize)` and `height(IntrinsicSize)` do.

use cranpose_testing::{ComposeTestRule, PlacedSemanticsNode};
use cranpose_ui::{Box, BoxSpec, Column, ColumnSpec, IntrinsicSize, Modifier, Row, RowSpec, Size};

fn tagged(modifier: Modifier, label: &'static str) -> Modifier {
    modifier.semantics(move |config| {
        config.content_description = Some(label.to_string());
    })
}

fn placed(content: impl FnMut() + 'static) -> PlacedSemanticsNode {
    let mut rule = ComposeTestRule::new();
    rule.set_content(content).expect("the content composes");
    rule.placed_semantics(Size::new(400.0, 400.0))
        .expect("the content lays out")
        .expect("the content places something")
}

fn bounds(root: &PlacedSemanticsNode, label: &str) -> (f32, f32) {
    let node = root
        .flatten()
        .into_iter()
        .find(|node| node.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("{label} is placed"));
    (node.layout_bounds.width, node.layout_bounds.height)
}

fn sized(width: f32, height: f32) -> Modifier {
    Modifier::empty().size(Size { width, height })
}

/// Three boxes 30, then a divider, then 50 tall, in a row of `size` height.
fn divided_row(size: IntrinsicSize) -> PlacedSemanticsNode {
    placed(move || {
        Row(
            tagged(Modifier::empty().height_intrinsic(size), "row"),
            RowSpec::default(),
            || {
                Box(sized(40.0, 30.0), BoxSpec::default(), || {});
                Box(
                    tagged(Modifier::empty().width(1.0).fill_max_height(), "divider"),
                    BoxSpec::default(),
                    || {},
                );
                Box(sized(40.0, 50.0), BoxSpec::default(), || {});
            },
        );
    })
}

#[test]
fn a_min_intrinsic_row_gives_a_fill_height_divider_its_tallest_sibling() {
    let root = divided_row(IntrinsicSize::Min);
    assert_eq!(
        bounds(&root, "row").1,
        50.0,
        "the row is as tall as its tallest child"
    );
    assert_eq!(
        bounds(&root, "divider").1,
        50.0,
        "the divider fills the row's intrinsic height"
    );
}

#[test]
fn a_max_intrinsic_row_gives_a_fill_height_divider_its_tallest_sibling() {
    let root = divided_row(IntrinsicSize::Max);
    assert_eq!(bounds(&root, "row").1, 50.0);
    assert_eq!(bounds(&root, "divider").1, 50.0);
}

#[test]
fn a_max_intrinsic_column_gives_fill_width_children_its_widest_child() {
    let root = placed(|| {
        Column(
            tagged(
                Modifier::empty().width_intrinsic(IntrinsicSize::Max),
                "column",
            ),
            ColumnSpec::default(),
            || {
                Box(
                    tagged(Modifier::empty().fill_max_width().height(10.0), "bar"),
                    BoxSpec::default(),
                    || {},
                );
                Box(sized(80.0, 10.0), BoxSpec::default(), || {});
                Box(sized(60.0, 10.0), BoxSpec::default(), || {});
            },
        );
    });
    assert_eq!(
        bounds(&root, "column").0,
        80.0,
        "the column is as wide as its widest child"
    );
    assert_eq!(
        bounds(&root, "bar").0,
        80.0,
        "the bar fills the column's intrinsic width"
    );
}
