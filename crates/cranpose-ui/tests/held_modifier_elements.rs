// A node's chain keeps the padding, size and background of its modifier in
// their nodes alone. As the modifier reorders, repeats, drops and changes
// them, the node lays out as a fresh composition of the same modifier does,
// and gives back a modifier equal to the one set.

use std::{cell::Cell, rc::Rc};

use cranpose_ui::{Box, BoxSpec, Color, LayoutBox, LayoutNode, LayoutTree, Modifier, Size};

use crate::{directional_layout::tagged, text_measured_layout_integration::layout_composition};

const VIEWPORT: Size = Size {
    width: 200.0,
    height: 200.0,
};

const RED: Color = Color(1.0, 0.0, 0.0, 1.0);
const BLUE: Color = Color(0.0, 0.0, 1.0, 1.0);

fn modifier(variant: usize) -> Modifier {
    let base = Modifier::empty();
    match variant {
        0 => base.padding(4.0).size_points(40.0, 20.0).background(RED),
        1 => base.size_points(40.0, 20.0).padding(4.0).background(RED),
        2 => base.padding(8.0).size_points(40.0, 20.0).background(BLUE),
        3 => base.background(RED).padding(4.0),
        4 => base.padding(4.0).padding(4.0).size_points(10.0, 10.0),
        _ => base.padding(6.0).rounded_corners(3.0).background(BLUE),
    }
}

// The subject, the one child of a box tagged "wrapper": a tag on the subject
// would put an element in its modifier that equals only itself.
fn subject(variant: usize) {
    Box(
        Modifier::empty().content_description("wrapper"),
        BoxSpec::default(),
        move || {
            Box(modifier(variant), BoxSpec::default(), || {
                Box(
                    Modifier::empty()
                        .size_points(12.0, 12.0)
                        .content_description("child"),
                    BoxSpec::default(),
                    || {},
                );
            });
        },
    );
}

fn subject_box(layout: &LayoutTree) -> &LayoutBox {
    &tagged(layout.root(), "wrapper").children[0]
}

fn rect(node: &LayoutBox) -> (f32, f32, f32, f32) {
    (node.rect.x, node.rect.y, node.rect.width, node.rect.height)
}

#[test]
fn a_chain_holding_elements_in_its_nodes_follows_every_change_of_its_modifier() {
    let state = Rc::new(Cell::new(None));
    let slot = Rc::clone(&state);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let variant = cranpose_core::rememberMutableStateOf(|| 0_usize);
        slot.set(Some(variant));
        subject(variant.get());
    });
    layout_composition(&mut composition, VIEWPORT);
    let variant = state.get().expect("variant state");
    for value in [1, 2, 3, 4, 5, 0, 4, 2] {
        variant.set(value);
        while composition.process_invalid_scopes().expect("recompose") {}
        let layout = layout_composition(&mut composition, VIEWPORT);
        let mut fresh = cranpose_ui::run_test_composition(move || subject(value));
        let fresh = layout_composition(&mut fresh, VIEWPORT);
        let pairs = [
            (subject_box(&layout), subject_box(&fresh)),
            (
                tagged(layout.root(), "child"),
                tagged(fresh.root(), "child"),
            ),
        ];
        for (kept, made) in pairs {
            assert_eq!(rect(kept), rect(made), "after variant {value}");
        }
        let id = subject_box(&layout).node_id;
        let held = composition
            .applier_mut()
            .with_node::<LayoutNode, _>(id, |node| node.modifier())
            .expect("the subject is a layout node");
        assert!(held == modifier(value), "the modifier of variant {value}");
    }
}
