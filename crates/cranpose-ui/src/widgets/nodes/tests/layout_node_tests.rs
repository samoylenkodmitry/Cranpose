use std::rc::Rc;

use cranpose_ui_graphics::Size as GeometrySize;
use cranpose_ui_layout::{Measurable, MeasureResult, MeasureScope};

use super::*;

#[derive(Default)]
struct TestMeasurePolicy;

impl MeasurePolicy for TestMeasurePolicy {
    fn measure(
        &self,
        _scope: &dyn MeasureScope,
        _measurables: &[Box<dyn Measurable>],
        _constraints: Constraints,
    ) -> MeasureResult {
        MeasureResult::new(
            GeometrySize {
                width: 0.0,
                height: 0.0,
            },
            Vec::new(),
        )
    }

    fn min_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        0.0
    }

    fn max_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        0.0
    }

    fn min_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
        0.0
    }

    fn max_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
        0.0
    }
}

fn fresh_node() -> LayoutNode {
    LayoutNode::new(Modifier::empty(), Rc::new(TestMeasurePolicy))
}

#[test]
fn bubbling_preserves_the_layout_nodes_existing_owner() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut node = fresh_node();
    node.set_parent_for_bubbling(41);
    node.set_parent_for_bubbling(42);
    assert_eq!(node.parent(), Some(41));
    node.on_removed_from_parent();
    node.set_parent_for_bubbling(42);
    assert_eq!(node.parent(), Some(42));
}

#[test]
fn modifier_slices_cache_reuses_unique_snapshot_allocation() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut node = fresh_node();
    let snapshot = node.modifier_slices_snapshot();
    let snapshot_ptr = Rc::as_ptr(&snapshot);
    drop(snapshot);

    node.set_modifier(Modifier::empty().padding(4.0));

    let updated = node.modifier_slices_snapshot();
    assert_eq!(Rc::as_ptr(&updated), snapshot_ptr);
}

#[test]
fn modifier_slices_cache_preserves_live_snapshot_isolation() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut node = fresh_node();
    let old_snapshot = node.modifier_slices_snapshot();
    let old_snapshot_ptr = Rc::as_ptr(&old_snapshot);

    node.set_modifier(Modifier::empty().padding(4.0));

    let updated = node.modifier_slices_snapshot();
    assert_ne!(Rc::as_ptr(&updated), old_snapshot_ptr);
    assert_eq!(old_snapshot.draw_commands().len(), 0);
}

#[test]
fn layout_node_registry_retains_warm_capacity_after_large_cleanup() {
    let _app_context = crate::render_state::app_context_test_scope();
    let app_context = crate::render_state::AppContext::new_with_density(1.0);
    app_context.enter(|| {
        let nodes: Vec<_> = (0..2048)
            .map(|_| {
                let id = allocate_virtual_node_id();
                let node = fresh_node();
                let owner_context_id = register_layout_node(id, &node);
                (id, owner_context_id, node)
            })
            .collect();

        for (id, owner_context_id, _) in &nodes {
            unregister_layout_node(*owner_context_id, *id);
        }

        let stats = layout_node_registry_stats();
        assert_eq!(stats.len, 0);
        assert!(
            (MIN_RETAINED_LAYOUT_NODE_REGISTRY_CAPACITY
                ..=MIN_RETAINED_LAYOUT_NODE_REGISTRY_CAPACITY.saturating_mul(2))
                .contains(&stats.capacity),
            "registry warm capacity {} fell outside expected retained range {}..={}",
            stats.capacity,
            MIN_RETAINED_LAYOUT_NODE_REGISTRY_CAPACITY,
            MIN_RETAINED_LAYOUT_NODE_REGISTRY_CAPACITY.saturating_mul(2),
        );
    });
}

#[test]
fn layout_node_registry_is_scoped_by_app_context() {
    let _app_context = crate::render_state::app_context_test_scope();
    let first = crate::render_state::AppContext::new_with_density(1.0);
    let second = crate::render_state::AppContext::new_with_density(1.0);

    let first_id = first.enter(allocate_virtual_node_id);
    let second_id = second.enter(allocate_virtual_node_id);

    assert_eq!(first_id, VIRTUAL_NODE_ID_START);
    assert_eq!(second_id, VIRTUAL_NODE_ID_START);

    let virtual_node = LayoutNode::new_virtual();
    let regular_node = fresh_node();

    first.enter(|| {
        register_layout_node(first_id, &virtual_node);
        register_layout_node(101, &regular_node);
        assert!(is_virtual_node(first_id));
        assert_eq!(layout_node_registry_stats().len, 2);
    });

    second.enter(|| {
        assert!(!is_virtual_node(first_id));
        assert_eq!(layout_node_registry_stats().len, 0);
        assert_eq!(allocate_virtual_node_id(), VIRTUAL_NODE_ID_START + 1);
    });

    first.enter(|| {
        let owner_context_id = crate::render_state::current_app_context_id();
        unregister_layout_node(owner_context_id, first_id);
        unregister_layout_node(owner_context_id, 101);
        assert_eq!(layout_node_registry_stats().len, 0);
    });
}

fn invalidation(kind: InvalidationKind) -> ModifierInvalidation {
    ModifierInvalidation::new(kind, NodeCapabilities::for_invalidation(kind))
}

#[test]
fn layout_invalidation_requires_layout_capability() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut node = fresh_node();
    node.clear_needs_measure();
    node.clear_needs_layout();
    node.modifier_capabilities = NodeCapabilities::DRAW;
    node.modifier_child_capabilities = node.modifier_capabilities;

    node.dispatch_modifier_invalidations(&[invalidation(InvalidationKind::Layout)]);

    assert!(!node.needs_measure());
    assert!(!node.needs_layout());
}

#[test]
fn semantics_configuration_reflects_modifier_state() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut node = fresh_node();
    node.set_modifier(Modifier::empty().semantics(|config| {
        config.content_description = Some("greeting".into());
        config.is_clickable = true;
    }));

    let config = node
        .semantics_configuration()
        .expect("expected semantics configuration");
    assert_eq!(config.content_description.as_deref(), Some("greeting"));
    assert!(config.is_clickable);
}

#[test]
fn layout_invalidation_marks_flags_when_capability_present() {
    let _app_context = crate::render_state::app_context_test_scope();
    let _guard = crate::render_state::render_state_test_guard();
    crate::reset_render_state_for_tests();
    let mut node = fresh_node();
    node.id.set(Some(11));
    node.clear_needs_measure();
    node.clear_needs_layout();
    node.modifier_capabilities = NodeCapabilities::LAYOUT;
    node.modifier_child_capabilities = node.modifier_capabilities;

    node.dispatch_modifier_invalidations(&[invalidation(InvalidationKind::Layout)]);

    assert!(node.needs_measure());
    assert!(node.needs_layout());
    assert_eq!(crate::take_layout_repass_nodes(), vec![11]);
    assert!(
        !crate::take_layout_invalidation(),
        "a scoped modifier invalidation must not invalidate the whole tree"
    );
}

#[test]
fn layout_invalidation_skips_repass_while_composing() {
    let _app_context = crate::render_state::app_context_test_scope();
    let _guard = crate::render_state::render_state_test_guard();
    crate::reset_render_state_for_tests();

    let node = Rc::new(RefCell::new(fresh_node()));
    {
        let mut node = node.borrow_mut();
        node.id.set(Some(17));
        node.clear_needs_measure();
        node.clear_needs_layout();
        node.modifier_capabilities = NodeCapabilities::LAYOUT;
        node.modifier_child_capabilities = node.modifier_capabilities;
    }

    let node_for_composition = Rc::clone(&node);
    let _composition = crate::run_test_composition(move || {
        node_for_composition
            .borrow()
            .dispatch_modifier_invalidations(&[invalidation(InvalidationKind::Layout)]);
    });

    let node = node.borrow();
    assert!(node.needs_measure());
    assert!(node.needs_layout());
    assert!(crate::take_layout_repass_nodes().is_empty());
    assert!(!crate::take_layout_invalidation());
}

#[test]
fn draw_invalidation_marks_redraw_flag_when_capable() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut node = fresh_node();
    node.clear_needs_measure();
    node.clear_needs_layout();
    node.modifier_capabilities = NodeCapabilities::DRAW;
    node.modifier_child_capabilities = node.modifier_capabilities;

    node.dispatch_modifier_invalidations(&[invalidation(InvalidationKind::Draw)]);

    assert!(node.needs_redraw());
    assert!(!node.needs_layout());
}

#[test]
fn draw_invalidation_capability_marks_redraw_for_layout_modifier_update() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut node = fresh_node();
    node.clear_needs_measure();
    node.clear_needs_layout();
    node.clear_needs_redraw();
    node.modifier_capabilities = NodeCapabilities::LAYOUT;
    node.modifier_child_capabilities = node.modifier_capabilities;

    node.dispatch_modifier_invalidations(&[ModifierInvalidation::new(
        InvalidationKind::Draw,
        NodeCapabilities::DRAW,
    )]);

    assert!(node.needs_redraw());
    assert!(!node.needs_measure());
    assert!(!node.needs_layout());
}

#[test]
fn semantics_invalidation_sets_semantics_flag_only() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut node = fresh_node();
    node.clear_needs_measure();
    node.clear_needs_layout();
    node.clear_needs_semantics();
    node.modifier_capabilities = NodeCapabilities::SEMANTICS;
    node.modifier_child_capabilities = node.modifier_capabilities;

    node.dispatch_modifier_invalidations(&[invalidation(InvalidationKind::Semantics)]);

    assert!(node.needs_semantics());
    assert!(!node.needs_measure());
    assert!(!node.needs_layout());
}

#[test]
fn pointer_invalidation_requires_pointer_capability() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut node = fresh_node();
    node.clear_needs_pointer_pass();
    node.modifier_capabilities = NodeCapabilities::DRAW;
    node.modifier_child_capabilities = node.modifier_capabilities;

    node.dispatch_modifier_invalidations(&[invalidation(InvalidationKind::PointerInput)]);

    assert!(!node.needs_pointer_pass());
}

#[test]
fn pointer_invalidation_marks_flag_and_requests_queue() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut node = fresh_node();
    node.clear_needs_pointer_pass();
    node.modifier_capabilities = NodeCapabilities::POINTER_INPUT;
    node.modifier_child_capabilities = node.modifier_capabilities;

    node.dispatch_modifier_invalidations(&[invalidation(InvalidationKind::PointerInput)]);

    assert!(node.needs_pointer_pass());
}

#[test]
fn focus_invalidation_requires_focus_capability() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut node = fresh_node();
    node.clear_needs_focus_sync();
    node.modifier_capabilities = NodeCapabilities::DRAW;
    node.modifier_child_capabilities = node.modifier_capabilities;
    crate::take_focus_invalidation();

    node.dispatch_modifier_invalidations(&[invalidation(InvalidationKind::Focus)]);

    assert!(!node.needs_focus_sync());
    assert!(!crate::take_focus_invalidation());
}

#[test]
fn focus_invalidation_marks_flag_and_requests_queue() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut node = fresh_node();
    node.clear_needs_focus_sync();
    node.modifier_capabilities = NodeCapabilities::FOCUS;
    node.modifier_child_capabilities = node.modifier_capabilities;
    crate::take_focus_invalidation();

    node.dispatch_modifier_invalidations(&[invalidation(InvalidationKind::Focus)]);

    assert!(node.needs_focus_sync());
    assert!(crate::take_focus_invalidation());
}

#[test]
fn set_modifier_marks_semantics_dirty() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut node = fresh_node();
    node.clear_needs_semantics();
    node.set_modifier(Modifier::empty().semantics(|config| {
        config.is_clickable = true;
    }));

    assert!(node.needs_semantics());
}

#[test]
fn modifier_child_capabilities_reflect_chain_head() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut node = fresh_node();
    node.set_modifier(Modifier::empty().padding(4.0));
    assert!(
        node.modifier_child_capabilities()
            .contains(NodeCapabilities::LAYOUT),
        "padding should introduce layout capability"
    );
}
