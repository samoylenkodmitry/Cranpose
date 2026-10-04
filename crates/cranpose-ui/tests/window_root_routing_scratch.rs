use std::{cell::Cell, rc::Rc};

use cranpose_core::{Applier, NodeId};
use cranpose_ui::{
    Box, BoxSpec, Modifier, TestComposition, WindowRootDescriptor, WindowRootRoutingScratch,
    nearest_window_roots_into, run_test_composition,
};
use cranpose_ui_graphics::Size;

struct TestWindow;

impl WindowRootDescriptor for TestWindow {
    fn layout_size(&self) -> Size {
        Size::new(200.0, 120.0)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn capture_node(slot: &Cell<Option<NodeId>>, node: NodeId) {
    slot.set(Some(node));
}

fn node(slot: &Cell<Option<NodeId>>, label: &str) -> NodeId {
    slot.get()
        .unwrap_or_else(|| panic!("{label} was not composed"))
}

fn move_child(composition: &mut TestComposition, child: NodeId, parent: NodeId) {
    let mut applier = composition.applier_mut();
    let old_parent = applier
        .get_mut(child)
        .expect("child node exists")
        .parent()
        .expect("child has a parent");
    assert!(
        applier
            .get_mut(old_parent)
            .expect("old parent exists")
            .remove_child(child),
        "old parent contains the child"
    );
    applier
        .get_mut(child)
        .expect("child node exists")
        .on_removed_from_parent();
    assert!(
        applier
            .get_mut(parent)
            .expect("new parent exists")
            .insert_child(child),
        "new parent did not already contain the child"
    );
    applier
        .get_mut(child)
        .expect("child node exists")
        .on_attached_to_parent(parent);
}

#[test]
fn one_routing_scratch_tracks_reparenting_across_empty_batches() {
    let root_slot = Rc::new(Cell::new(None));
    let first_window_slot = Rc::new(Cell::new(None));
    let second_window_slot = Rc::new(Cell::new(None));
    let child_slot = Rc::new(Cell::new(None));
    let root_capture = Rc::clone(&root_slot);
    let first_capture = Rc::clone(&first_window_slot);
    let second_capture = Rc::clone(&second_window_slot);
    let child_capture = Rc::clone(&child_slot);

    let mut composition = run_test_composition(move || {
        let first_capture = Rc::clone(&first_capture);
        let second_capture = Rc::clone(&second_capture);
        let child_capture = Rc::clone(&child_capture);
        let root = Box(Modifier::empty(), BoxSpec::default(), move || {
            let first_capture = Rc::clone(&first_capture);
            let child_capture = Rc::clone(&child_capture);
            let first = Box(
                Modifier::empty().window_root(Rc::new(TestWindow)),
                BoxSpec::default(),
                move || {
                    let child_capture = Rc::clone(&child_capture);
                    let child = Box(Modifier::empty(), BoxSpec::default(), || {});
                    capture_node(&child_capture, child);
                },
            );
            capture_node(&first_capture, first);

            let second_capture = Rc::clone(&second_capture);
            let second = Box(
                Modifier::empty().window_root(Rc::new(TestWindow)),
                BoxSpec::default(),
                || {},
            );
            capture_node(&second_capture, second);
        });
        capture_node(&root_capture, root);
    });

    let root = node(&root_slot, "primary root");
    let first_window = node(&first_window_slot, "first window root");
    let second_window = node(&second_window_slot, "second window root");
    let child = node(&child_slot, "window child");
    let mut scratch = WindowRootRoutingScratch::default();
    let mut owners = Vec::new();

    {
        let mut applier = composition.applier_mut();
        nearest_window_roots_into(
            &mut applier,
            [child, child, first_window, root],
            &mut owners,
            &mut scratch,
        );
        assert_eq!(
            owners,
            [
                Some(first_window),
                Some(first_window),
                Some(first_window),
                None
            ],
            "the first window owns its descendants and itself"
        );

        nearest_window_roots_into(&mut applier, [], &mut owners, &mut scratch);
        assert!(owners.is_empty(), "an empty batch clears prior results");
    }

    move_child(&mut composition, child, second_window);
    {
        let mut applier = composition.applier_mut();
        nearest_window_roots_into(
            &mut applier,
            [child, first_window, second_window, root],
            &mut owners,
            &mut scratch,
        );
        assert_eq!(
            owners,
            [
                Some(second_window),
                Some(first_window),
                Some(second_window),
                None
            ],
            "the same node id follows its new window owner"
        );
    }

    move_child(&mut composition, child, root);
    let mut applier = composition.applier_mut();
    nearest_window_roots_into(&mut applier, [child], &mut owners, &mut scratch);
    assert_eq!(
        owners,
        [None],
        "reparenting into the primary tree clears the previous window owner"
    );
}
