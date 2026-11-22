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

#[derive(Clone)]
struct TakeoverProbe(Rc<LifecycleProbe>);

impl PartialEq for TakeoverProbe {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone)]
struct TakeoverHandle(Rc<RefCell<Option<MovableContent>>>);

impl PartialEq for TakeoverHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[cranpose_macros::composable]
fn TakeoverOwnerBranch(probe: TakeoverProbe, shared: TakeoverHandle) {
    let probe = Rc::clone(&probe.0);
    let content = rememberMovableContentOf(move || probe.emit_movable_body(Vec::new(), None));
    *shared.0.borrow_mut() = Some(content.clone());
    emit_wrapper(|| content.show());
}

#[derive(Clone)]
struct TakeoverCalls(Rc<Cell<usize>>);

impl PartialEq for TakeoverCalls {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[cranpose_macros::composable]
fn TakeoverTakerBranch(shared: TakeoverHandle, calls: TakeoverCalls, decline_on_wake: bool) {
    let count = calls.0.get() + 1;
    calls.0.set(count);
    if (!decline_on_wake || count == 1)
        && let Some(content) = shared.0.borrow().clone()
    {
        emit_wrapper(|| content.show());
    }
}

struct TakeoverRig {
    composition: Composition<MemoryApplier>,
    show_a: MutableState<bool>,
    show_b: MutableState<bool>,
    show_c: MutableState<bool>,
    shared: TakeoverHandle,
    probe: TakeoverProbe,
    taker_calls: TakeoverCalls,
    second_taker_calls: TakeoverCalls,
    decline_on_wake: bool,
    root_key: Key,
}

impl TakeoverRig {
    fn new(root_key: Key, decline_on_wake: bool) -> Self {
        let composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        Self {
            show_a: MutableState::with_runtime(true, runtime.clone()),
            show_b: MutableState::with_runtime(false, runtime.clone()),
            show_c: MutableState::with_runtime(false, runtime),
            shared: TakeoverHandle(Rc::new(RefCell::new(None))),
            probe: TakeoverProbe(Rc::new(LifecycleProbe::default())),
            taker_calls: TakeoverCalls(Rc::new(Cell::new(0))),
            second_taker_calls: TakeoverCalls(Rc::new(Cell::new(0))),
            decline_on_wake,
            composition,
            root_key,
        }
    }

    fn render(&mut self) {
        let show_a = self.show_a;
        let show_b = self.show_b;
        let show_c = self.show_c;
        let shared = self.shared.clone();
        let probe = self.probe.clone();
        let calls = self.taker_calls.clone();
        let second_calls = self.second_taker_calls.clone();
        let decline_on_wake = self.decline_on_wake;
        self.composition
            .render(self.root_key, move || {
                if show_a.value() {
                    TakeoverOwnerBranch(probe.clone(), shared.clone());
                }
                if show_c.value() {
                    TakeoverTakerBranch(shared.clone(), second_calls.clone(), false);
                }
                if show_b.value() {
                    TakeoverTakerBranch(shared.clone(), calls.clone(), decline_on_wake);
                }
            })
            .expect("render composition");
    }
    fn start_and_switch(&mut self) -> NodeId {
        self.render();
        let node = self
            .probe
            .0
            .node
            .get()
            .expect("movable composed in the owner group");
        self.show_a.set_value(false);
        self.show_b.set_value(true);
        self.render();
        node
    }

    fn assert_live_attached(&mut self, node: NodeId, label: &str) {
        assert_eq!(self.probe.0.node.get(), Some(node), "{label}: node kept");
        assert_eq!(self.probe.0.unmounts.get(), 0, "{label}: zero unmounts");
        assert!(
            self.composition
                .applier_mut()
                .get_mut(node)
                .expect("node stays attached")
                .parent()
                .is_some(),
            "{label}: node attached"
        );
    }
}

#[test]
fn owner_group_leaves_while_another_group_takes_live_content() {
    let mut rig = TakeoverRig::new(location_key(file!(), line!(), column!()), false);
    let node = rig.start_and_switch();
    rig.assert_live_attached(node, "takeover into branch B");

    rig.show_b.set_value(false);
    rig.render();
    rig.probe.0.assert_disposed(&mut rig.composition, node);
}

#[test]
fn handed_off_content_survives_second_takeover_after_owner_left() {
    let mut rig = TakeoverRig::new(location_key(file!(), line!(), column!()), false);
    let node = rig.start_and_switch();
    rig.assert_live_attached(node, "initial takeover into branch B");

    // The owner already left in the previous pass so the content is marked
    // disposed-while-attached in branch B. Now branch C composes before
    // branch B leaves in the same pass, waiting on a placeholder until branch B
    // detaches it.
    rig.show_c.set_value(true);
    rig.show_b.set_value(false);
    rig.render();
    rig.assert_live_attached(node, "second takeover into waiting branch C");

    // Once branch C hides it too, the requeued owner drop disposes it.
    rig.show_c.set_value(false);
    rig.render();
    rig.probe.0.assert_disposed(&mut rig.composition, node);
}

#[test]
fn woken_taker_that_declines_content_still_disposes_orphaned_movable() {
    let mut rig = TakeoverRig::new(location_key(file!(), line!(), column!()), true);
    let node = rig.start_and_switch();
    assert_eq!(rig.taker_calls.0.get(), 2, "taker recomposed after waking");
    rig.probe.0.assert_disposed(&mut rig.composition, node);
}

#[test]
fn owner_group_leaves_across_hosts_while_second_host_takes_live_content() {
    let mut rig = TwoHostRig::new(location_key(file!(), line!(), column!()));
    let runtime = rig.composition.runtime_handle();
    let show_in_a = MutableState::with_runtime(true, runtime);
    rig.show_in_b.set_value(false);

    let render = |rig: &mut TwoHostRig| {
        rig.render(move |shared_movable, probe| {
            if show_in_a.value() {
                let probe = Rc::clone(probe);
                let content = rememberMovableContentOf(move || {
                    probe.emit_movable_body(Vec::new(), None);
                });
                *shared_movable.borrow_mut() = Some(content.clone());
                emit_wrapper(|| content.show());
            }
        });
    };

    render(&mut rig);
    let node = rig.probe.node.get().expect("movable composed in host A");

    // Host A leaves and drops the owner while host B takes the live handle
    // later in the same outer pass.
    show_in_a.set_value(false);
    rig.show_in_b.set_value(true);
    render(&mut rig);
    assert_eq!(rig.probe.node.get(), Some(node));
    assert_eq!(rig.probe.unmounts.get(), 0);
    assert!(
        rig.composition
            .applier_mut()
            .get_mut(node)
            .expect("node stays attached in host B")
            .parent()
            .is_some(),
        "taken-over node is attached in host B"
    );

    // Once host B hides it, it disposes because its owner left in host A.
    rig.show_in_b.set_value(false);
    render(&mut rig);
    rig.probe.assert_disposed(&mut rig.composition, node);
}

#[test]
fn sole_owned_movables_past_inline_limit_dispose_inline() {
    const HALF: usize = 10;
    const COUNT: usize = HALF * 2;
    let mut composition = Composition::new(MemoryApplier::new());
    let phase = MutableState::with_runtime(1u8, composition.runtime_handle());
    let drops = Rc::new(Cell::new(0usize));
    let root_key = location_key(file!(), line!(), column!());

    let render = |composition: &mut Composition<MemoryApplier>| {
        let drops = Rc::clone(&drops);
        composition
            .render(root_key, move || {
                let stage = phase.value();
                if stage > 0 {
                    // Stage 1 composes items `HALF..COUNT` first; stage 2 prepends
                    // `0..HALF` so the single leaving group holds > 16 movables in
                    // non-monotonic id order, exercising sorted insertion and search.
                    let range = if stage == 1 { HALF..COUNT } else { 0..COUNT };
                    let container_key = location_key(file!(), line!(), column!());
                    with_current_composer(|composer| {
                        composer.with_group(container_key, |composer| {
                            for index in range {
                                let drops = Rc::clone(&drops);
                                composer.with_key(&index, |_| {
                                    let content = rememberMovableContentOf(move || {
                                        let _drop = remember(|| DropCounter(Rc::clone(&drops)));
                                    });
                                    content.show();
                                });
                            }
                        });
                    });
                }
            })
            .expect("render composition");
    };

    render(&mut composition);
    assert_eq!(drops.get(), 0);

    phase.set_value(2);
    render(&mut composition);
    assert_eq!(drops.get(), 0);

    phase.set_value(0);
    render(&mut composition);
    assert_eq!(drops.get(), COUNT);
}
