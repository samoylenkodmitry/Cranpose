use std::{
    cell::Cell,
    rc::{Rc, Weak},
};

use cranpose_core::{
    Applier, ApplierHost, Composer, ConcreteApplierHost, DefaultScheduler, Key, MemoryApplier,
    Node, NodeDisposal, NodeError, NodeId, Phase, RecomposeOptions, Runtime, SlotTable, SlotsHost,
    scheduler_ref,
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

struct RetryApplier {
    inner: MemoryApplier,
    fail_disposal: Rc<Cell<bool>>,
}

impl Applier for RetryApplier {
    fn create(&mut self, node: Box<dyn Node>) -> NodeId {
        self.inner.create(node)
    }

    fn get_mut(&mut self, id: NodeId) -> Result<&mut dyn Node, NodeError> {
        if self.fail_disposal.get() {
            return Err(NodeError::TypeMismatch {
                id,
                expected: "retry gate is closed",
            });
        }
        self.inner.get_mut(id)
    }

    fn remove(&mut self, id: NodeId) -> Result<(), NodeError> {
        if self.fail_disposal.get() {
            return Err(NodeError::TypeMismatch {
                id,
                expected: "retry gate is closed",
            });
        }
        self.inner.remove(id)
    }

    fn node_generation(&self, id: NodeId) -> u32 {
        self.inner.node_generation(id)
    }

    fn insert_with_id(&mut self, id: NodeId, node: Box<dyn Node>) -> Result<(), NodeError> {
        self.inner.insert_with_id(id, node)
    }
}

struct RememberedResource(Rc<Cell<usize>>);

impl Drop for RememberedResource {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

struct UnmountCount(Rc<Cell<usize>>);

impl Node for UnmountCount {
    fn unmount(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

struct RetainedHostFixture {
    composer: Composer,
    secondary_slots: Rc<SlotsHost>,
    host: Rc<ConcreteApplierHost<RetryApplier>>,
    runtime: Runtime,
    fail_disposal: Rc<Cell<bool>>,
    resource_drops: Rc<Cell<usize>>,
    unmounts: Rc<Cell<usize>>,
    retained_node: NodeId,
}

fn retained_host_fixture() -> RetainedHostFixture {
    const BRANCH_KEY: Key = 0x5ec0;

    let fail_disposal = Rc::new(Cell::new(false));
    let host = Rc::new(ConcreteApplierHost::new(RetryApplier {
        inner: MemoryApplier::new(),
        fail_disposal: Rc::clone(&fail_disposal),
    }));
    let root_slots = Rc::new(SlotsHost::new(SlotTable::default()));
    let secondary_slots = Rc::new(SlotsHost::new(SlotTable::default()));
    let applier: Rc<dyn ApplierHost> = host.clone();
    let runtime = Runtime::new(scheduler_ref(DefaultScheduler));
    let composer = Composer::new(root_slots, applier, runtime.handle(), None);
    composer.enter_phase(Phase::Measure);

    let resource_drops = Rc::new(Cell::new(0));
    let unmounts = Rc::new(Cell::new(0));
    let emitted_node = Rc::new(Cell::new(None));
    let subcompose = |show_branch: bool| {
        let resource_drops = Rc::clone(&resource_drops);
        let unmounts = Rc::clone(&unmounts);
        let emitted_node = Rc::clone(&emitted_node);
        composer
            .subcompose_slot(&secondary_slots, None, |composer| {
                if show_branch {
                    composer.cranpose_with_reuse(
                        BRANCH_KEY,
                        RecomposeOptions::default(),
                        |composer| {
                            let _resource = composer
                                .remember(|| RememberedResource(Rc::clone(&resource_drops)));
                            emitted_node.set(Some(
                                composer.emit_node(|| UnmountCount(Rc::clone(&unmounts))),
                            ));
                        },
                    );
                }
            })
            .expect("subcompose retained branch");
    };

    subcompose(true);
    let retained_node = emitted_node.get().expect("branch emits a node");
    subcompose(false);

    RetainedHostFixture {
        composer,
        secondary_slots,
        host,
        runtime,
        fail_disposal,
        resource_drops,
        unmounts,
        retained_node,
    }
}

#[test]
fn failed_retained_host_reset_keeps_owned_disposal_for_retry() {
    let fixture = retained_host_fixture();
    let RetainedHostFixture {
        composer: _composer,
        secondary_slots,
        host,
        runtime: _runtime,
        fail_disposal,
        resource_drops,
        unmounts,
        retained_node,
    } = fixture;

    assert_eq!(resource_drops.get(), 0);
    assert_eq!(unmounts.get(), 0);

    fail_disposal.set(true);
    assert!(secondary_slots.reset().is_err());
    assert_eq!(
        resource_drops.get(),
        0,
        "failed disposal keeps remembered state"
    );
    assert_eq!(
        unmounts.get(),
        0,
        "failed disposal has not unmounted the node"
    );
    assert!(host.borrow_typed().inner.get_mut(retained_node).is_ok());

    assert!(
        secondary_slots.reset().is_err(),
        "a retry must still report failure"
    );
    assert_eq!(
        resource_drops.get(),
        0,
        "repeated failure keeps state alive"
    );
    assert_eq!(unmounts.get(), 0, "repeated failure does not unmount");

    fail_disposal.set(false);
    secondary_slots
        .reset()
        .expect("reset retries queued disposal");
    assert_eq!(resource_drops.get(), 1, "remembered state releases once");
    assert_eq!(unmounts.get(), 1, "retained node unmounts once");
    assert!(host.borrow_typed().inner.get_mut(retained_node).is_err());
}

#[test]
fn failed_host_extraction_preserves_disposal_ownership_for_retry() {
    let RetainedHostFixture {
        composer,
        secondary_slots,
        host,
        runtime: _runtime,
        fail_disposal,
        resource_drops,
        unmounts,
        retained_node,
    } = retained_host_fixture();
    fail_disposal.set(true);
    assert!(secondary_slots.reset().is_err());
    drop(composer);
    drop(secondary_slots);

    let host = Rc::try_unwrap(host)
        .unwrap_or_else(|_| panic!("fixture left outstanding applier references"));
    let (host, error) = match host.try_into_inner() {
        Err(failed) => failed,
        Ok(_) => panic!("extraction must report the queued disposal failure"),
    };
    assert!(matches!(error, NodeError::TypeMismatch { .. }));
    assert_eq!(
        resource_drops.get(),
        0,
        "failed extraction keeps remembered state"
    );
    assert_eq!(
        unmounts.get(),
        0,
        "failed extraction leaves the node mounted"
    );
    assert!(host.borrow_typed().inner.get_mut(retained_node).is_ok());

    fail_disposal.set(false);
    let mut applier = match host.try_into_inner() {
        Ok(applier) => applier,
        Err((_, error)) => panic!("disposal retry failed: {error}"),
    };
    assert_eq!(
        resource_drops.get(),
        1,
        "successful extraction releases state once"
    );
    assert_eq!(unmounts.get(), 1, "successful extraction unmounts once");
    assert!(applier.inner.get_mut(retained_node).is_err());
}
