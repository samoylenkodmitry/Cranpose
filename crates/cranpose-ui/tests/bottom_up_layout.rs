//! A layout pass measures again the nodes that changed and walks up from
//! them only as far as each change reaches. Each frame lays out as a fresh
//! composition of the same state does: a leaf that resizes its row's other
//! children, a child of a column that reads its children's intrinsic widths,
//! a weight that changes, and changes in two places at once.

use std::{cell::Cell, rc::Rc};

use cranpose_core::MutableState;
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, IntrinsicSize, LayoutTree, Modifier, Row, RowSpec, Size,
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
        let mut fresh = cranpose_ui::run_test_composition(move || screen(knobs));
        let fresh = layout_composition(&mut fresh, VIEWPORT);
        for tag in TAGS {
            assert_eq!(rect(&layout, tag), rect(&fresh, tag), "{tag} at {knobs:?}");
        }
    }
}
