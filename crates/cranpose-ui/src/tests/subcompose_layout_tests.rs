use std::{cell::RefCell, rc::Rc};

use cranpose_core::{self, Applier, ConcreteApplierHost, MutableState, Node, SlotTable, SlotsHost};
use smallvec::SmallVec;

use super::*;

#[test]
fn bubbling_preserves_the_subcompose_nodes_existing_owner() {
    let _app_context = crate::render_state::app_context_test_scope();
    let policy: Rc<MeasurePolicy> = Rc::new(|scope, _| scope.layout(0.0, 0.0, Vec::new()));
    let mut node = SubcomposeLayoutNode::new(Modifier::empty(), policy);
    node.set_parent_for_bubbling(41);
    node.set_parent_for_bubbling(42);
    assert_eq!(node.parent(), Some(41));
    node.on_removed_from_parent();
    node.set_parent_for_bubbling(42);
    assert_eq!(node.parent(), Some(42));
}

#[derive(Default)]
struct DummyNode;

impl Node for DummyNode {}

fn runtime_handle() -> (
    RuntimeHandle,
    cranpose_core::Composition<cranpose_core::MemoryApplier>,
) {
    let composition = cranpose_core::Composition::new(cranpose_core::MemoryApplier::new());
    let handle = composition.runtime_handle();
    (handle, composition)
}

fn setup_composer(
    slots: &mut SlotTable,
    applier: &mut cranpose_core::MemoryApplier,
    handle: RuntimeHandle,
    root: Option<NodeId>,
) -> (
    Composer,
    Rc<SlotsHost>,
    Rc<ConcreteApplierHost<cranpose_core::MemoryApplier>>,
) {
    let slots_host = Rc::new(SlotsHost::new(std::mem::take(slots)));
    let applier_host = Rc::new(ConcreteApplierHost::new(std::mem::replace(
        applier,
        cranpose_core::MemoryApplier::new(),
    )));
    let composer = Composer::new(
        Rc::clone(&slots_host),
        Rc::<ConcreteApplierHost<cranpose_core::MemoryApplier>>::clone(&applier_host),
        handle,
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
        .try_into_inner()
        .unwrap_or_else(|(_, error)| panic!("applier disposal failed: {error}"));
}

/// Measures the subcompose node `node_id` of `applier` under `constraints`,
/// as a layout pass does, with `measurer` for its children.
pub(crate) fn measure_subcompose_with(
    applier: &mut cranpose_core::MemoryApplier,
    handle: &RuntimeHandle,
    node_id: NodeId,
    constraints: Constraints,
    measurer: Box<dyn FnMut(NodeId, Constraints) -> SubcomposePlaceable>,
) -> MeasureResult {
    let applier_host = Rc::new(ConcreteApplierHost::new(std::mem::replace(
        applier,
        cranpose_core::MemoryApplier::new(),
    )));
    let node_handle = {
        let mut applier_ref = applier_host.borrow_typed();
        let node = applier_ref.get_mut(node_id).expect("node available");
        let typed = node
            .as_any_mut()
            .downcast_mut::<SubcomposeLayoutNode>()
            .expect("subcompose layout node");
        typed.handle()
    };
    let error = Rc::new(RefCell::new(None));
    let result = node_handle
        .measure_with_callbacks(
            &(Rc::clone(&applier_host) as Rc<dyn ApplierHost>),
            handle,
            node_id,
            constraints,
            MeasureCallbacks {
                measurer,
                retained_measure_lookup: Box::new(|_| None),
                error: &error,
            },
        )
        .expect("measure result");
    assert!(
        error.borrow().is_none(),
        "unexpected subcompose measure error"
    );
    drop(node_handle);
    *applier = Rc::try_unwrap(applier_host)
        .unwrap_or_else(|_| panic!("applier host still has outstanding references"))
        .try_into_inner()
        .unwrap_or_else(|(_, error)| panic!("applier disposal failed: {error}"));
    result
}

fn measure_once(
    applier: &mut cranpose_core::MemoryApplier,
    handle: &RuntimeHandle,
    node_id: NodeId,
    constraints: Constraints,
) -> MeasureResult {
    measure_subcompose_with(
        applier,
        handle,
        node_id,
        constraints,
        Box::new(|child_id, _constraints| SubcomposePlaceable::value(0.0, 0.0, child_id)),
    )
}

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
    let node = SubcomposeLayoutNode::new(Modifier::empty(), Rc::clone(&policy));
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
        measurer: Box::new(|child_id, _constraints| SubcomposePlaceable::value(0.0, 0.0, child_id)),
        retained_measure_lookup: Box::new(|_| None),
        error: &error,
        parent_handle,
        root_id,
        scratch: MeasureScratch::default(),
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
fn measure_subcomposes_content() {
    let _app_context = crate::render_state::app_context_test_scope();
    let (handle, _composition) = runtime_handle();
    let mut applier = cranpose_core::MemoryApplier::new();
    let recorded = Rc::new(RefCell::new(Vec::new()));
    let recorded_capture = Rc::clone(&recorded);
    let policy: Rc<MeasurePolicy> = Rc::new(move |scope, constraints| {
        assert_eq!(constraints, Constraints::tight(0.0, 0.0));
        let measurables = scope.subcompose(SlotId::new(1), (), || {
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
        Modifier::empty(),
        Rc::clone(&policy),
    )));
    let result = measure_once(&mut applier, &handle, node_id, Constraints::tight(0.0, 0.0));
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
    let mut applier = cranpose_core::MemoryApplier::new();
    let recorded = Rc::new(RefCell::new(Vec::new()));
    let recorded_capture = Rc::clone(&recorded);
    let policy: Rc<MeasurePolicy> = Rc::new(move |scope, _constraints| {
        let measurables = scope.subcompose(SlotId::new(99), (), || {
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
        Modifier::empty(),
        Rc::clone(&policy),
    )));
    let _ = measure_once(
        &mut applier,
        &handle,
        node_id,
        Constraints::loose(100.0, 100.0),
    );
    let _ = measure_once(
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
fn retained_slot_activation_checks_child_identity_order_and_length() {
    let _app_context = crate::render_state::app_context_test_scope();
    let (handle, _composition) = runtime_handle();
    let mut applier = cranpose_core::MemoryApplier::new();
    let expected = Rc::new(RefCell::new(None::<Vec<u64>>));
    let observed = Rc::new(RefCell::new(Vec::new()));
    let expected_for_policy = Rc::clone(&expected);
    let observed_for_policy = Rc::clone(&observed);
    let policy: Rc<MeasurePolicy> = Rc::new(move |scope, _constraints| {
        let (children, matches) = match expected_for_policy.borrow().as_ref() {
            Some(expected) => match scope
                .activate_exact_retained_slot_with_known_children(SlotId::new(99), expected)
                .expect("retained slot activates")
            {
                RetainedSlotChildren::Clean => (
                    expected
                        .iter()
                        .map(|&id| SubcomposeChild::new(NodeId::try_from(id).expect("node id")))
                        .collect(),
                    true,
                ),
                RetainedSlotChildren::Remeasure {
                    children,
                    children_match,
                } => (children, children_match),
            },
            None => (
                scope.subcompose(SlotId::new(99), (), || {
                    cranpose_core::with_current_composer(|composer| {
                        composer.emit_node(|| DummyNode);
                        composer.emit_node(|| DummyNode);
                    });
                }),
                true,
            ),
        };
        assert!(children.iter().all(|child| child.size() == Size::default()));
        observed_for_policy.borrow_mut().push((
            children
                .iter()
                .map(|child| child.node_id() as u64)
                .collect::<Vec<_>>(),
            matches,
        ));
        scope.layout(0.0, 0.0, Vec::new())
    });
    let node_id = applier.create(Box::new(SubcomposeLayoutNode::new(
        Modifier::empty(),
        policy,
    )));
    measure_once(
        &mut applier,
        &handle,
        node_id,
        Constraints::loose(100.0, 100.0),
    );
    let original = observed.borrow()[0].0.clone();
    assert_eq!(original.len(), 2);
    assert_ne!(original[0], original[1]);
    let cases = [
        original.clone(),
        vec![original[1], original[0]],
        vec![original[0]],
        vec![original[0], original[1], original[0]],
        Vec::new(),
        vec![original[0], original[0]],
        original.clone(),
    ];
    for (index, known) in cases.into_iter().enumerate() {
        let should_match = known == original;
        *expected.borrow_mut() = Some(known);
        measure_once(
            &mut applier,
            &handle,
            node_id,
            Constraints::loose(101.0 + index as f32, 100.0),
        );
        assert_eq!(observed.borrow().len(), index + 2);
        assert_eq!(
            observed.borrow().last(),
            Some(&(original.clone(), should_match))
        );
    }
}

#[test]
fn handle_reports_modifier_capabilities() {
    let _app_context = crate::render_state::app_context_test_scope();
    let policy: Rc<MeasurePolicy> =
        Rc::new(|scope, _constraints| scope.layout(0.0, 0.0, Vec::new()));
    let mut node = SubcomposeLayoutNode::new(
        Modifier::empty().background(crate::modifier::Color(0.1, 0.2, 0.3, 1.0)),
        Rc::clone(&policy),
    );

    let handle = node.handle();
    assert!(handle.has_draw_modifier_nodes());
    assert!(!handle.has_layout_modifier_nodes());

    node.set_modifier(Modifier::empty().padding(4.0));
    let handle = node.handle();
    assert!(handle.has_layout_modifier_nodes());
    assert!(!handle.has_draw_modifier_nodes());
}

#[test]
fn inactive_slots_move_to_reusable_pool() {
    let _app_context = crate::render_state::app_context_test_scope();
    let (handle, _composition) = runtime_handle();
    let mut applier = cranpose_core::MemoryApplier::new();
    let toggle = MutableState::with_runtime(true, handle.clone());
    let toggle_capture = toggle;
    let policy: Rc<MeasurePolicy> = Rc::new(move |scope, _constraints| {
        if toggle_capture.value() {
            scope.subcompose(SlotId::new(1), (), || {
                cranpose_core::with_current_composer(|composer| {
                    composer.emit_node(|| DummyNode);
                });
            });
        }
        scope.layout(0.0, 0.0, Vec::new())
    });
    let node_id = applier.create(Box::new(SubcomposeLayoutNode::new(
        Modifier::empty(),
        Rc::clone(&policy),
    )));
    let _ = measure_once(
        &mut applier,
        &handle,
        node_id,
        Constraints::loose(50.0, 50.0),
    );
    toggle.set(false);

    let _ = measure_once(
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
    let node = SubcomposeLayoutNode::new(Modifier::empty(), Rc::clone(&policy));

    {
        let mut inner = node.inner.borrow_mut();
        inner.children = vec![11, 22];
        inner.last_placements = Rc::new((33..45).collect());
    }

    let expected: Vec<_> = (33..45).collect();
    node.with_active_children(|active| assert_eq!(active, expected.as_slice()));
    let mut active = SmallVec::<[NodeId; 8]>::from_slice(&[99]);
    Node::collect_children_into(&node, &mut active);
    assert_eq!(active.as_slice(), expected.as_slice());

    node.handle().set_active_children(Vec::<NodeId>::new());
    node.with_active_children(|active| assert!(active.is_empty()));
    Node::collect_children_into(&node, &mut active);
    assert!(active.is_empty());

    let mut owned = SmallVec::<[NodeId; 8]>::new();
    Node::collect_owned_children_into(&node, &mut owned);
    assert_eq!(owned.as_slice(), &[11, 22]);
}

#[test]
fn subcompose_reuses_measured_children_scratch_map() {
    let _app_context = crate::render_state::app_context_test_scope();
    let policy: Rc<MeasurePolicy> =
        Rc::new(|scope, _constraints| scope.layout(0.0, 0.0, Vec::new()));
    let node = SubcomposeLayoutNode::new(Modifier::empty(), Rc::clone(&policy));
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
fn active_child_storage_is_shared_only_until_the_reader_returns() {
    let _app_context = crate::render_state::app_context_test_scope();
    let policy: Rc<MeasurePolicy> =
        Rc::new(|scope, _constraints| scope.layout(0.0, 0.0, Vec::new()));
    let node = SubcomposeLayoutNode::new(Modifier::empty(), policy);
    let handle = node.handle();
    handle.set_active_children(1..13);
    let original = node.with_active_children(<[NodeId]>::as_ptr);
    node.with_active_children(|children| {
        handle.set_active_children(children.iter().copied());
        node.with_active_children(|unchanged| assert_eq!(unchanged.as_ptr(), original));
        handle.set_active_children([7, 8, 9]);
        assert_eq!(children, &(1..13).collect::<Vec<_>>());
        node.with_active_children(|changed| {
            assert_eq!(changed, &[7, 8, 9]);
            assert_ne!(changed.as_ptr(), original);
        });
    });
    assert_eq!(Rc::strong_count(&node.inner.borrow().last_placements), 1);
    let exclusive = node.with_active_children(<[NodeId]>::as_ptr);
    handle.set_active_children([9, 8, 7]);
    node.with_active_children(|children| {
        assert_eq!(children, &[9, 8, 7]);
        assert_eq!(children.as_ptr(), exclusive);
    });
}

#[test]
fn subcompose_scope_layout_uses_retained_placement_scratch() {
    let _app_context = crate::render_state::app_context_test_scope();
    let (handle, _composition) = runtime_handle();
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
    let node = SubcomposeLayoutNode::new(Modifier::empty(), Rc::clone(&policy));
    let node_handle = node.handle();
    node_handle.recycle_placement_scratch(Vec::with_capacity(16));
    let node_id = applier.create(Box::new(node));

    let result = measure_once(&mut applier, &handle, node_id, Constraints::tight(0.0, 0.0));

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
    let mut node = SubcomposeLayoutNode::new(Modifier::empty(), Rc::clone(&policy));
    node.set_modifier(Modifier::empty().padding(2.0));
    let snapshot = node.modifier_slices_snapshot();
    let snapshot_ptr = Rc::as_ptr(&snapshot);
    drop(snapshot);

    node.set_modifier(Modifier::empty().padding(4.0));

    let updated = node.modifier_slices_snapshot();
    assert_eq!(Rc::as_ptr(&updated), snapshot_ptr);
}

#[test]
fn semantics_configuration_reads_the_live_modifier_chain() {
    let _app_context = crate::render_state::app_context_test_scope();
    let policy: Rc<MeasurePolicy> = Rc::new(|scope, _| scope.layout(0.0, 0.0, Vec::new()));
    let node = SubcomposeLayoutNode::new(Modifier::empty(), Rc::clone(&policy));
    assert_eq!(node.semantics_configuration(), None);

    let described =
        SubcomposeLayoutNode::new(Modifier::empty().content_description("Feed"), policy);
    assert_eq!(
        described
            .semantics_configuration()
            .and_then(|config| config.content_description)
            .as_deref(),
        Some("Feed")
    );
}
