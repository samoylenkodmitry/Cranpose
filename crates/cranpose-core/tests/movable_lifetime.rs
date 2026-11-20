use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{
    Applier, Composition, DisposableEffect, DisposableEffectResult, Key, LaunchedEffect,
    LaunchedEffectScope, MemoryApplier, MovableContent, MutableState, Node, NodeId, RuntimeCell,
    SlotId, SlotTable, SlotsHost, SubcomposeState, location_key, movable, pop_parent, push_parent,
    remember, rememberMovableContentOf, with_current_composer,
};

use crate::tracked_parent::TrackedParent;

struct DropCounter(Rc<Cell<usize>>);

impl Drop for DropCounter {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

struct CellCheckingNode {
    parent: Option<NodeId>,
    cells: Vec<RuntimeCell<Cell<usize>>>,
    unmounts: Rc<Cell<usize>>,
    watched_drops: Option<Rc<Cell<usize>>>,
    unmount_saw_watched_dropped: Rc<Cell<bool>>,
}

impl Node for CellCheckingNode {
    fn unmount(&mut self) {
        for cell in &self.cells {
            cell.with(|value| value.set(value.get() + 1));
        }
        if let Some(watched) = &self.watched_drops
            && watched.get() > 0
        {
            self.unmount_saw_watched_dropped.set(true);
        }
        self.unmounts.set(self.unmounts.get() + 1);
    }

    fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    fn on_attached_to_parent(&mut self, parent: NodeId) {
        self.parent = Some(parent);
    }

    fn on_removed_from_parent(&mut self) {
        self.parent = None;
    }
}

#[derive(Default)]
struct LifecycleProbe {
    node: Cell<Option<NodeId>>,
    unmounts: Rc<Cell<usize>>,
    payload_drops: Rc<Cell<usize>>,
    disposals: Rc<Cell<usize>>,
    launched: RefCell<Vec<LaunchedEffectScope>>,
    unmount_saw_watched_dropped: Rc<Cell<bool>>,
}

impl LifecycleProbe {
    fn emit_movable_body(
        self: &Rc<Self>,
        cells: Vec<RuntimeCell<Cell<usize>>>,
        watched_drops: Option<Rc<Cell<usize>>>,
    ) {
        let probe = Rc::clone(self);
        with_current_composer(|composer| {
            let _payload = composer.remember(|| DropCounter(Rc::clone(&probe.payload_drops)));
            let node = composer.emit_node(|| CellCheckingNode {
                parent: None,
                cells,
                unmounts: Rc::clone(&probe.unmounts),
                watched_drops,
                unmount_saw_watched_dropped: Rc::clone(&probe.unmount_saw_watched_dropped),
            });
            probe.node.set(Some(node));
        });
        let disposals = Rc::clone(&self.disposals);
        DisposableEffect((), move |_| {
            DisposableEffectResult::new(move || disposals.set(disposals.get() + 1))
        });
        let launched = Rc::clone(self);
        LaunchedEffect((), async move |scope| {
            launched.launched.borrow_mut().push(scope);
        });
    }

    fn last_launched_active(&self) -> bool {
        self.launched
            .borrow()
            .last()
            .expect("launched effect should have run")
            .is_active()
    }

    fn assert_disposed(&self, composition: &mut Composition<MemoryApplier>, node: NodeId) {
        assert_eq!(self.unmounts.get(), 1, "movable node unmounts once");
        assert_eq!(self.payload_drops.get(), 1, "movable payloads drop once");
        assert_eq!(
            self.disposals.get(),
            1,
            "movable disposable effect cleans up once"
        );
        assert!(
            !self.last_launched_active(),
            "movable launched effect cancels on disposal"
        );
        assert!(
            composition.applier_mut().get_mut(node).is_err(),
            "disposed movable node leaves the applier"
        );
    }
}

fn emit_wrapper(body: impl FnOnce()) {
    let parent = with_current_composer(|composer| composer.emit_node(TrackedParent::default));
    push_parent(parent);
    body();
    pop_parent();
}

#[test]
fn remember_movable_content_disposes_when_owner_leaves_while_sibling_runtime_cell_is_alive() {
    let mut composition = Composition::new(MemoryApplier::new());
    let show = MutableState::with_runtime(true, composition.runtime_handle());
    let probe = Rc::new(LifecycleProbe::default());
    let root_key = location_key(file!(), line!(), column!());

    let render = |composition: &mut Composition<MemoryApplier>| {
        let probe = Rc::clone(&probe);
        composition
            .render(root_key, move || {
                if show.value() {
                    let cell = remember(|| RuntimeCell::new(Cell::new(0))).with(|c| *c);
                    let content = rememberMovableContentOf({
                        let probe = Rc::clone(&probe);
                        move || probe.emit_movable_body(vec![cell], None)
                    });
                    emit_wrapper(|| {
                        emit_wrapper(|| content.show());
                    });
                }
            })
            .expect("render composition");
    };

    render(&mut composition);
    let first_node = probe.node.get().expect("movable node composed");
    assert_eq!(probe.unmounts.get(), 0);
    assert_eq!(probe.payload_drops.get(), 0);
    assert_eq!(probe.disposals.get(), 0);
    assert!(probe.last_launched_active());

    show.set_value(false);
    render(&mut composition);
    probe.assert_disposed(&mut composition, first_node);

    show.set_value(true);
    render(&mut composition);
    let second_node = probe.node.get().expect("movable node recomposed");
    assert_ne!(
        second_node, first_node,
        "re-entering composes a fresh movable instance"
    );
}

#[test]
fn remember_movable_content_already_retained_disposes_when_owner_leaves() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let show_owner = MutableState::with_runtime(true, runtime.clone());
    let show_inner = MutableState::with_runtime(true, runtime);
    let probe = Rc::new(LifecycleProbe::default());
    let root_key = location_key(file!(), line!(), column!());

    let render = |composition: &mut Composition<MemoryApplier>| {
        let probe = Rc::clone(&probe);
        composition
            .render(root_key, move || {
                if show_owner.value() {
                    let cell = remember(|| RuntimeCell::new(Cell::new(0))).with(|c| *c);
                    let content = rememberMovableContentOf({
                        let probe = Rc::clone(&probe);
                        move || probe.emit_movable_body(vec![cell], None)
                    });
                    if show_inner.value() {
                        emit_wrapper(|| content.show());
                    }
                }
            })
            .expect("render composition");
    };

    render(&mut composition);
    let node = probe.node.get().expect("movable node composed");

    // Hide the movable while its owner stays alive: it remains retained.
    show_inner.set_value(false);
    render(&mut composition);
    assert_eq!(probe.unmounts.get(), 0);
    assert_eq!(probe.payload_drops.get(), 0);
    assert!(composition.applier_mut().get_mut(node).is_ok());

    // Now remove the owner: the retained movable must unmount while the
    // sibling RuntimeCell is still alive.
    show_owner.set_value(false);
    render(&mut composition);
    probe.assert_disposed(&mut composition, node);
}

#[test]
fn remember_movable_content_keeps_state_across_passes_while_owner_lives() {
    let mut composition = Composition::new(MemoryApplier::new());
    let slot = MutableState::with_runtime(0u8, composition.runtime_handle());
    let probe = Rc::new(LifecycleProbe::default());
    let root_key = location_key(file!(), line!(), column!());

    let render = |composition: &mut Composition<MemoryApplier>| {
        let probe = Rc::clone(&probe);
        composition
            .render(root_key, move || {
                let cell = remember(|| RuntimeCell::new(Cell::new(0))).with(|c| *c);
                let content = rememberMovableContentOf({
                    let probe = Rc::clone(&probe);
                    move || probe.emit_movable_body(vec![cell], None)
                });
                let which = slot.value();
                emit_wrapper(|| {
                    if which == 0 {
                        content.show();
                    }
                });
                emit_wrapper(|| {
                    if which == 2 {
                        content.show();
                    }
                });
            })
            .expect("render composition");
    };

    render(&mut composition);
    let node = probe.node.get().expect("node in first parent");

    // Intermediate pass where neither parent shows it: owner stays alive.
    slot.set_value(1);
    render(&mut composition);
    assert_eq!(probe.unmounts.get(), 0);
    assert_eq!(probe.payload_drops.get(), 0);
    assert!(probe.last_launched_active());

    // Second parent shows it a pass later: same node and live effects.
    slot.set_value(2);
    render(&mut composition);
    assert_eq!(probe.node.get(), Some(node));
    assert_eq!(probe.unmounts.get(), 0);
    assert_eq!(probe.payload_drops.get(), 0);
    assert!(probe.last_launched_active());
}

#[test]
fn remember_movable_content_moved_to_subcomposition_disposes_when_target_detaches_after_owner_left()
{
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let show_owner = MutableState::with_runtime(true, runtime.clone());
    let show_in_sub = MutableState::with_runtime(true, runtime);
    let sub_slots = Rc::new(SlotsHost::new(SlotTable::new()));
    let shared_movable: Rc<RefCell<Option<MovableContent>>> = Rc::new(RefCell::new(None));
    let probe = Rc::new(LifecycleProbe::default());
    let root_key = location_key(file!(), line!(), column!());

    let render = |composition: &mut Composition<MemoryApplier>| {
        let sub_slots = Rc::clone(&sub_slots);
        let shared_movable = Rc::clone(&shared_movable);
        let probe = Rc::clone(&probe);
        composition
            .render(root_key, move || {
                if show_owner.value() {
                    let content = rememberMovableContentOf({
                        let probe = Rc::clone(&probe);
                        move || probe.emit_movable_body(Vec::new(), None)
                    });
                    *shared_movable.borrow_mut() = Some(content);
                }
                let host =
                    with_current_composer(|composer| composer.emit_node(TrackedParent::default));
                let content = shared_movable.borrow().clone();
                with_current_composer(|composer| {
                    composer
                        .subcompose_in(&sub_slots, Some(host), |_| {
                            if show_in_sub.value()
                                && let Some(content) = content
                            {
                                content.show();
                            }
                        })
                        .expect("subcompose_in");
                });
            })
            .expect("render composition");
    };

    render(&mut composition);
    let node = probe.node.get().expect("movable node in subcomposition");
    assert_eq!(probe.unmounts.get(), 0);

    // Owner leaves while the subcomposition still displays the movable.
    show_owner.set_value(false);
    render(&mut composition);
    assert_eq!(
        probe.unmounts.get(),
        0,
        "attached movable stays alive until its host detaches it"
    );

    // Subcomposition stops showing it: must dispose rather than leak pinned.
    show_in_sub.set_value(false);
    render(&mut composition);
    probe.assert_disposed(&mut composition, node);
}

#[test]
fn nested_remember_movable_content_disposes_completely_with_outer_owner() {
    let mut composition = Composition::new(MemoryApplier::new());
    let show = MutableState::with_runtime(true, composition.runtime_handle());
    let inner_probe = Rc::new(LifecycleProbe::default());
    let root_key = location_key(file!(), line!(), column!());

    let render = |composition: &mut Composition<MemoryApplier>| {
        let inner_probe = Rc::clone(&inner_probe);
        composition
            .render(root_key, move || {
                if show.value() {
                    let outer_cell = remember(|| RuntimeCell::new(Cell::new(0))).with(|c| *c);
                    let outer = rememberMovableContentOf({
                        let inner_probe = Rc::clone(&inner_probe);
                        move || {
                            let inner_cell =
                                remember(|| RuntimeCell::new(Cell::new(0))).with(|c| *c);
                            let inner = rememberMovableContentOf({
                                let inner_probe = Rc::clone(&inner_probe);
                                move || {
                                    inner_probe
                                        .emit_movable_body(vec![outer_cell, inner_cell], None);
                                }
                            });
                            emit_wrapper(|| inner.show());
                        }
                    });
                    emit_wrapper(|| outer.show());
                }
            })
            .expect("render composition");
    };

    render(&mut composition);
    let inner_node = inner_probe.node.get().expect("inner movable composed");

    show.set_value(false);
    render(&mut composition);
    inner_probe.assert_disposed(&mut composition, inner_node);
}

struct TwoHostRig {
    composition: Composition<MemoryApplier>,
    show_in_b: MutableState<bool>,
    host_a: Rc<RefCell<Option<Rc<SlotsHost>>>>,
    host_b: Rc<SlotsHost>,
    shared_movable: Rc<RefCell<Option<MovableContent>>>,
    probe: Rc<LifecycleProbe>,
    root_key: Key,
}

impl TwoHostRig {
    fn new(root_key: Key) -> Self {
        let composition = Composition::new(MemoryApplier::new());
        let show_in_b = MutableState::with_runtime(true, composition.runtime_handle());
        Self {
            composition,
            show_in_b,
            host_a: Rc::new(RefCell::new(Some(Rc::new(
                SlotsHost::new(SlotTable::new()),
            )))),
            host_b: Rc::new(SlotsHost::new(SlotTable::new())),
            shared_movable: Rc::new(RefCell::new(None)),
            probe: Rc::new(LifecycleProbe::default()),
            root_key,
        }
    }

    fn render(
        &mut self,
        host_a_body: impl Fn(&Rc<RefCell<Option<MovableContent>>>, &Rc<LifecycleProbe>) + 'static,
    ) {
        let host_a = Rc::clone(&self.host_a);
        let host_b = Rc::clone(&self.host_b);
        let shared_movable = Rc::clone(&self.shared_movable);
        let probe = Rc::clone(&self.probe);
        let show_in_b = self.show_in_b;
        self.composition
            .render(self.root_key, move || {
                let root_a =
                    with_current_composer(|composer| composer.emit_node(TrackedParent::default));
                let root_b =
                    with_current_composer(|composer| composer.emit_node(TrackedParent::default));
                if let Some(slots_a) = host_a.borrow().clone() {
                    let shared_movable = Rc::clone(&shared_movable);
                    let probe = Rc::clone(&probe);
                    with_current_composer(|composer| {
                        composer
                            .subcompose_in(&slots_a, Some(root_a), |_| {
                                host_a_body(&shared_movable, &probe);
                            })
                            .expect("subcompose host_a");
                    });
                }
                let content = shared_movable.borrow().clone();
                with_current_composer(|composer| {
                    composer
                        .subcompose_in(&host_b, Some(root_b), |_| {
                            if show_in_b.value()
                                && let Some(content) = content
                            {
                                content.show();
                            }
                        })
                        .expect("subcompose host_b");
                });
            })
            .expect("render composition");
    }
}

#[test]
fn retained_movable_in_another_host_disposes_when_owner_host_drops_outside_pass() {
    let mut rig = TwoHostRig::new(location_key(file!(), line!(), column!()));
    let render = |rig: &mut TwoHostRig| {
        rig.render(|shared_movable, probe| {
            let probe = Rc::clone(probe);
            let cell = remember(|| RuntimeCell::new(Cell::new(0))).with(|c| *c);
            let content = rememberMovableContentOf(move || {
                probe.emit_movable_body(vec![cell], None);
            });
            *shared_movable.borrow_mut() = Some(content);
        });
    };

    render(&mut rig);
    let node = rig.probe.node.get().expect("movable composed in host_b");

    // Detach from host_b so the movable sits retained in host_b while host_a
    // still owns it.
    rig.show_in_b.set_value(false);
    render(&mut rig);
    assert_eq!(rig.probe.unmounts.get(), 0);
    assert!(rig.composition.applier_mut().get_mut(node).is_ok());

    // Drop host_a outside any compose pass: the movable retained in host_b must
    // unmount before host_a's RuntimeCell is released.
    rig.host_a.borrow_mut().take();
    rig.probe.assert_disposed(&mut rig.composition, node);
}

struct RetainedHostFixture {
    composition: Composition<MemoryApplier>,
    sub_slots: Rc<RefCell<Option<Rc<SlotsHost>>>>,
    host_resource_drops: Rc<Cell<usize>>,
    probe: Rc<LifecycleProbe>,
    node: NodeId,
}

fn setup_retained_host_with_watched_resource() -> RetainedHostFixture {
    let mut composition = Composition::new(MemoryApplier::new());
    let show_movable = MutableState::with_runtime(true, composition.runtime_handle());
    let sub_slots = Rc::new(RefCell::new(Some(Rc::new(
        SlotsHost::new(SlotTable::new()),
    ))));
    let host_resource_drops = Rc::new(Cell::new(0));
    let probe = Rc::new(LifecycleProbe::default());
    let root_key = location_key(file!(), line!(), column!());

    let render = |composition: &mut Composition<MemoryApplier>| {
        let sub_slots = Rc::clone(&sub_slots);
        let host_resource_drops = Rc::clone(&host_resource_drops);
        let probe = Rc::clone(&probe);
        composition
            .render(root_key, move || {
                let host_node =
                    with_current_composer(|composer| composer.emit_node(TrackedParent::default));
                if let Some(slots) = sub_slots.borrow().clone() {
                    with_current_composer(|composer| {
                        composer
                            .subcompose_in(&slots, Some(host_node), |_| {
                                let cell = remember(|| RuntimeCell::new(Cell::new(0))).with(|c| *c);
                                let _host_resource =
                                    remember(|| DropCounter(Rc::clone(&host_resource_drops)));
                                if show_movable.value() {
                                    let probe = Rc::clone(&probe);
                                    let watched = Rc::clone(&host_resource_drops);
                                    movable("retained-host-key", move || {
                                        probe.emit_movable_body(vec![cell], Some(watched));
                                    });
                                }
                            })
                            .expect("subcompose slots");
                    });
                }
            })
            .expect("render composition");
    };

    render(&mut composition);
    let node = probe.node.get().expect("keyed movable composed");
    show_movable.set_value(false);
    render(&mut composition);
    assert_eq!(probe.unmounts.get(), 0, "retained movable stays mounted");
    assert_eq!(
        host_resource_drops.get(),
        0,
        "host resource stays alive before disposal"
    );
    RetainedHostFixture {
        composition,
        sub_slots,
        host_resource_drops,
        probe,
        node,
    }
}

fn assert_host_unmounted_before_payloads(fixture: &mut RetainedHostFixture) {
    assert_eq!(
        fixture.probe.unmounts.get(),
        1,
        "retained movable unmounts once"
    );
    assert_eq!(
        fixture.probe.payload_drops.get(),
        1,
        "retained movable payloads drop once"
    );
    assert_eq!(
        fixture.host_resource_drops.get(),
        1,
        "host resource drops once"
    );
    assert!(
        !fixture.probe.unmount_saw_watched_dropped.get(),
        "retained movable node must unmount before the host's own remembered payloads drop"
    );
    assert!(
        fixture
            .composition
            .applier_mut()
            .get_mut(fixture.node)
            .is_err(),
        "disposed movable node leaves the applier"
    );
}

#[test]
fn dropping_slots_host_while_applier_is_borrowed_unmounts_retained_movable_before_host_drops() {
    let mut fixture = setup_retained_host_with_watched_resource();
    {
        let _applier = fixture.composition.applier_mut();
        fixture.sub_slots.borrow_mut().take();
    }
    assert_host_unmounted_before_payloads(&mut fixture);
}

#[test]
fn reset_while_applier_is_borrowed_unmounts_retained_movable_before_host_drops() {
    let mut fixture = setup_retained_host_with_watched_resource();
    let slots = fixture
        .sub_slots
        .borrow_mut()
        .take()
        .expect("sub_slots created");
    {
        let _applier = fixture.composition.applier_mut();
        slots.reset().expect("reset while applier is borrowed");
    }
    assert_host_unmounted_before_payloads(&mut fixture);
}

fn setup_retained_nested_owner() -> (
    Composition<MemoryApplier>,
    Rc<SlotsHost>,
    Rc<LifecycleProbe>,
) {
    let mut composition = Composition::new(MemoryApplier::new());
    let show_a = MutableState::with_runtime(true, composition.runtime_handle());
    let sub_slots = Rc::new(SlotsHost::new(SlotTable::new()));
    let probe = Rc::new(LifecycleProbe::default());
    let root_key = location_key(file!(), line!(), column!());

    let render = |composition: &mut Composition<MemoryApplier>| {
        let sub_slots = Rc::clone(&sub_slots);
        let probe = Rc::clone(&probe);
        composition
            .render(root_key, move || {
                let host =
                    with_current_composer(|composer| composer.emit_node(TrackedParent::default));
                let probe = Rc::clone(&probe);
                with_current_composer(|composer| {
                    composer
                        .subcompose_in(&sub_slots, Some(host), |_| {
                            let a = rememberMovableContentOf(move || {
                                let probe = Rc::clone(&probe);
                                let b = rememberMovableContentOf(move || {
                                    probe.emit_movable_body(Vec::new(), None);
                                });
                                emit_wrapper(|| b.show());
                            });
                            if show_a.value() {
                                emit_wrapper(|| a.show());
                            }
                        })
                        .expect("subcompose_in");
                });
            })
            .expect("render composition");
    };

    render(&mut composition);
    show_a.set_value(false);
    render(&mut composition);
    assert_eq!(
        probe.unmounts.get(),
        0,
        "A and B stay retained while A's owner lives"
    );
    (composition, sub_slots, probe)
}

#[test]
fn reset_outside_pass_with_retained_nested_owner() {
    let (_composition, sub_slots, probe) = setup_retained_nested_owner();
    sub_slots.reset().expect("reset outside a pass");
    assert_eq!(probe.unmounts.get(), 1);
}

#[test]
fn into_table_outside_pass_with_retained_nested_owner() {
    let (_composition, sub_slots, probe) = setup_retained_nested_owner();
    let _table = sub_slots.into_table().expect("into_table outside a pass");
    assert_eq!(probe.unmounts.get(), 1);
}

#[test]
fn node_disposal_flush_keeps_owner_host_runtime_cell_alive_until_target_host_unmounts() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let show_in_target = MutableState::with_runtime(true, runtime.clone());
    let keep_owner = MutableState::with_runtime(true, runtime);
    let subcompose_state = Rc::new(RefCell::new({
        let mut state = SubcomposeState::default();
        state.set_reusable_pool_limits(0, 0);
        state
    }));
    let shared_movable: Rc<RefCell<Option<MovableContent>>> = Rc::new(RefCell::new(None));
    let captured_composer = Rc::new(RefCell::new(None));
    let pending_disposal = Rc::new(RefCell::new(None));
    let probe = Rc::new(LifecycleProbe::default());
    let root_key = location_key(file!(), line!(), column!());

    let render = |composition: &mut Composition<MemoryApplier>| {
        let subcompose_state = Rc::clone(&subcompose_state);
        let shared_movable = Rc::clone(&shared_movable);
        let captured_composer = Rc::clone(&captured_composer);
        let pending_disposal = Rc::clone(&pending_disposal);
        let probe = Rc::clone(&probe);
        composition
            .render(root_key, move || {
                let root = with_current_composer(|composer| {
                    *captured_composer.borrow_mut() = Some(composer.clone());
                    composer.emit_node(TrackedParent::default)
                });
                let mut state = subcompose_state.borrow_mut();
                state.begin_pass();
                if keep_owner.value() {
                    let owner_slots = state.get_or_create_slots(SlotId::new(1));
                    let shared_movable = Rc::clone(&shared_movable);
                    let probe = Rc::clone(&probe);
                    let owner_node = with_current_composer(|composer| {
                        composer
                            .subcompose_in(&owner_slots, Some(root), |composer| {
                                let cell = remember(|| RuntimeCell::new(Cell::new(0))).with(|c| *c);
                                let content = rememberMovableContentOf(move || {
                                    probe.emit_movable_body(vec![cell], None);
                                });
                                *shared_movable.borrow_mut() = Some(content);
                                composer.emit_node(TrackedParent::default)
                            })
                            .expect("subcompose owner slot")
                    });
                    state.register_active(SlotId::new(1), &[owner_node], &[]);
                }
                let target_slots = state.get_or_create_slots(SlotId::new(2));
                let content = shared_movable.borrow().clone();
                with_current_composer(|composer| {
                    composer
                        .subcompose_in(&target_slots, Some(root), |_| {
                            if show_in_target.value()
                                && let Some(content) = content
                            {
                                content.show();
                            }
                        })
                        .expect("subcompose target slot");
                });
                state.register_active(SlotId::new(2), &[], &[]);
                let disposal = state.finish_pass();
                if !keep_owner.value() {
                    *pending_disposal.borrow_mut() = Some(disposal);
                }
            })
            .expect("render composition");
    };

    render(&mut composition);
    let node = probe.node.get().expect("movable node in target slot");

    // Retain the movable inside the target slot while the owner slot still lives.
    show_in_target.set_value(false);
    render(&mut composition);
    assert_eq!(probe.unmounts.get(), 0);

    // Evict the owner slot into SubcomposeDisposal without running it yet,
    // then dispose it under a borrowed applier so owner_slots drops inside
    // NodeDisposalQueue::flush (outside any compose pass).
    keep_owner.set_value(false);
    render(&mut composition);
    let disposal = pending_disposal
        .borrow_mut()
        .take()
        .expect("owner slot disposal queued");
    let composer = captured_composer
        .borrow_mut()
        .take()
        .expect("composer captured");
    composer
        .dispose_subcomposed_nodes(disposal)
        .expect("dispose owner slot through NodeDisposalQueue::flush");
    probe.assert_disposed(&mut composition, node);
}

#[test]
fn nested_owner_drops_after_composition_drop_without_panic() {
    let mut rig = TwoHostRig::new(location_key(file!(), line!(), column!()));
    let render = |rig: &mut TwoHostRig| {
        rig.render(|shared_movable, probe| {
            let probe = Rc::clone(probe);
            let content = rememberMovableContentOf(move || {
                let probe = Rc::clone(&probe);
                let inner = rememberMovableContentOf(move || {
                    probe.emit_movable_body(Vec::new(), None);
                });
                emit_wrapper(|| inner.show());
            });
            *shared_movable.borrow_mut() = Some(content);
        });
    };

    render(&mut rig);
    rig.show_in_b.set_value(false);
    render(&mut rig);
    let TwoHostRig {
        composition,
        host_a,
        host_b: _host_b,
        probe,
        ..
    } = rig;
    drop(composition);
    host_a.borrow_mut().take();
    assert_eq!(probe.payload_drops.get(), 1, "inner payload drops once");
}

#[test]
fn fresh_remember_movable_shown_in_two_hosts_in_same_pass_keeps_single_instance() {
    let mut rig = TwoHostRig::new(location_key(file!(), line!(), column!()));
    let show_in_a = MutableState::with_runtime(true, rig.composition.runtime_handle());
    let keep_owner = MutableState::with_runtime(true, rig.composition.runtime_handle());
    let body_creations = Rc::new(Cell::new(0usize));
    let render = |rig: &mut TwoHostRig| {
        let body_creations = Rc::clone(&body_creations);
        rig.render(move |shared_movable, probe| {
            if keep_owner.value() {
                let body_creations = Rc::clone(&body_creations);
                let probe = Rc::clone(probe);
                let content = rememberMovableContentOf(move || {
                    let _initialized = remember(|| {
                        body_creations.set(body_creations.get() + 1);
                    });
                    probe.emit_movable_body(Vec::new(), None);
                });
                *shared_movable.borrow_mut() = Some(content.clone());
                if show_in_a.value() {
                    content.show();
                }
            }
        });
    };

    // Pass 1: fresh movable is shown in host_a AND requested in host_b in the
    // very same pass. Only one instance may compose; host_b stays a placeholder.
    render(&mut rig);
    assert_eq!(body_creations.get(), 1);
    assert_eq!(rig.probe.unmounts.get(), 0);
    let node = rig.probe.node.get().expect("movable composed in host_a");

    // Pass 2: host_a releases the movable; host_b takes over the same instance.
    show_in_a.set_value(false);
    render(&mut rig);
    let _recomposed = rig
        .composition
        .process_invalid_scopes()
        .expect("recompose pending movable");
    assert_eq!(body_creations.get(), 1);
    assert_eq!(rig.probe.unmounts.get(), 0);

    // Pass 3: owner leaves host_a while host_b still displays the movable, then
    // host_b hides it. It must dispose instead of staying pinned.
    keep_owner.set_value(false);
    render(&mut rig);
    assert_eq!(rig.probe.unmounts.get(), 0);

    rig.show_in_b.set_value(false);
    render(&mut rig);
    rig.probe.assert_disposed(&mut rig.composition, node);
}

#[test]
fn sole_owned_movable_disposal_still_splits_nested_unowned_movable() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let show_branch = MutableState::with_runtime(true, runtime.clone());
    let show_second = MutableState::with_runtime(false, runtime);
    let inner_probe = Rc::new(LifecycleProbe::default());
    let outer_drops = Rc::new(Cell::new(0usize));
    let root_key = location_key(file!(), line!(), column!());

    let render = |composition: &mut Composition<MemoryApplier>| {
        let inner_probe = Rc::clone(&inner_probe);
        let outer_drops = Rc::clone(&outer_drops);
        composition
            .render(root_key, move || {
                let outer_owned = rememberMovableContentOf({
                    let inner_probe = Rc::clone(&inner_probe);
                    move || inner_probe.emit_movable_body(Vec::new(), None)
                });
                if show_branch.value() {
                    let local = rememberMovableContentOf({
                        let outer_owned = outer_owned.clone();
                        let outer_drops = Rc::clone(&outer_drops);
                        move || {
                            let _drop = remember(|| DropCounter(Rc::clone(&outer_drops)));
                            emit_wrapper(|| outer_owned.show());
                        }
                    });
                    emit_wrapper(|| local.show());
                }
                if show_second.value() {
                    emit_wrapper(|| outer_owned.show());
                }
            })
            .expect("render composition");
    };

    render(&mut composition);
    let inner_node = inner_probe.node.get().expect("inner movable composed");
    assert_eq!(outer_drops.get(), 0);
    assert_eq!(inner_probe.unmounts.get(), 0);

    // Leaving the branch disposes `local` inline (sole-owned, weak_count == 0)
    // while splitting and retaining `outer_owned` whose owner still lives.
    show_branch.set_value(false);
    render(&mut composition);
    assert_eq!(outer_drops.get(), 1, "local sole-owned movable disposed");
    assert_eq!(
        inner_probe.unmounts.get(),
        0,
        "unowned nested movable stays retained"
    );
    assert!(inner_probe.last_launched_active());

    show_second.set_value(true);
    render(&mut composition);
    assert_eq!(
        inner_probe.node.get(),
        Some(inner_node),
        "retained nested movable moves to the second parent with its original node"
    );
    assert_eq!(inner_probe.unmounts.get(), 0);
}

#[test]
fn leaving_owner_with_live_handle_allows_same_pass_sibling_takeover() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let show_a = MutableState::with_runtime(true, runtime.clone());
    let show_b = MutableState::with_runtime(false, runtime);
    let shared_movable: Rc<RefCell<Option<MovableContent>>> = Rc::new(RefCell::new(None));
    let probe = Rc::new(LifecycleProbe::default());
    let root_key = location_key(file!(), line!(), column!());

    let render = |composition: &mut Composition<MemoryApplier>| {
        let shared_movable = Rc::clone(&shared_movable);
        let probe = Rc::clone(&probe);
        composition
            .render(root_key, move || {
                if show_a.value() {
                    let content = rememberMovableContentOf({
                        let probe = Rc::clone(&probe);
                        move || probe.emit_movable_body(Vec::new(), None)
                    });
                    *shared_movable.borrow_mut() = Some(content.clone());
                    emit_wrapper(|| content.show());
                }
                if show_b.value()
                    && let Some(content) = shared_movable.borrow().clone()
                {
                    emit_wrapper(|| content.show());
                }
            })
            .expect("render composition");
    };

    render(&mut composition);
    let node = probe.node.get().expect("movable composed in branch A");

    // Branch A leaves while a live handle exists and branch B takes it in the
    // very same pass.
    show_a.set_value(false);
    show_b.set_value(true);
    render(&mut composition);
    assert_eq!(
        probe.node.get(),
        Some(node),
        "sibling takes over the live movable in the same pass"
    );
    assert_eq!(probe.unmounts.get(), 0);

    // Once branch B stops showing it, it disposes because its owner already left.
    show_b.set_value(false);
    render(&mut composition);
    probe.assert_disposed(&mut composition, node);
}
