use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{
    Applier, Composition, MemoryApplier, MutableState, Node, NodeId, SlotId, location_key, movable,
    with_current_composer,
};
use cranpose_ui::{
    AppContext, LayoutEngine, MeasureResult, Modifier, Placement, Size, SubcomposeMeasureScope,
};

const INITIAL_SLOTS: usize = 24;
const PROBE_RESOURCE_BYTES: usize = 4096;
const RETAINED_MOVABLES: usize = 2;

#[derive(Default)]
struct RetainedMovableLifecycle {
    remembered_drops: Rc<Cell<usize>>,
    unmounts_saw_state_alive: Cell<usize>,
    unmounts_saw_state_dropped: Cell<usize>,
}

#[derive(Default)]
struct OrdinarySlotLifecycle {
    remembered_drops: Rc<Cell<usize>>,
    unmount_saw_state_alive: Cell<bool>,
    unmount_saw_state_dropped: Cell<bool>,
}

fn nested_layout_key() -> u64 {
    location_key(file!(), line!(), column!())
}

struct DropProbe {
    _resource: Resource,
    parent: Option<NodeId>,
    retained_lifecycle: Option<Rc<RetainedMovableLifecycle>>,
    ordinary_slot_lifecycle: Option<Rc<OrdinarySlotLifecycle>>,
}

impl Node for DropProbe {
    fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    fn on_attached_to_parent(&mut self, parent: NodeId) {
        self.parent = Some(parent);
    }

    fn on_removed_from_parent(&mut self) {
        self.parent = None;
    }

    fn unmount(&mut self) {
        if let Some(lifecycle) = &self.retained_lifecycle {
            if lifecycle.remembered_drops.get() == 0 {
                lifecycle
                    .unmounts_saw_state_alive
                    .set(lifecycle.unmounts_saw_state_alive.get() + 1);
            } else {
                lifecycle
                    .unmounts_saw_state_dropped
                    .set(lifecycle.unmounts_saw_state_dropped.get() + 1);
            }
        }
        if let Some(lifecycle) = &self.ordinary_slot_lifecycle {
            if lifecycle.remembered_drops.get() == 0 {
                lifecycle.unmount_saw_state_alive.set(true);
            } else {
                lifecycle.unmount_saw_state_dropped.set(true);
            }
        }
    }
}

struct Resource {
    dropped: Rc<Cell<usize>>,
    _bytes: Box<[u8]>,
}

impl Resource {
    fn new(dropped: Rc<Cell<usize>>) -> Self {
        Self {
            dropped,
            _bytes: vec![0; PROBE_RESOURCE_BYTES].into_boxed_slice(),
        }
    }
}

impl Drop for Resource {
    fn drop(&mut self) {
        self.dropped.set(self.dropped.get() + 1);
    }
}

fn render_layout(
    context: &Rc<AppContext>,
    composition: &mut Composition<MemoryApplier>,
    slot_count: usize,
    content_type: Option<u64>,
    dropped: Rc<Cell<usize>>,
    forgotten: Rc<Cell<usize>>,
    slot_lifecycles: Rc<RefCell<Vec<Rc<OrdinarySlotLifecycle>>>>,
) {
    context
        .enter(|| {
            composition.render(location_key(file!(), line!(), column!()), move || {
                let dropped = Rc::clone(&dropped);
                let forgotten = Rc::clone(&forgotten);
                let slot_lifecycles = Rc::clone(&slot_lifecycles);
                cranpose_ui::SubcomposeLayout(Modifier::empty(), move |scope, constraints| {
                    for slot in 0..slot_count {
                        let dropped = Rc::clone(&dropped);
                        let forgotten = Rc::clone(&forgotten);
                        let slot_lifecycle = Rc::new(OrdinarySlotLifecycle::default());
                        slot_lifecycles
                            .borrow_mut()
                            .push(Rc::clone(&slot_lifecycle));
                        let slot_lifecycle = Rc::clone(&slot_lifecycle);
                        let slot_id = SlotId::new(slot as u64);
                        scope.update_content_type(slot_id, content_type);
                        scope.subcompose(slot_id, (), move || {
                            cranpose_core::remember(|| Resource::new(Rc::clone(&forgotten)));
                            cranpose_core::remember(|| {
                                Resource::new(Rc::clone(&slot_lifecycle.remembered_drops))
                            });
                            with_current_composer(|composer| {
                                composer.emit_node(|| DropProbe {
                                    _resource: Resource::new(Rc::clone(&dropped)),
                                    parent: None,
                                    retained_lifecycle: None,
                                    ordinary_slot_lifecycle: Some(Rc::clone(&slot_lifecycle)),
                                });
                            });
                        });
                    }
                    let (width, height) = constraints.constrain(0.0, 0.0);
                    MeasureResult::new(Size::new(width, height), Vec::new())
                });
            })
        })
        .expect("subcompose composition");

    measure_layout(context, composition);
}

fn measure_layout(context: &Rc<AppContext>, composition: &mut Composition<MemoryApplier>) {
    context.enter(|| {
        let root = composition.root().expect("subcompose root");
        let runtime = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(runtime);
        applier
            .compute_layout(root, Size::new(320.0, 480.0))
            .expect("subcompose layout");
        applier.clear_runtime_handle();
    });
}

fn compose_retained_pair(
    probe_node_ids: &Rc<RefCell<Vec<NodeId>>>,
    dropped: &Rc<Cell<usize>>,
    lifecycle: &Rc<RetainedMovableLifecycle>,
) {
    for index in 0..RETAINED_MOVABLES {
        let probe_node_ids = Rc::clone(probe_node_ids);
        let dropped = Rc::clone(dropped);
        let lifecycle = Rc::clone(lifecycle);
        movable(("nested-retained-probe", index), move || {
            cranpose_core::remember(|| Resource::new(Rc::clone(&lifecycle.remembered_drops)));
            with_current_composer(|composer| {
                let id = composer.emit_node(|| DropProbe {
                    _resource: Resource::new(Rc::clone(&dropped)),
                    parent: None,
                    retained_lifecycle: Some(Rc::clone(&lifecycle)),
                    ordinary_slot_lifecycle: None,
                });
                probe_node_ids.borrow_mut().push(id);
            });
        });
    }
}

fn compose_nested_movable_layout(
    context: &Rc<AppContext>,
    composition: &mut Composition<MemoryApplier>,
    slot_count: usize,
    show_movable: MutableState<bool>,
    nested_node_id: Rc<Cell<Option<NodeId>>>,
    probe_node_ids: Rc<RefCell<Vec<NodeId>>>,
    dropped: Rc<Cell<usize>>,
    retained_lifecycle: Rc<RetainedMovableLifecycle>,
) {
    context
        .enter(|| {
            composition.render(nested_layout_key(), move || {
                let nested_node_id = Rc::clone(&nested_node_id);
                let probe_node_ids = Rc::clone(&probe_node_ids);
                let dropped = Rc::clone(&dropped);
                let retained_lifecycle = Rc::clone(&retained_lifecycle);
                cranpose_ui::SubcomposeLayout(Modifier::empty(), move |scope, constraints| {
                    let mut placements = Vec::new();
                    for slot in 0..slot_count {
                        let nested_node_id = Rc::clone(&nested_node_id);
                        let probe_node_ids = Rc::clone(&probe_node_ids);
                        let dropped = Rc::clone(&dropped);
                        let retained_lifecycle = Rc::clone(&retained_lifecycle);
                        let children = scope.subcompose(SlotId::new(slot as u64), (), move || {
                            if slot == slot_count - 1 {
                                let show = show_movable.value();
                                let probe_node_ids = Rc::clone(&probe_node_ids);
                                let dropped = Rc::clone(&dropped);
                                let retained_lifecycle = Rc::clone(&retained_lifecycle);
                                let nested = cranpose_ui::SubcomposeLayout(
                                    Modifier::empty(),
                                    move |inner_scope, inner_constraints| {
                                        let probe_node_ids = Rc::clone(&probe_node_ids);
                                        let dropped = Rc::clone(&dropped);
                                        let retained_lifecycle = Rc::clone(&retained_lifecycle);
                                        inner_scope.subcompose(SlotId::new(0), show, move || {
                                            if show {
                                                compose_retained_pair(
                                                    &probe_node_ids,
                                                    &dropped,
                                                    &retained_lifecycle,
                                                );
                                            }
                                        });
                                        let (width, height) = inner_constraints.constrain(0.0, 0.0);
                                        MeasureResult::new(Size::new(width, height), Vec::new())
                                    },
                                );
                                nested_node_id.set(Some(nested));
                            }
                        });
                        for child in children {
                            let placeable = scope.measure(child, constraints);
                            placements.push(Placement::new(placeable.node_id(), 0.0, 0.0, 0));
                        }
                    }
                    let (width, height) = constraints.constrain(320.0, 480.0);
                    MeasureResult::new(Size::new(width, height), placements)
                });
            })
        })
        .expect("nested subcompose composition");

    measure_layout(context, composition);
}

#[test]
fn evicting_subcompose_slots_releases_their_node_owned_resources() {
    for content_type in [None, Some(9)] {
        let context = AppContext::new();
        let mut composition = Composition::new(MemoryApplier::new());
        let dropped = Rc::new(Cell::new(0));
        let forgotten = Rc::new(Cell::new(0));
        let slot_lifecycles = Rc::new(RefCell::new(Vec::new()));

        render_layout(
            &context,
            &mut composition,
            INITIAL_SLOTS,
            content_type,
            Rc::clone(&dropped),
            Rc::clone(&forgotten),
            Rc::clone(&slot_lifecycles),
        );
        assert_eq!(dropped.get(), 0, "active slot nodes must remain alive");
        assert_eq!(forgotten.get(), 0, "active slot state must remain alive");

        render_layout(
            &context,
            &mut composition,
            0,
            content_type,
            Rc::clone(&dropped),
            Rc::clone(&forgotten),
            Rc::clone(&slot_lifecycles),
        );

        assert!(
            dropped.get() > 0,
            "slot roots evicted from the bounded reuse pool must release their child nodes"
        );
        assert!(
            forgotten.get() > 0,
            "evicted slots must release remembered resources"
        );
        let slot_lifecycles = slot_lifecycles.borrow();
        assert_eq!(slot_lifecycles.len(), INITIAL_SLOTS);
        let mut observed_disposed_slots = 0;
        for lifecycle in slot_lifecycles.iter() {
            if lifecycle.remembered_drops.get() == 1 {
                observed_disposed_slots += 1;
                assert!(
                    lifecycle.unmount_saw_state_alive.get(),
                    "each virtual root must unmount while its own remembered state is alive"
                );
                assert!(
                    !lifecycle.unmount_saw_state_dropped.get(),
                    "a virtual root must not unmount after its remembered state was dropped"
                );
            } else {
                assert_eq!(lifecycle.remembered_drops.get(), 0);
            }
        }
        assert_eq!(observed_disposed_slots, dropped.get());
    }
}

fn assert_nested_host_disposal_releases_retained_movable(remove_outer_layout: bool) {
    let context = AppContext::new();
    let mut composition = Composition::new(MemoryApplier::new());
    let show_movable =
        context.enter(|| MutableState::with_runtime(true, composition.runtime_handle()));
    let nested_node_id = Rc::new(Cell::new(None));
    let probe_node_ids = Rc::new(RefCell::new(Vec::new()));
    let dropped = Rc::new(Cell::new(0));
    let retained_lifecycle = Rc::new(RetainedMovableLifecycle::default());

    compose_nested_movable_layout(
        &context,
        &mut composition,
        INITIAL_SLOTS,
        show_movable,
        Rc::clone(&nested_node_id),
        Rc::clone(&probe_node_ids),
        Rc::clone(&dropped),
        Rc::clone(&retained_lifecycle),
    );
    let probe_ids = probe_node_ids.borrow().clone();
    assert_eq!(
        probe_ids.len(),
        RETAINED_MOVABLES,
        "both movable nodes compose"
    );
    assert!(nested_node_id.get().is_some(), "nested subcompose node");

    context.enter(|| show_movable.set(false));
    while context
        .enter(|| composition.process_invalid_scopes())
        .expect("recompose the nested slot")
    {}
    measure_layout(&context, &mut composition);
    assert_eq!(dropped.get(), 0, "the inner slot must retain the movable");
    assert_eq!(
        retained_lifecycle.remembered_drops.get(),
        0,
        "the retained movable must keep its remembered state alive"
    );
    {
        let mut applier = composition.applier_mut();
        for &probe_id in &probe_ids {
            assert_eq!(
                applier.get_mut(probe_id).expect("retained node").parent(),
                None,
                "each retained movable root must be detached from its old slot"
            );
        }
    }

    if remove_outer_layout {
        context
            .enter(|| composition.render(nested_layout_key(), || {}))
            .expect("remove outer subcompose layout");
    } else {
        compose_nested_movable_layout(
            &context,
            &mut composition,
            0,
            show_movable,
            nested_node_id,
            probe_node_ids,
            Rc::clone(&dropped),
            Rc::clone(&retained_lifecycle),
        );
    }

    assert_eq!(
        dropped.get(),
        RETAINED_MOVABLES,
        "disposing the outer subcomposition must release its retained movable"
    );
    assert_eq!(
        retained_lifecycle.remembered_drops.get(),
        RETAINED_MOVABLES,
        "disposing the retained movable must release its remembered state"
    );
    assert_eq!(
        retained_lifecycle.unmounts_saw_state_alive.get(),
        RETAINED_MOVABLES,
        "both retained nodes must unmount while their remembered states are alive"
    );
    assert_eq!(
        retained_lifecycle.unmounts_saw_state_dropped.get(),
        0,
        "neither retained node may unmount after its remembered state was dropped"
    );
}

#[test]
fn evicting_or_removing_outer_layout_disposes_nested_retained_movable_content() {
    assert_nested_host_disposal_releases_retained_movable(false);
    assert_nested_host_disposal_releases_retained_movable(true);
}
