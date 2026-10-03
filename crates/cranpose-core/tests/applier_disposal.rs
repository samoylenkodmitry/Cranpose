use std::{
    cell::Cell,
    rc::{Rc, Weak},
};

use cranpose_core::{
    Applier, ApplierHost, ConcreteApplierHost, MemoryApplier, Node, NodeDisposal, NodeId,
};

struct DropCount(Rc<Cell<usize>>);

impl Node for DropCount {}

impl Drop for DropCount {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

struct CascadeOnDrop {
    host: Weak<ConcreteApplierHost<MemoryApplier>>,
    child: (NodeId, u32),
    drops: Rc<Cell<usize>>,
}

impl Node for CascadeOnDrop {}

impl Drop for CascadeOnDrop {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
        if let Some(host) = self.host.upgrade() {
            let _ = host.dispose_nodes(NodeDisposal::new(vec![self.child]));
        }
    }
}

fn cascading_nodes(
    host: &Rc<ConcreteApplierHost<MemoryApplier>>,
) -> ((NodeId, u32), Rc<Cell<usize>>, Rc<Cell<usize>>) {
    let child_drops = Rc::new(Cell::new(0));
    let trigger_drops = Rc::new(Cell::new(0));
    let mut applier = host.borrow_typed();
    let child = applier.create(Box::new(DropCount(Rc::clone(&child_drops))));
    let child_generation = applier.node_generation(child);
    let trigger = applier.create(Box::new(CascadeOnDrop {
        host: Rc::downgrade(host),
        child: (child, child_generation),
        drops: Rc::clone(&trigger_drops),
    }));
    (
        (trigger, applier.node_generation(trigger)),
        trigger_drops,
        child_drops,
    )
}

fn assert_cascade_released(trigger_drops: &Cell<usize>, child_drops: &Cell<usize>) {
    assert_eq!(trigger_drops.get(), 1, "trigger resource drops once");
    assert_eq!(child_drops.get(), 1, "cascaded resource drops once");
}

fn assert_deferred_disposal<G>(
    host: &ConcreteApplierHost<MemoryApplier>,
    trigger: (NodeId, u32),
    guard: G,
    drops: (&Cell<usize>, &Cell<usize>),
) {
    host.dispose_nodes(NodeDisposal::new(vec![trigger]))
        .expect("queue trigger disposal");
    assert_eq!(drops.0.get(), 0);
    assert_eq!(drops.1.get(), 0);
    drop(guard);
    assert_cascade_released(drops.0, drops.1);
}

#[test]
fn nested_disposal_waits_for_dynamic_applier_borrow_to_end() {
    let host = Rc::new(ConcreteApplierHost::new(MemoryApplier::new()));
    let (trigger, trigger_drops, child_drops) = cascading_nodes(&host);

    assert_deferred_disposal(
        &host,
        trigger,
        host.borrow_dyn(),
        (&trigger_drops, &child_drops),
    );
}

#[test]
fn nested_disposal_waits_for_typed_applier_borrow_to_end() {
    let host = Rc::new(ConcreteApplierHost::new(MemoryApplier::new()));
    let (trigger, trigger_drops, child_drops) = cascading_nodes(&host);

    assert_deferred_disposal(
        &host,
        trigger,
        host.borrow_typed(),
        (&trigger_drops, &child_drops),
    );
}

#[test]
fn nested_disposal_waits_for_try_borrowed_applier_to_end() {
    let host = Rc::new(ConcreteApplierHost::new(MemoryApplier::new()));
    let (trigger, trigger_drops, child_drops) = cascading_nodes(&host);

    assert_deferred_disposal(
        &host,
        trigger,
        host.try_borrow_typed()
            .expect("acquire typed applier borrow"),
        (&trigger_drops, &child_drops),
    );
}

#[test]
fn direct_disposal_without_an_outstanding_borrow_releases_resources() {
    let host = Rc::new(ConcreteApplierHost::new(MemoryApplier::new()));
    let (trigger, trigger_drops, child_drops) = cascading_nodes(&host);

    host.dispose_nodes(NodeDisposal::new(vec![trigger]))
        .expect("dispose node without an outstanding borrow");

    assert_cascade_released(&trigger_drops, &child_drops);
}

#[test]
fn deferred_disposal_preserves_a_new_node_that_reuses_the_same_id() {
    let host = ConcreteApplierHost::new(MemoryApplier::new());
    let first_drops = Rc::new(Cell::new(0));
    let replacement_drops = Rc::new(Cell::new(0));
    let mut applier = host.borrow_typed();
    let first = applier.create(Box::new(DropCount(Rc::clone(&first_drops))));
    let generation = applier.node_generation(first);
    host.dispose_nodes(NodeDisposal::new(vec![(first, generation)]))
        .expect("defer first node disposal");
    applier.remove(first).expect("remove the old node");
    let replacement = applier
        .insert_recycled_node_or_create(first, Box::new(DropCount(Rc::clone(&replacement_drops))))
        .id;
    assert_eq!(first, replacement, "fixture reuses the freed ID");
    drop(applier);

    assert_eq!(first_drops.get(), 1);
    assert_eq!(replacement_drops.get(), 0);
    assert!(host.borrow_typed().get_mut(replacement).is_ok());
}
