//! A layout pass measures again the nodes that changed and walks up from
//! them only as far as each change reaches. Each frame lays out as a fresh
//! composition of the same state does: a leaf that resizes its row's other
//! children, a child of a column that reads its children's intrinsic widths,
//! a weight that changes, and changes in two places at once. The walk goes
//! up through subcompose nodes too: a `BoxWithConstraints` that keeps its
//! layout or measures again, a lazy list item that keeps its size or not,
//! and one subcompose node inside another; each pass leaves no layout to do.

use std::{cell::Cell, rc::Rc};

use cranpose_core::MutableState;
use cranpose_foundation::lazy::{LazyItems, LazyListScope, rememberLazyListState};
use cranpose_ui::{
    Box, BoxSpec, BoxWithConstraints, Column, ColumnSpec, IntrinsicSize, LayoutTree, LazyColumn,
    LazyColumnSpec, Modifier, Row, RowSpec, Size,
};

use crate::{directional_layout::tagged, text_measured_layout_integration::layout_composition};

const VIEWPORT: Size = Size {
    width: 300.0,
    height: 200.0,
};

const TAGS: [&str; 9] = [
    "row",
    "weighted",
    "inner column",
    "wrapper",
    "leaf",
    "below leaf",
    "intrinsic",
    "other",
    "filler",
];

#[derive(Clone, Copy, Debug, PartialEq)]
struct Knobs {
    leaf: f32,
    other: f32,
    weight: f32,
}

fn tagged_box(modifier: Modifier, tag: &'static str) {
    Box(modifier.content_description(tag), BoxSpec::default(), || {});
}

fn screen(knobs: Knobs) {
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            Row(
                Modifier::empty()
                    .fill_max_width()
                    .content_description("row"),
                RowSpec::default(),
                move || {
                    tagged_box(
                        Modifier::empty().weight(knobs.weight).height(10.0),
                        "weighted",
                    );
                    tagged_box(Modifier::empty().size_points(20.0, 10.0), "fixed");
                    Column(
                        Modifier::empty().content_description("inner column"),
                        ColumnSpec::default(),
                        move || {
                            Box(
                                Modifier::empty().content_description("wrapper"),
                                BoxSpec::default(),
                                move || {
                                    tagged_box(
                                        Modifier::empty().size_points(knobs.leaf, 12.0),
                                        "leaf",
                                    );
                                },
                            );
                            tagged_box(Modifier::empty().size_points(30.0, 8.0), "below leaf");
                        },
                    );
                },
            );
            Column(
                Modifier::empty()
                    .width_intrinsic(IntrinsicSize::Max)
                    .content_description("intrinsic"),
                ColumnSpec::default(),
                move || {
                    tagged_box(Modifier::empty().size_points(knobs.other, 10.0), "other");
                    tagged_box(Modifier::empty().fill_max_width().height(6.0), "filler");
                },
            );
        },
    );
}

fn rect(layout: &LayoutTree, tag: &str) -> (f32, f32, f32, f32) {
    let rect = tagged(layout.root(), tag).rect;
    (rect.x, rect.y, rect.width, rect.height)
}

// A pass leaves no node of the tree needing layout.
fn assert_no_layout_left(composition: &mut cranpose_ui::TestComposition, at: impl std::fmt::Debug) {
    let root = composition.root().expect("composition root");
    let left = cranpose_ui::tree_needs_layout(&mut *composition.applier_mut(), root);
    assert!(!left.expect("layout flags"), "layout left to do at {at:?}");
}

struct States {
    leaf: MutableState<f32>,
    other: MutableState<f32>,
    weight: MutableState<f32>,
}

#[test]
fn changed_nodes_lay_out_as_a_fresh_composition_does() {
    let first = Knobs {
        leaf: 40.0,
        other: 50.0,
        weight: 1.0,
    };
    let slot = Rc::new(Cell::new(None));
    let states = Rc::clone(&slot);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let leaf = cranpose_core::rememberMutableStateOf(|| first.leaf);
        let other = cranpose_core::rememberMutableStateOf(|| first.other);
        let weight = cranpose_core::rememberMutableStateOf(|| first.weight);
        states.set(Some(States {
            leaf,
            other,
            weight,
        }));
        screen(Knobs {
            leaf: leaf.get(),
            other: other.get(),
            weight: weight.get(),
        });
    });
    layout_composition(&mut composition, VIEWPORT);
    let states = slot.take().expect("states");
    let frames = [
        Knobs {
            leaf: 90.0,
            ..first
        },
        Knobs {
            leaf: 12.0,
            ..first
        },
        Knobs {
            leaf: 12.0,
            other: 120.0,
            ..first
        },
        Knobs {
            leaf: 12.0,
            other: 20.0,
            weight: 3.0,
        },
        Knobs {
            leaf: 70.0,
            other: 80.0,
            weight: 3.0,
        },
        Knobs {
            leaf: 70.0,
            other: 80.0,
            weight: 0.5,
        },
    ];
    for knobs in frames {
        states.leaf.set(knobs.leaf);
        states.other.set(knobs.other);
        states.weight.set(knobs.weight);
        while composition.process_invalid_scopes().expect("recompose") {}
        let layout = layout_composition(&mut composition, VIEWPORT);
        assert_no_layout_left(&mut composition, knobs);
        let mut fresh = cranpose_ui::run_test_composition(move || screen(knobs));
        let fresh = layout_composition(&mut fresh, VIEWPORT);
        for tag in TAGS {
            assert_eq!(rect(&layout, tag), rect(&fresh, tag), "{tag} at {knobs:?}");
        }
    }
}

const SLOT_VIEWPORT: Size = Size {
    width: 300.0,
    height: 320.0,
};

const SLOT_TAGS: [&str; 21] = [
    "constrained",
    "fixed content",
    "content row",
    "content leaf",
    "content sibling",
    "wrapping",
    "content root",
    "below wrapping",
    "list",
    "fixed item",
    "item leaf",
    "item sibling",
    "growing item",
    "nested row",
    "nested leaf",
    "nested sibling",
    "wrapping item",
    "wrap leaf",
    "wrap sibling",
    "last item",
    "item row",
];

/// Sizes that subcomposed content reads, so a change composes only the slot
/// that reads it and leaves its subcompose node clean.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SlotKnobs {
    /// A leaf in a row of a fixed-size `BoxWithConstraints` content.
    leaf: f32,
    /// The content of a `BoxWithConstraints` that wraps it.
    root: f32,
    /// A leaf in a row of a fixed-size lazy item.
    item_leaf: f32,
    /// A lazy item that resizes itself.
    item_height: f32,
    /// A leaf under a `BoxWithConstraints` that is a lazy item.
    nested: f32,
    /// A leaf in a row that is a lazy item.
    wrap_leaf: f32,
}

#[derive(Clone, Copy)]
struct SlotStates {
    leaf: MutableState<f32>,
    root: MutableState<f32>,
    item_leaf: MutableState<f32>,
    item_height: MutableState<f32>,
    nested: MutableState<f32>,
    wrap_leaf: MutableState<f32>,
}

impl SlotStates {
    fn remember(knobs: SlotKnobs) -> Self {
        cranpose_core::remember(|| Self {
            leaf: cranpose_core::mutableStateOf(knobs.leaf),
            root: cranpose_core::mutableStateOf(knobs.root),
            item_leaf: cranpose_core::mutableStateOf(knobs.item_leaf),
            item_height: cranpose_core::mutableStateOf(knobs.item_height),
            nested: cranpose_core::mutableStateOf(knobs.nested),
            wrap_leaf: cranpose_core::mutableStateOf(knobs.wrap_leaf),
        })
        .with(|states| *states)
    }

    fn set(&self, knobs: SlotKnobs) {
        self.leaf.set(knobs.leaf);
        self.root.set(knobs.root);
        self.item_leaf.set(knobs.item_leaf);
        self.item_height.set(knobs.item_height);
        self.nested.set(knobs.nested);
        self.wrap_leaf.set(knobs.wrap_leaf);
    }
}

/// A row of a square leaf `leaf.1` wide and a fixed sibling.
fn leaf_row(tag: &'static str, leaf: (&'static str, f32), sibling: &'static str) {
    Row(
        Modifier::empty().content_description(tag),
        RowSpec::default(),
        move || {
            tagged_box(Modifier::empty().size_points(leaf.1, leaf.1), leaf.0);
            tagged_box(Modifier::empty().size_points(15.0, 10.0), sibling);
        },
    );
}

fn lazy_item(states: SlotStates, index: usize) {
    match index {
        0 => {
            Box(
                Modifier::empty()
                    .size_points(150.0, 30.0)
                    .content_description("fixed item"),
                BoxSpec::default(),
                move || {
                    leaf_row(
                        "item row",
                        ("item leaf", states.item_leaf.get()),
                        "item sibling",
                    );
                },
            );
        }
        1 => tagged_box(
            Modifier::empty().size_points(40.0, states.item_height.get()),
            "growing item",
        ),
        2 => {
            BoxWithConstraints(Modifier::empty(), move |_| {
                leaf_row(
                    "nested row",
                    ("nested leaf", states.nested.get()),
                    "nested sibling",
                );
            });
        }
        3 => leaf_row(
            "wrapping item",
            ("wrap leaf", states.wrap_leaf.get()),
            "wrap sibling",
        ),
        _ => tagged_box(Modifier::empty().size_points(30.0, 10.0), "last item"),
    }
}

fn slot_screen(states: SlotStates) {
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            BoxWithConstraints(
                Modifier::empty().content_description("constrained"),
                move |_| {
                    Box(
                        Modifier::empty()
                            .size_points(120.0, 20.0)
                            .content_description("fixed content"),
                        BoxSpec::default(),
                        move || {
                            leaf_row(
                                "content row",
                                ("content leaf", states.leaf.get()),
                                "content sibling",
                            );
                        },
                    );
                },
            );
            BoxWithConstraints(
                Modifier::empty().content_description("wrapping"),
                move |_| {
                    tagged_box(
                        Modifier::empty().size_points(states.root.get(), 12.0),
                        "content root",
                    );
                },
            );
            tagged_box(Modifier::empty().size_points(25.0, 10.0), "below wrapping");
            LazyColumn(
                Modifier::empty()
                    .size_points(200.0, 190.0)
                    .content_description("list"),
                rememberLazyListState(),
                LazyColumnSpec::default(),
                move |scope| {
                    scope.items(LazyItems::new(5), move |index| lazy_item(states, index));
                },
            );
        },
    );
}

#[test]
fn changes_below_subcompose_nodes_lay_out_as_a_fresh_composition_does() {
    let first = SlotKnobs {
        leaf: 30.0,
        root: 60.0,
        item_leaf: 20.0,
        item_height: 20.0,
        nested: 10.0,
        wrap_leaf: 10.0,
    };
    let slot = Rc::new(Cell::new(None));
    let states = Rc::clone(&slot);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let remembered = SlotStates::remember(first);
        states.set(Some(remembered));
        slot_screen(remembered);
    });
    layout_composition(&mut composition, SLOT_VIEWPORT);
    let states = slot.take().expect("states");
    let mut frames = Vec::new();
    let mut knobs = first;
    let changes: [fn(&mut SlotKnobs); 10] = [
        |knobs| knobs.leaf = 50.0,
        |knobs| knobs.root = 90.0,
        |knobs| knobs.item_leaf = 45.0,
        |knobs| knobs.item_height = 35.0,
        |knobs| knobs.nested = 30.0,
        |knobs| knobs.wrap_leaf = 25.0,
        |knobs| {
            knobs.leaf = 20.0;
            knobs.item_leaf = 10.0;
        },
        |knobs| {
            knobs.root = 40.0;
            knobs.item_height = 15.0;
            knobs.nested = 5.0;
            knobs.wrap_leaf = 40.0;
        },
        |knobs| knobs.item_leaf = 30.0,
        |knobs| knobs.item_height = 25.0,
    ];
    for change in changes {
        change(&mut knobs);
        frames.push(knobs);
    }
    for knobs in frames {
        states.set(knobs);
        while composition.process_invalid_scopes().expect("recompose") {}
        let layout = layout_composition(&mut composition, SLOT_VIEWPORT);
        assert_no_layout_left(&mut composition, knobs);
        let mut fresh = cranpose_ui::run_test_composition(move || {
            slot_screen(SlotStates::remember(knobs));
        });
        let fresh = layout_composition(&mut fresh, SLOT_VIEWPORT);
        for tag in SLOT_TAGS {
            assert_eq!(rect(&layout, tag), rect(&fresh, tag), "{tag} at {knobs:?}");
        }
    }
}
