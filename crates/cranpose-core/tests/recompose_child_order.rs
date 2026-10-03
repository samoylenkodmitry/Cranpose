use std::{cell::Cell, rc::Rc};

use cranpose_core::{
    Applier, Composition, MemoryApplier, MutableState, Node, NodeId, pop_parent, push_parent,
    with_current_composer,
};
#[path = "support/ordered_parent.rs"]
mod ordered_parent;

use ordered_parent::{OrderedParent, ParentTracked};

struct LabelNode {
    label: &'static str,
}
impl ordered_parent::FixtureLeaf for LabelNode {}

#[cranpose_macros::composable]
fn Screen(
    add_before_a: MutableState<bool>,
    add_after_virtual: MutableState<bool>,
    parent_id: Rc<Cell<Option<NodeId>>>,
) {
    let parent = with_current_composer(|composer| composer.emit_node(OrderedParent::default));
    parent_id.set(Some(parent));
    push_parent(parent);
    RowA(add_before_a);
    RowB(add_after_virtual);
    pop_parent();
}

#[cranpose_macros::composable]
fn RowA(add_extra: MutableState<bool>) {
    if add_extra.get() {
        Child("a-extra");
    }
    Child("a");
}

#[cranpose_macros::composable]
fn RowB(add_extra: MutableState<bool>) {
    if add_extra.get() {
        Child("b-extra");
    }
    Child("b");
}

#[cranpose_macros::composable]
fn Child(label: &'static str) {
    with_current_composer(|composer| {
        composer.emit_node(|| ParentTracked::new(LabelNode { label }));
    });
}

fn insert_virtual_child(
    composition: &mut Composition<MemoryApplier>,
    parent: NodeId,
    node_id: NodeId,
    label: &'static str,
    index: usize,
) {
    let mut applier = composition.applier_mut();
    applier
        .insert_with_id(node_id, Box::new(ParentTracked::new(LabelNode { label })))
        .expect("virtual child ID is available");
    applier
        .get_mut(node_id)
        .expect("virtual child was inserted")
        .on_attached_to_parent(parent);
    applier
        .with_node::<OrderedParent, _>(parent, |parent| {
            parent.hidden_children.push(node_id);
            parent.insert_child(node_id);
            parent.move_child(parent.children.len() - 1, index);
        })
        .expect("composition root is the virtual parent's parent");
}

fn child_labels(composition: &mut Composition<MemoryApplier>, parent: NodeId) -> Vec<&'static str> {
    let mut applier = composition.applier_mut();
    let children = applier
        .with_node::<OrderedParent, _>(parent, |parent| parent.children.clone())
        .expect("composition root exists");
    children
        .into_iter()
        .map(|child| {
            applier
                .with_node::<ParentTracked<LabelNode>, _>(child, |node| node.node.label)
                .expect("ordered child exists")
        })
        .collect()
}

#[test]
fn recomposition_preserves_order_around_virtual_children() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let add_before_a = MutableState::with_runtime(false, runtime.clone());
    let add_after_virtual = MutableState::with_runtime(false, runtime);
    let parent_id = Rc::new(Cell::new(None));
    let root_key = cranpose_core::location_key(file!(), line!(), column!());

    composition
        .render(root_key, || {
            Screen(add_before_a, add_after_virtual, Rc::clone(&parent_id));
        })
        .expect("initial composition succeeds");
    let parent = parent_id.get().expect("composition emitted its parent");
    insert_virtual_child(
        &mut composition,
        parent,
        usize::MAX - 10,
        "virtual-before-b",
        1,
    );
    insert_virtual_child(
        &mut composition,
        parent,
        usize::MAX - 11,
        "virtual-after-b",
        3,
    );
    assert_eq!(
        child_labels(&mut composition, parent),
        ["a", "virtual-before-b", "b", "virtual-after-b"]
    );

    add_before_a.set(true);
    composition
        .process_invalid_scopes()
        .expect("first row recomposes");
    assert_eq!(
        child_labels(&mut composition, parent),
        ["a-extra", "a", "virtual-before-b", "b", "virtual-after-b"]
    );
    add_after_virtual.set(true);
    composition
        .process_invalid_scopes()
        .expect("second row recomposes");
    assert_eq!(
        child_labels(&mut composition, parent),
        [
            "a-extra",
            "a",
            "virtual-before-b",
            "b-extra",
            "b",
            "virtual-after-b"
        ]
    );
    add_before_a.set(false);
    composition
        .process_invalid_scopes()
        .expect("first extra is removed");
    add_after_virtual.set(false);
    composition
        .process_invalid_scopes()
        .expect("second extra is removed");
    assert_eq!(
        child_labels(&mut composition, parent),
        ["a", "virtual-before-b", "b", "virtual-after-b"]
    );
}
