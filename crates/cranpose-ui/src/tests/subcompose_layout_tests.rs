use super::*;
use std::cell::RefCell;
use std::rc::Rc;

use cranpose_core::{
    self, Applier, ConcreteApplierHost, MutableState, SlotTable, SlotsHost, SnapshotStateObserver,
};
use smallvec::SmallVec;

#[derive(Default)]
struct DummyNode;

impl cranpose_core::Node for DummyNode {}

fn runtime_handle() -> (
    cranpose_core::RuntimeHandle,
    cranpose_core::Composition<cranpose_core::MemoryApplier>,
) {
    let composition = cranpose_core::Composition::new(cranpose_core::MemoryApplier::new());
    let handle = composition.runtime_handle();
    (handle, composition)
}

fn setup_composer(
    slots: &mut SlotTable,
    applier: &mut cranpose_core::MemoryApplier,
    handle: cranpose_core::RuntimeHandle,
    root: Option<cranpose_core::NodeId>,
) -> (
    cranpose_core::Composer,
    Rc<SlotsHost>,
    Rc<ConcreteApplierHost<cranpose_core::MemoryApplier>>,
) {
    let slots_host = Rc::new(SlotsHost::new(std::mem::take(slots)));
    let applier_host = Rc::new(ConcreteApplierHost::new(std::mem::replace(
        applier,
        cranpose_core::MemoryApplier::new(),
    )));
    let observer = SnapshotStateObserver::new(|callback| callback());
    let composer = cranpose_core::Composer::new(
        Rc::clone(&slots_host),
        applier_host.clone(),
        handle,
        observer,
        root,
    );
    (composer, slots_host, applier_host)
}

fn teardown_composer(
    slots: &mut SlotTable,
    applier: &mut cranpose_core::MemoryApplier,
    slots_host: Rc<SlotsHost>,
    applier_host: Rc<ConcreteApplierHost<cranpose_core::MemoryApplier>>,
) {
    *slots = slots_host
        .into_table()
        .expect("restore subcompose test slots");
    *applier = Rc::try_unwrap(applier_host)
        .unwrap_or_else(|_| panic!("applier host still has outstanding references"))
        .into_inner();
}

fn measure_once(
    slots: &mut SlotTable,
    applier: &mut cranpose_core::MemoryApplier,
    handle: &cranpose_core::RuntimeHandle,
    node_id: cranpose_core::NodeId,
    constraints: Constraints,
) -> MeasureResult {
    let (composer, slots_host, applier_host) =
        setup_composer(slots, applier, handle.clone(), Some(node_id));
    composer.enter_phase(Phase::Measure);
    let node_handle = {
        let mut applier_ref = applier_host.borrow_typed();
        let node = applier_ref.get_mut(node_id).expect("node available");
        let typed = node
            .as_any_mut()
            .downcast_mut::<SubcomposeLayoutNode>()
            .expect("subcompose layout node");
        typed.handle()
    };
    let measurer =
        Box::new(|_child_id: cranpose_core::NodeId, _constraints: Constraints| Size::default());
    let cached_measure_registrar =
        Box::new(|_child_id: cranpose_core::NodeId, _constraints: Constraints| None);
    let error = Rc::new(RefCell::new(None));
    let result = node_handle
        .measure(
            &composer,
            node_id,
            constraints,
            measurer,
            cached_measure_registrar,
            &error,
        )
        .expect("measure result");
    assert!(
        error.borrow().is_none(),
        "unexpected subcompose measure error"
    );
    drop(composer);
    teardown_composer(slots, applier, slots_host, applier_host);
    result
}

/// `SubcomposeMeasureScopeImpl::density()`/`font_scale()` used to read
/// `composer_context::with_composer`, which panics with "no active composer"
/// unless the thread-local composer stack was entered by `Composer::install`.
/// The single production construction site happens to sit inside that install
/// call, which is why the bug never surfaced there -- but nothing about the
/// type made a second, non-installed construction site safe.
///
/// This constructs the scope directly, deliberately outside of
/// `Composer::install`/`subcompose_slot_with_context`, and reads `density()`
/// and `font_scale()` from it. Pre-fix this panics; post-fix it is a plain
/// field read of the `Density` the scope was built with, so it cannot depend
/// on -- or require -- an ambient composer.
#[test]
fn density_and_font_scale_do_not_require_an_active_composer() {
    assert!(
        cranpose_core::composer_context::current_composer().is_none(),
        "test must start with no active composer"
    );

    let _app_context = crate::render_state::app_context_test_scope();
    let (handle, _composition) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = cranpose_core::MemoryApplier::new();

    let policy: Rc<MeasurePolicy> =
        Rc::new(|scope, _constraints| scope.layout(0.0, 0.0, Vec::new()));
    let node = SubcomposeLayoutNode::new(crate::modifier::Modifier::empty(), Rc::clone(&policy));
    let parent_handle = node.handle();
    let root_id = applier.create(Box::new(node));

    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle, Some(root_id));

    let mut state = SubcomposeState::default();
    let error = RefCell::new(None);
    let density = crate::density::Density::new(2.5, 1.75);

    let scope = SubcomposeMeasureScopeImpl::new(SubcomposeMeasureScopeInit {
        composer,
        density,
        state: &mut state,
        constraints: Constraints::tight(0.0, 0.0),
        measurer: Box::new(|_child_id, _constraints| Size::default()),
        cached_measure_batch_registrar: Box::new(|_node_ids, _constraints, out| out.clear()),
        retained_measure_lookup: Box::new(|_| None),
        retained_measure_registrar: Box::new(|_| {}),
        error: &error,
        parent_handle,
        root_id,
        placement_scratch: Vec::new(),
    });

    assert!(
        cranpose_core::composer_context::current_composer().is_none(),
        "constructing the scope must not enter the ambient composer"
    );
    assert_eq!(cranpose_ui_layout::MeasureScope::density(&scope), 2.5);
    assert_eq!(cranpose_ui_layout::MeasureScope::font_scale(&scope), 1.75);

    drop(scope);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

#[test]
fn cached_measurement_node_ids_are_registered_in_one_batch() {
    let _app_context = crate::render_state::app_context_test_scope();
    let (handle, _composition) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = cranpose_core::MemoryApplier::new();

    let registered_count = Rc::new(RefCell::new(0usize));
    let registered_count_for_policy = Rc::clone(&registered_count);
    let policy: Rc<MeasurePolicy> = Rc::new(move |scope, _constraints| {
        let count = scope.ensure_cached_measurement_node_ids(
            vec![101usize, 102usize, 103usize],
            Constraints::tight(24.0, 12.0),
        );
        *registered_count_for_policy.borrow_mut() = count;
        scope.layout(0.0, 0.0, Vec::new())
    });

    let node_id = applier.create(Box::new(SubcomposeLayoutNode::new(
        crate::modifier::Modifier::empty(),
        Rc::clone(&policy),
    )));
    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle.clone(), Some(node_id));
    composer.enter_phase(Phase::Measure);
    let node_handle = {
        let mut applier_ref = applier_host.borrow_typed();
        let node = applier_ref.get_mut(node_id).expect("node available");
        node.as_any_mut()
            .downcast_mut::<SubcomposeLayoutNode>()
            .expect("subcompose layout node")
            .handle()
    };

    let batch_calls = Rc::new(RefCell::new(0usize));
    let batch_nodes = Rc::new(RefCell::new(Vec::<Vec<cranpose_core::NodeId>>::new()));
    let batch_calls_for_registrar = Rc::clone(&batch_calls);
    let batch_nodes_for_registrar = Rc::clone(&batch_nodes);
    let measure_calls = Rc::new(RefCell::new(0usize));
    let measure_calls_for_measurer = Rc::clone(&measure_calls);
    let error = Rc::new(RefCell::new(None));

    node_handle
        .measure_with_cached_batch(
            &composer,
            node_id,
            Constraints::tight(0.0, 0.0),
            CachedBatchMeasureInputs {
                measurer: Box::new(move |_child_id, _constraints| {
                    *measure_calls_for_measurer.borrow_mut() += 1;
                    Size::default()
                }),
                cached_measure_batch_registrar: Box::new(move |node_ids, _constraints, out| {
                    *batch_calls_for_registrar.borrow_mut() += 1;
                    batch_nodes_for_registrar
                        .borrow_mut()
                        .push(node_ids.to_vec());
                    out.clear();
                    out.extend(node_ids.iter().map(|_| {
                        Some(Size {
                            width: 24.0,
                            height: 12.0,
                        })
                    }));
                }),
                retained_measure_lookup: Box::new(|_| None),
                retained_measure_registrar: Box::new(|_| {}),
                error: &error,
            },
        )
        .expect("measure result");

    assert!(error.borrow().is_none());
    assert_eq!(*registered_count.borrow(), 3);
    assert_eq!(*measure_calls.borrow(), 0);
    assert_eq!(*batch_calls.borrow(), 1);
    assert_eq!(batch_nodes.borrow().as_slice(), &[vec![101, 102, 103]]);

    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

#[test]
fn retained_measurements_skip_cached_batch_registration() {
    let _app_context = crate::render_state::app_context_test_scope();
    let (handle, _composition) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = cranpose_core::MemoryApplier::new();

    let retained = Rc::new(crate::layout::MeasuredNode::leaf(
        101,
        Size {
            width: 24.0,
            height: 12.0,
        },
    ));
    let retained_for_policy = Rc::clone(&retained);
    let registered_count = Rc::new(RefCell::new(0usize));
    let registered_count_for_policy = Rc::clone(&registered_count);
    let policy: Rc<MeasurePolicy> = Rc::new(move |scope, _constraints| {
        scope.register_retained_measurements(std::slice::from_ref(&retained_for_policy));
        let count = scope.ensure_cached_measurement_node_ids(
            vec![101usize, 102usize],
            Constraints::tight(24.0, 12.0),
        );
        *registered_count_for_policy.borrow_mut() = count;
        scope.layout(0.0, 0.0, Vec::new())
    });

    let node_id = applier.create(Box::new(SubcomposeLayoutNode::new(
        crate::modifier::Modifier::empty(),
        Rc::clone(&policy),
    )));
    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle.clone(), Some(node_id));
    composer.enter_phase(Phase::Measure);
    let node_handle = {
        let mut applier_ref = applier_host.borrow_typed();
        let node = applier_ref.get_mut(node_id).expect("node available");
        node.as_any_mut()
            .downcast_mut::<SubcomposeLayoutNode>()
            .expect("subcompose layout node")
            .handle()
    };

    let batch_nodes = Rc::new(RefCell::new(Vec::<Vec<cranpose_core::NodeId>>::new()));
    let batch_nodes_for_registrar = Rc::clone(&batch_nodes);
    let retained_registrations = Rc::new(RefCell::new(Vec::<cranpose_core::NodeId>::new()));
    let retained_registrations_for_registrar = Rc::clone(&retained_registrations);
    let error = Rc::new(RefCell::new(None));

    node_handle
        .measure_with_cached_batch(
            &composer,
            node_id,
            Constraints::tight(0.0, 0.0),
            CachedBatchMeasureInputs {
                measurer: Box::new(|_, _| Size::default()),
                cached_measure_batch_registrar: Box::new(move |node_ids, _constraints, out| {
                    batch_nodes_for_registrar
                        .borrow_mut()
                        .push(node_ids.to_vec());
                    out.clear();
                    out.extend(node_ids.iter().map(|_| {
                        Some(Size {
                            width: 24.0,
                            height: 12.0,
                        })
                    }));
                }),
                retained_measure_lookup: Box::new(|_| None),
                retained_measure_registrar: Box::new(move |measurements| {
                    retained_registrations_for_registrar
                        .borrow_mut()
                        .extend(measurements.iter().map(|measured| measured.node_id()));
                }),
                error: &error,
            },
        )
        .expect("measure result");

    assert!(error.borrow().is_none());
    assert_eq!(*registered_count.borrow(), 1);
    assert_eq!(batch_nodes.borrow().as_slice(), &[vec![102]]);
    assert_eq!(retained_registrations.borrow().as_slice(), &[101]);

    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

#[test]
fn measure_subcomposes_content() {
    let _app_context = crate::render_state::app_context_test_scope();
    let (handle, _composition) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = cranpose_core::MemoryApplier::new();
    let recorded = Rc::new(RefCell::new(Vec::new()));
    let recorded_capture = Rc::clone(&recorded);
    let policy: Rc<MeasurePolicy> = Rc::new(move |scope, constraints| {
        assert_eq!(constraints, Constraints::tight(0.0, 0.0));
        let measurables = scope.subcompose(SlotId::new(1), || {
            cranpose_core::with_current_composer(|composer| {
                composer.emit_node(|| DummyNode);
            });
        });
        for measurable in measurables {
            recorded_capture.borrow_mut().push(measurable.node_id());
        }
        scope.layout(0.0, 0.0, Vec::new())
    });
    let node_id = applier.create(Box::new(SubcomposeLayoutNode::new(
        crate::modifier::Modifier::empty(),
        Rc::clone(&policy),
    )));
    let result = measure_once(
        &mut slots,
        &mut applier,
        &handle,
        node_id,
        Constraints::tight(0.0, 0.0),
    );
    assert_eq!(result.size, Size::default());
    {
        let node = applier.get_mut(node_id).expect("node available");
        let typed = node
            .as_any_mut()
            .downcast_mut::<SubcomposeLayoutNode>()
            .expect("subcompose layout node");
        assert!(typed.state().reusable().is_empty());
    }
    assert_eq!(recorded.borrow().len(), 1);
}

#[test]
fn subcompose_reuses_nodes_across_measures() {
    let _app_context = crate::render_state::app_context_test_scope();
    let (handle, _composition) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = cranpose_core::MemoryApplier::new();
    let recorded = Rc::new(RefCell::new(Vec::new()));
    let recorded_capture = Rc::clone(&recorded);
    let policy: Rc<MeasurePolicy> = Rc::new(move |scope, _constraints| {
        let measurables = scope.subcompose(SlotId::new(99), || {
            cranpose_core::with_current_composer(|composer| {
                composer.emit_node(|| DummyNode);
            });
        });
        for measurable in measurables {
            recorded_capture.borrow_mut().push(measurable.node_id());
        }
        scope.layout(0.0, 0.0, Vec::new())
    });
    let node_id = applier.create(Box::new(SubcomposeLayoutNode::new(
        crate::modifier::Modifier::empty(),
        Rc::clone(&policy),
    )));
    let _ = measure_once(
        &mut slots,
        &mut applier,
        &handle,
        node_id,
        Constraints::loose(100.0, 100.0),
    );
    let _ = measure_once(
        &mut slots,
        &mut applier,
        &handle,
        node_id,
        Constraints::loose(200.0, 200.0),
    );

    let recorded = recorded.borrow();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0], recorded[1]);
    {
        let node = applier.get_mut(node_id).expect("node available");
        let typed = node
            .as_any_mut()
            .downcast_mut::<SubcomposeLayoutNode>()
            .expect("subcompose layout node");
        assert!(typed.state().reusable().is_empty());
    }
}

#[test]
fn handle_reports_modifier_capabilities() {
    let _app_context = crate::render_state::app_context_test_scope();
    let policy: Rc<MeasurePolicy> =
        Rc::new(|scope, _constraints| scope.layout(0.0, 0.0, Vec::new()));
    let mut node = SubcomposeLayoutNode::new(
        crate::modifier::Modifier::empty().background(crate::modifier::Color(0.1, 0.2, 0.3, 1.0)),
        Rc::clone(&policy),
    );

    let handle = node.handle();
    assert!(handle.has_draw_modifier_nodes());
    assert!(!handle.has_layout_modifier_nodes());

    node.set_modifier(crate::modifier::Modifier::empty().padding(4.0));
    let handle = node.handle();
    assert!(handle.has_layout_modifier_nodes());
    assert!(!handle.has_draw_modifier_nodes());
}

#[test]
fn inactive_slots_move_to_reusable_pool() {
    let _app_context = crate::render_state::app_context_test_scope();
    let (handle, _composition) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = cranpose_core::MemoryApplier::new();
    let toggle = MutableState::with_runtime(true, handle.clone());
    let toggle_capture = toggle;
    let policy: Rc<MeasurePolicy> = Rc::new(move |scope, _constraints| {
        if toggle_capture.value() {
            scope.subcompose(SlotId::new(1), || {
                cranpose_core::with_current_composer(|composer| {
                    composer.emit_node(|| DummyNode);
                });
            });
        }
        scope.layout(0.0, 0.0, Vec::new())
    });
    let node_id = applier.create(Box::new(SubcomposeLayoutNode::new(
        crate::modifier::Modifier::empty(),
        Rc::clone(&policy),
    )));
    let _ = measure_once(
        &mut slots,
        &mut applier,
        &handle,
        node_id,
        Constraints::loose(50.0, 50.0),
    );
    toggle.set(false);

    let _ = measure_once(
        &mut slots,
        &mut applier,
        &handle,
        node_id,
        Constraints::loose(50.0, 50.0),
    );

    {
        let node = applier.get_mut(node_id).expect("node available");
        let typed = node
            .as_any_mut()
            .downcast_mut::<SubcomposeLayoutNode>()
            .expect("subcompose layout node");
        assert!(!typed.state().reusable().is_empty());
    }
}

#[test]
fn active_children_follow_last_rendered_placements() {
    let _app_context = crate::render_state::app_context_test_scope();
    let policy: Rc<MeasurePolicy> =
        Rc::new(|scope, _constraints| scope.layout(0.0, 0.0, Vec::new()));
    let node = SubcomposeLayoutNode::new(crate::modifier::Modifier::empty(), Rc::clone(&policy));

    {
        let mut inner = node.inner.borrow_mut();
        inner.children = vec![11, 22];
        inner.last_placements = vec![33, 44];
    }

    assert_eq!(node.active_children(), vec![33, 44]);
    assert_eq!(cranpose_core::Node::children(&node), vec![33, 44]);

    node.handle().set_active_children(Vec::<NodeId>::new());
    assert!(node.active_children().is_empty());

    let mut owned = SmallVec::<[NodeId; 8]>::new();
    cranpose_core::Node::collect_owned_children_into(&node, &mut owned);
    assert_eq!(owned.as_slice(), &[11, 22]);
}

#[test]
fn subcompose_reuses_measured_children_scratch_map() {
    let _app_context = crate::render_state::app_context_test_scope();
    let policy: Rc<MeasurePolicy> =
        Rc::new(|scope, _constraints| scope.layout(0.0, 0.0, Vec::new()));
    let node = SubcomposeLayoutNode::new(crate::modifier::Modifier::empty(), Rc::clone(&policy));
    let handle = node.handle();

    let first = handle.measured_children_scratch();
    let first_ptr = Rc::as_ptr(&first);
    first.borrow_mut().reserve(8);
    let retained_capacity = first.borrow().capacity();
    drop(first);

    let second = handle.measured_children_scratch();
    assert_eq!(
        Rc::as_ptr(&second),
        first_ptr,
        "SubcomposeLayout should reuse the measured-child scratch map"
    );
    assert!(
        second.borrow().capacity() >= retained_capacity,
        "SubcomposeLayout should retain measured-child scratch capacity"
    );
}

#[test]
fn subcompose_scope_layout_uses_retained_placement_scratch() {
    let _app_context = crate::render_state::app_context_test_scope();
    let (handle, _composition) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = cranpose_core::MemoryApplier::new();
    let policy: Rc<MeasurePolicy> = Rc::new(|scope, _constraints| {
        scope.layout(
            10.0,
            20.0,
            [
                Placement::new(101, 1.0, 2.0, 0),
                Placement::new(102, 3.0, 4.0, 0),
            ],
        )
    });
    let node = SubcomposeLayoutNode::new(crate::modifier::Modifier::empty(), Rc::clone(&policy));
    let node_handle = node.handle();
    node_handle.recycle_placement_scratch(Vec::with_capacity(16));
    let node_id = applier.create(Box::new(node));

    let result = measure_once(
        &mut slots,
        &mut applier,
        &handle,
        node_id,
        Constraints::tight(0.0, 0.0),
    );

    assert_eq!(result.size, Size::new(10.0, 20.0));
    assert_eq!(result.placements.len(), 2);
    assert!(
        result.placements.capacity() >= 16,
        "SubcomposeLayout should return placements backed by retained scratch"
    );
}

#[test]
fn subcompose_modifier_slices_cache_reuses_unique_snapshot_allocation() {
    let _app_context = crate::render_state::app_context_test_scope();
    let policy: Rc<MeasurePolicy> =
        Rc::new(|scope, _constraints| scope.layout(0.0, 0.0, Vec::new()));
    let mut node =
        SubcomposeLayoutNode::new(crate::modifier::Modifier::empty(), Rc::clone(&policy));
    let snapshot = node.modifier_slices_snapshot();
    let snapshot_ptr = Rc::as_ptr(&snapshot);
    drop(snapshot);

    node.set_modifier(crate::modifier::Modifier::empty().padding(4.0));

    let updated = node.modifier_slices_snapshot();
    assert_eq!(Rc::as_ptr(&updated), snapshot_ptr);
}
