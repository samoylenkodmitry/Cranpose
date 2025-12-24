use super::*;
use std::cell::RefCell;
use std::rc::Rc;

use compose_core::{
    self, Applier, ConcreteApplierHost, MutableState, SlotBackend, SlotStorage, SlotsHost,
    SnapshotStateObserver,
};

#[derive(Default)]
struct DummyNode;

impl compose_core::Node for DummyNode {}

fn runtime_handle() -> (
    compose_core::RuntimeHandle,
    compose_core::Composition<compose_core::MemoryApplier>,
) {
    let composition = compose_core::Composition::new(compose_core::MemoryApplier::new());
    let handle = composition.runtime_handle();
    (handle, composition)
}

fn setup_composer(
    slots: &mut SlotBackend,
    applier: &mut compose_core::MemoryApplier,
    handle: compose_core::RuntimeHandle,
    root: Option<compose_core::NodeId>,
) -> (
    compose_core::Composer,
    Rc<SlotsHost>,
    Rc<ConcreteApplierHost<compose_core::MemoryApplier>>,
) {
    let slots_host = Rc::new(SlotsHost::new(std::mem::take(slots)));
    let applier_host = Rc::new(ConcreteApplierHost::new(std::mem::replace(
        applier,
        compose_core::MemoryApplier::new(),
    )));
    let observer = SnapshotStateObserver::new(|callback| callback());
    let composer = compose_core::Composer::new(
        Rc::clone(&slots_host),
        applier_host.clone(),
        handle,
        observer,
        root,
    );
    (composer, slots_host, applier_host)
}

fn teardown_composer(
    slots: &mut SlotBackend,
    applier: &mut compose_core::MemoryApplier,
    slots_host: Rc<SlotsHost>,
    applier_host: Rc<ConcreteApplierHost<compose_core::MemoryApplier>>,
) {
    *slots = Rc::try_unwrap(slots_host)
        .unwrap_or_else(|_| panic!("slots host still has outstanding references"))
        .take();
    *applier = Rc::try_unwrap(applier_host)
        .unwrap_or_else(|_| panic!("applier host still has outstanding references"))
        .into_inner();
}

fn measure_once(
    slots: &mut SlotBackend,
    applier: &mut compose_core::MemoryApplier,
    handle: &compose_core::RuntimeHandle,
    node_id: compose_core::NodeId,
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
        Box::new(|_child_id: compose_core::NodeId, _constraints: Constraints| Size::default());
    let error = Rc::new(RefCell::new(None));
    let result = node_handle
        .measure(&composer, node_id, constraints, measurer, Rc::clone(&error))
        .expect("measure result");
    assert!(
        error.borrow().is_none(),
        "unexpected subcompose measure error"
    );
    drop(composer);
    teardown_composer(slots, applier, slots_host, applier_host);
    result
}

#[test]
fn measure_subcomposes_content() {
    let (handle, _composition) = runtime_handle();
    let mut slots = SlotBackend::default();
    let mut applier = compose_core::MemoryApplier::new();
    let recorded = Rc::new(RefCell::new(Vec::new()));
    let recorded_capture = Rc::clone(&recorded);
    let policy: Rc<MeasurePolicy> = Rc::new(move |scope, constraints| {
        assert_eq!(constraints, Constraints::tight(0.0, 0.0));
        let measurables = scope.subcompose(SlotId::new(1), || {
            compose_core::with_current_composer(|composer| {
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
    let (handle, _composition) = runtime_handle();
    let mut slots = SlotBackend::default();
    let mut applier = compose_core::MemoryApplier::new();
    let recorded = Rc::new(RefCell::new(Vec::new()));
    let recorded_capture = Rc::clone(&recorded);
    let policy: Rc<MeasurePolicy> = Rc::new(move |scope, _constraints| {
        let measurables = scope.subcompose(SlotId::new(99), || {
            compose_core::with_current_composer(|composer| {
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
    slots.reset();
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
    let (handle, _composition) = runtime_handle();
    let mut slots = SlotBackend::default();
    let mut applier = compose_core::MemoryApplier::new();
    let toggle = MutableState::with_runtime(true, handle.clone());
    let toggle_capture = toggle;
    let policy: Rc<MeasurePolicy> = Rc::new(move |scope, _constraints| {
        if toggle_capture.value() {
            scope.subcompose(SlotId::new(1), || {
                compose_core::with_current_composer(|composer| {
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

    slots.reset();
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
