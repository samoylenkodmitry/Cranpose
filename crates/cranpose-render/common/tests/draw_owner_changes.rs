use std::{cell::RefCell, rc::Rc};

use cranpose_core::{MutableState, NodeId};
use cranpose_render_common::{
    DrawOwnerChange::{Joined, Left},
    RenderScene, undrawn_nodes,
};
use cranpose_ui::{BoxSpec, Modifier};
use cranpose_ui_graphics::{Color, Size};

use crate::scene_probe::{initial_graph, update_scene};

#[test]
fn a_node_one_scene_left_and_another_joined_stays_drawn() {
    // Two scenes' changes in one frame: node 1 moved from the first scene
    // to the second, node 3 left the second, node 2 joined the first, and
    // node 4 left both.
    let changes = [
        Left(NodeId(4)),
        Left(NodeId(1)),
        Joined(NodeId(2)),
        Left(NodeId(3)),
        Joined(NodeId(1)),
        Left(NodeId(4)),
    ];
    let (mut left, mut joined) = (Vec::new(), Vec::new());
    undrawn_nodes(&changes, &mut left, &mut joined);
    assert_eq!(left, [NodeId(3), NodeId(4)]);
}

// A box that holds a square while `shown` says so.
fn toggled_square(shown: Rc<RefCell<Option<MutableState<bool>>>>) -> cranpose_ui::TestComposition {
    cranpose_ui::run_test_composition(move || {
        let state = cranpose_core::rememberMutableStateOf(|| false);
        *shown.borrow_mut() = Some(state);
        cranpose_ui::Box(
            Modifier::empty().size(Size::new(100.0, 100.0)),
            BoxSpec::default(),
            move || {
                if state.get() {
                    cranpose_ui::Box(
                        Modifier::empty()
                            .size(Size::new(40.0, 40.0))
                            .background(Color::RED),
                        BoxSpec::default(),
                        || {},
                    );
                }
            },
        );
    })
}

#[test]
fn a_node_shown_and_hidden_again_between_reads_reads_as_left() {
    let shown = Rc::default();
    let mut composition = toggled_square(Rc::clone(&shown));
    let root = composition.root().expect("a root");
    let viewport = Size::new(120.0, 120.0);
    let mut scene = initial_graph(&mut composition, root, viewport);
    let mut changes = Vec::new();
    scene.read_draw_owner_changes(&mut changes);
    changes.clear();
    let state = shown.borrow().expect("the state");

    state.set(true);
    update_scene(&mut composition, root, viewport, &mut scene);
    state.set(false);
    update_scene(&mut composition, root, viewport, &mut scene);

    assert!(
        scene.read_draw_owner_changes(&mut changes),
        "the scene can tell"
    );
    assert!(
        matches!(changes.as_slice(), [Left(_)]),
        "the square's latest change stands: {changes:?}"
    );
}
