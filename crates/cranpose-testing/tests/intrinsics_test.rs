//! `Modifier::width_intrinsic` and `height_intrinsic` size a layout to its
//! content's intrinsic size and measure the content at that size, as
//! Compose's `Modifier.width(IntrinsicSize)` and `height(IntrinsicSize)` do.

use std::{cell::Cell, rc::Rc};

use cranpose_core::MutableState;
use cranpose_testing::{ComposeTestRule, PlacedSemanticsNode};
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, Constraints, IntrinsicSize, Layout, MeasureResult, Modifier,
    Row, RowSpec, Size,
};
use cranpose_ui_layout::{Measurable, MeasurePolicy, MeasureScope, VerticalAlignment};

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

/// A layout half as tall as it is wide, which counts its measures.
#[derive(Clone)]
struct HalfAsTall {
    measures: Rc<Cell<u32>>,
}

impl PartialEq for HalfAsTall {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.measures, &other.measures)
    }
}

type Measurables = [Box<dyn Measurable>];

impl MeasurePolicy for HalfAsTall {
    fn measure(
        &self,
        _: &dyn MeasureScope,
        _: &Measurables,
        constraints: Constraints,
    ) -> MeasureResult {
        self.measures.set(self.measures.get() + 1);
        let width = if constraints.max_width.is_finite() {
            constraints.max_width
        } else {
            constraints.min_width
        };
        let (width, height) = constraints.constrain(width, width / 2.0);
        MeasureResult::new(Size::new(width, height), Vec::new())
    }

    fn min_intrinsic_width(&self, _: &Measurables, height: f32) -> f32 {
        height * 2.0
    }

    fn max_intrinsic_width(&self, _: &Measurables, height: f32) -> f32 {
        height * 2.0
    }

    fn min_intrinsic_height(&self, _: &Measurables, width: f32) -> f32 {
        width / 2.0
    }

    fn max_intrinsic_height(&self, _: &Measurables, width: f32) -> f32 {
        width / 2.0
    }
}

fn divider() {
    Box(
        tagged(Modifier::empty().width(1.0).fill_max_height(), "divider"),
        BoxSpec::default(),
        || {},
    );
}

#[test]
fn a_child_gives_its_intrinsic_height_without_a_measure() {
    let measures = Rc::new(Cell::new(0));
    let policy = HalfAsTall {
        measures: Rc::clone(&measures),
    };
    let root = placed(move || {
        let policy = policy.clone();
        Row(
            tagged(
                Modifier::empty().height_intrinsic(IntrinsicSize::Min),
                "row",
            ),
            RowSpec::default(),
            move || {
                Layout(Modifier::empty().width(80.0), policy.clone(), || {});
                divider();
            },
        );
    });
    assert_eq!(bounds(&root, "row").1, 40.0);
    assert_eq!(bounds(&root, "divider").1, 40.0);
    assert_eq!(
        measures.get(),
        1,
        "the row reads the child's intrinsic height from its policy and measures it once"
    );
}

/// The height of a divider beside a 30 tall box and a 60 tall box that
/// `modifier` wraps, in a row of min intrinsic height.
fn divider_beside(modifier: fn() -> Modifier) -> f32 {
    let root = placed(move || {
        Row(
            Modifier::empty().height_intrinsic(IntrinsicSize::Min),
            RowSpec::default(),
            move || {
                Box(sized(40.0, 30.0), BoxSpec::default(), || {});
                Box(
                    modifier().size(Size::new(40.0, 60.0)),
                    BoxSpec::default(),
                    || {},
                );
                divider();
            },
        );
    });
    bounds(&root, "divider").1
}

#[test]
fn modifiers_that_keep_the_content_size_give_its_intrinsic_size() {
    assert_eq!(
        divider_beside(|| Modifier::empty().offset(5.0, 5.0)),
        60.0,
        "an offset"
    );
    assert_eq!(
        divider_beside(|| Modifier::empty().wrap_content_height(VerticalAlignment::Top, false)),
        60.0,
        "a wrapped height"
    );
    assert_eq!(
        divider_beside(|| Modifier::empty().report_size(Rc::default())),
        60.0,
        "a size report"
    );
    assert_eq!(
        divider_beside(|| Modifier::empty().report_window_rect(Rc::new(Cell::new(
            cranpose_ui_graphics::Rect::from_size(Size::default())
        )))),
        60.0,
        "a window rect report"
    );
}

/// A 200 wide row of min intrinsic height around `content`, which lays out
/// half as tall layouts with the policy it gets.
fn intrinsic_row(content: fn(HalfAsTall)) -> PlacedSemanticsNode {
    let policy = HalfAsTall {
        measures: Rc::default(),
    };
    placed(move || {
        let policy = policy.clone();
        Row(
            tagged(
                Modifier::empty()
                    .width(200.0)
                    .height_intrinsic(IntrinsicSize::Min),
                "row",
            ),
            RowSpec::default(),
            move || content(policy.clone()),
        );
    })
}

#[test]
fn a_fractional_fill_gives_its_content_the_filled_width_for_intrinsics() {
    let root = intrinsic_row(|policy| {
        Box(
            Modifier::empty().fill_max_width_fraction(0.5),
            BoxSpec::default(),
            move || {
                Layout(Modifier::empty(), policy.clone(), || {});
            },
        );
        divider();
    });
    assert_eq!(
        bounds(&root, "row").1,
        50.0,
        "the content is half as tall as the 100 wide half of the row"
    );
    assert_eq!(bounds(&root, "divider").1, 50.0);
}

#[test]
fn a_weighted_child_gives_its_intrinsic_height_in_its_share_of_the_row() {
    let root = intrinsic_row(|policy| {
        Layout(Modifier::empty().weight(1.0), policy, || {});
        Box(sized(100.0, 10.0), BoxSpec::default(), || {});
    });
    assert_eq!(
        bounds(&root, "row").1,
        50.0,
        "the weighted child is half as tall as the 100 the fixed child leaves it"
    );
}

#[test]
fn a_weighted_row_is_wide_enough_for_each_weighted_child() {
    let root = placed(|| {
        Row(
            tagged(Modifier::empty().width_intrinsic(IntrinsicSize::Max), "row"),
            RowSpec::default(),
            || {
                Box(Modifier::empty().weight(1.0), BoxSpec::default(), || {
                    Box(sized(30.0, 10.0), BoxSpec::default(), || {});
                });
                Box(
                    tagged(Modifier::empty().weight(1.0), "wide"),
                    BoxSpec::default(),
                    || {
                        Box(sized(50.0, 10.0), BoxSpec::default(), || {});
                    },
                );
            },
        );
    });
    assert_eq!(
        bounds(&root, "row").0,
        100.0,
        "equal weights need twice the widest weighted child"
    );
    assert_eq!(bounds(&root, "wide").0, 50.0);
}

/// A layout as wide as it is asked to be, whose min intrinsic width is 10
/// and max intrinsic width is 100.
#[derive(Clone, PartialEq)]
struct Stretchy;

impl MeasurePolicy for Stretchy {
    fn measure(
        &self,
        _: &dyn MeasureScope,
        _: &Measurables,
        constraints: Constraints,
    ) -> MeasureResult {
        let width = if constraints.max_width.is_finite() {
            constraints.max_width
        } else {
            constraints.min_width
        };
        let (width, height) = constraints.constrain(width, 10.0);
        MeasureResult::new(Size::new(width, height), Vec::new())
    }

    fn min_intrinsic_width(&self, _: &Measurables, _: f32) -> f32 {
        10.0
    }

    fn max_intrinsic_width(&self, _: &Measurables, _: f32) -> f32 {
        100.0
    }

    fn min_intrinsic_height(&self, _: &Measurables, _: f32) -> f32 {
        10.0
    }

    fn max_intrinsic_height(&self, _: &Measurables, _: f32) -> f32 {
        10.0
    }
}

/// A rule that composes a row of `Stretchy` whose intrinsic modifier is what
/// `modifier` makes of the state it returns.
fn stretchy_row<T: Clone + PartialEq + 'static>(
    initial: T,
    modifier: fn(T) -> Modifier,
) -> (ComposeTestRule, MutableState<T>) {
    let mut rule = ComposeTestRule::new();
    let state = MutableState::with_runtime(initial, rule.runtime_handle());
    rule.set_content(move || {
        Row(
            tagged(modifier(state.get()), "row"),
            RowSpec::default(),
            || {
                Layout(Modifier::empty(), Stretchy, || {});
            },
        );
    })
    .expect("the content composes");
    (rule, state)
}

fn row_width(rule: &mut ComposeTestRule) -> f32 {
    let root = rule
        .placed_semantics(Size::new(400.0, 400.0))
        .expect("the content lays out")
        .expect("the content places something");
    bounds(&root, "row").0
}

#[test]
fn a_changed_intrinsic_size_lays_the_content_out_at_its_new_size() {
    let (mut rule, size) = stretchy_row(IntrinsicSize::Min, |size| {
        Modifier::empty().width_intrinsic(size)
    });
    assert_eq!(row_width(&mut rule), 10.0, "the min intrinsic width");
    size.set(IntrinsicSize::Max);
    assert_eq!(
        row_width(&mut rule),
        100.0,
        "the row takes the max intrinsic width once it asks for it"
    );
}

#[test]
fn an_intrinsic_modifier_that_changes_its_axis_leaves_the_old_axis_free() {
    let (mut rule, along_width) = stretchy_row(true, |along_width| {
        if along_width {
            Modifier::empty().width_intrinsic(IntrinsicSize::Max)
        } else {
            Modifier::empty().height_intrinsic(IntrinsicSize::Max)
        }
    });
    assert_eq!(row_width(&mut rule), 100.0, "the width follows the content");
    along_width.set(false);
    assert_eq!(
        row_width(&mut rule),
        400.0,
        "the width is free again and takes all the room the layout offers"
    );
}
