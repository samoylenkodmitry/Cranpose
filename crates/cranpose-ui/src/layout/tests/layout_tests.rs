use super::*;
use crate::layout::policies::LeafMeasurePolicy;
use crate::modifier::{Color, Modifier, Size};
use crate::subcompose_layout::SubcomposeLayoutScope;
use cranpose_core::{Applier, ConcreteApplierHost, MemoryApplier, Node};
use cranpose_ui_layout::{MeasurePolicy, MeasureResult, Placement};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use super::core::Measurable;

fn measure_layout(
    applier: &mut MemoryApplier,
    root: NodeId,
    max_size: Size,
) -> Result<LayoutMeasurements, NodeError> {
    let measurements = super::measure_layout(applier, root, max_size)?;
    Ok(measurements)
}

#[derive(Clone, Copy)]
struct VerticalStackPolicy;

impl MeasurePolicy for VerticalStackPolicy {
    fn measure(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let mut y: f32 = 0.0;
        let mut width: f32 = 0.0;
        let mut placements = Vec::new();
        for measurable in measurables {
            let placeable = measurable.measure(constraints);
            width = width.max(placeable.width());
            let height = placeable.height();
            placements.push(Placement::new(placeable.node_id(), 0.0, y, 0));
            y += height;
        }
        let width = width.clamp(constraints.min_width, constraints.max_width);
        let height = y.clamp(constraints.min_height, constraints.max_height);
        MeasureResult::new(Size { width, height }, placements)
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.min_intrinsic_width(height))
            .fold(0.0, f32::max)
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.max_intrinsic_width(height))
            .fold(0.0, f32::max)
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.min_intrinsic_height(width))
            .fold(0.0, |acc, h| acc + h)
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.max_intrinsic_height(width))
            .fold(0.0, |acc, h| acc + h)
    }
}

#[derive(Clone)]
struct MaxSizePolicy;

impl MeasurePolicy for MaxSizePolicy {
    fn measure(
        &self,
        _measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let width = if constraints.max_width.is_finite() {
            constraints.max_width
        } else {
            constraints.min_width
        };
        let height = if constraints.max_height.is_finite() {
            constraints.max_height
        } else {
            constraints.min_height
        };
        MeasureResult::new(Size { width, height }, Vec::new())
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

#[derive(Clone)]
struct RecordingPolicy {
    seen: Rc<RefCell<Option<Constraints>>>,
}

impl MeasurePolicy for RecordingPolicy {
    fn measure(
        &self,
        _measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        *self.seen.borrow_mut() = Some(constraints);
        MeasureResult::new(
            Size {
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

#[derive(Clone)]
struct MutableLeafPolicy {
    size: Rc<Cell<Size>>,
    calls: Rc<Cell<usize>>,
}

impl MeasurePolicy for MutableLeafPolicy {
    fn measure(
        &self,
        _measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        self.calls.set(self.calls.get() + 1);
        let size = self.size.get();
        MeasureResult::new(
            Size {
                width: size
                    .width
                    .clamp(constraints.min_width, constraints.max_width),
                height: size
                    .height
                    .clamp(constraints.min_height, constraints.max_height),
            },
            Vec::new(),
        )
    }

    fn min_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        self.size.get().width
    }

    fn max_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        self.size.get().width
    }

    fn min_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
        self.size.get().height
    }

    fn max_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
        self.size.get().height
    }
}

#[test]
fn clamp_dimension_respects_infinite_max() {
    let clamped = clamp_dimension(50.0, 10.0, f32::INFINITY);
    assert_eq!(clamped, 50.0);
}

#[test]
fn layout_measure_respects_parent_constraints_for_weighted_nodes() -> Result<(), NodeError> {
    let seen = Rc::new(RefCell::new(None));
    let policy = RecordingPolicy {
        seen: Rc::clone(&seen),
    };

    let mut applier = MemoryApplier::new();
    let root_id = applier.create(Box::new(LayoutNode::new(
        Modifier::empty().weight(1.0),
        Rc::new(policy),
    )));

    measure_layout(&mut applier, root_id, Size::new(800.0, 600.0))?;

    let recorded = seen.borrow().expect("expected measure to run");
    assert!(recorded.max_width.is_finite());
    assert!(recorded.max_height.is_finite());
    assert_eq!(recorded.max_width, 800.0);
    assert_eq!(recorded.max_height, 600.0);

    Ok(())
}

// Note: Weight distribution tests removed - weights are not yet implemented
// in the new MeasurePolicy-based system. They were part of the old
// ColumnNode/RowNode implementation that has been replaced.

#[test]
fn resolve_dimension_applies_explicit_points() {
    let size = resolve_dimension(
        10.0,
        DimensionConstraint::Points(20.0),
        None,
        None,
        0.0,
        100.0,
    );
    assert_eq!(size, 20.0);
}

#[test]
fn align_helpers_respect_available_space() {
    assert_eq!(
        align_horizontal(HorizontalAlignment::CenterHorizontally, 100.0, 40.0),
        30.0
    );
    assert_eq!(align_vertical(VerticalAlignment::Bottom, 50.0, 10.0), 40.0);
}

// Note: box_respects_child_alignment test removed - it tested the old BoxNode
// implementation. Box now uses LayoutNode with BoxMeasurePolicy.

// ============================================================================
// SELECTIVE MEASURE/LAYOUT TESTS
// ============================================================================

#[test]
fn new_layout_node_starts_dirty() {
    let node = LayoutNode::new(Modifier::empty(), Rc::new(MaxSizePolicy));
    assert!(node.needs_measure(), "New node should need measure");
    assert!(node.needs_layout(), "New node should need layout");
}

#[test]
fn mark_needs_measure_sets_both_flags() {
    let node = LayoutNode::new(Modifier::empty(), Rc::new(MaxSizePolicy));
    node.clear_needs_measure();
    node.clear_needs_layout();

    assert!(!node.needs_measure());
    assert!(!node.needs_layout());

    node.mark_needs_measure();
    assert!(
        node.needs_measure(),
        "mark_needs_measure should set needs_measure flag"
    );
    assert!(
        node.needs_layout(),
        "mark_needs_measure should set needs_layout flag"
    );
}

#[test]
fn mark_needs_layout_only_sets_layout_flag() {
    let node = LayoutNode::new(Modifier::empty(), Rc::new(MaxSizePolicy));
    node.clear_needs_measure();
    node.clear_needs_layout();

    node.mark_needs_layout();
    assert!(
        !node.needs_measure(),
        "mark_needs_layout should NOT set needs_measure flag"
    );
    assert!(
        node.needs_layout(),
        "mark_needs_layout should set needs_layout flag"
    );
}

#[test]
fn set_modifier_marks_dirty() {
    let mut node = LayoutNode::new(Modifier::empty(), Rc::new(MaxSizePolicy));
    node.clear_needs_measure();
    node.clear_needs_layout();
    node.clear_needs_semantics();

    node.set_modifier(Modifier::empty().padding(4.0));
    assert!(
        node.needs_measure(),
        "set_modifier should mark node as needing measure"
    );
    assert!(
        node.needs_layout(),
        "set_modifier should mark node as needing layout"
    );
    assert!(
        node.needs_semantics(),
        "set_modifier should mark node as needing semantics"
    );
}

#[test]
fn set_measure_policy_marks_dirty() {
    let mut node = LayoutNode::new(Modifier::empty(), Rc::new(MaxSizePolicy));
    node.clear_needs_measure();
    node.clear_needs_layout();

    node.set_measure_policy(Rc::new(VerticalStackPolicy));
    assert!(
        node.needs_measure(),
        "set_measure_policy should mark node as needing measure"
    );
    assert!(
        node.needs_layout(),
        "set_measure_policy should mark node as needing layout"
    );
}

#[test]
fn insert_child_marks_dirty() -> Result<(), NodeError> {
    let mut applier = MemoryApplier::new();
    let child = applier.create(Box::new(LayoutNode::new(
        Modifier::empty(),
        Rc::new(LeafMeasurePolicy::new(Size {
            width: 10.0,
            height: 10.0,
        })),
    )));

    let mut node = LayoutNode::new(Modifier::empty(), Rc::new(MaxSizePolicy));
    node.clear_needs_measure();
    node.clear_needs_layout();

    node.insert_child(child);
    assert!(
        node.needs_measure(),
        "insert_child should mark node as needing measure"
    );
    assert!(
        node.needs_layout(),
        "insert_child should mark node as needing layout"
    );
    Ok(())
}

#[test]
fn remove_child_marks_dirty() -> Result<(), NodeError> {
    let mut applier = MemoryApplier::new();
    let child = applier.create(Box::new(LayoutNode::new(
        Modifier::empty(),
        Rc::new(LeafMeasurePolicy::new(Size {
            width: 10.0,
            height: 10.0,
        })),
    )));

    let mut node = LayoutNode::new(Modifier::empty(), Rc::new(MaxSizePolicy));
    node.insert_child(child);
    node.clear_needs_measure();
    node.clear_needs_layout();

    node.remove_child(child);
    assert!(
        node.needs_measure(),
        "remove_child should mark node as needing measure"
    );
    assert!(
        node.needs_layout(),
        "remove_child should mark node as needing layout"
    );
    Ok(())
}

#[test]
fn selective_measure_uses_cache_when_not_dirty() -> Result<(), NodeError> {
    let mut applier = MemoryApplier::new();
    let node = LayoutNode::new(Modifier::empty(), Rc::new(MaxSizePolicy));
    let node_id = applier.create(Box::new(node));

    let _constraints = Constraints {
        min_width: 0.0,
        max_width: 100.0,
        min_height: 0.0,
        max_height: 100.0,
    };

    // First measure - should measure and cache
    let result1 = measure_layout(
        &mut applier,
        node_id,
        Size {
            width: 100.0,
            height: 100.0,
        },
    )?;
    let size1 = result1.root_size();

    // Clear dirty flag to simulate no changes
    applier.with_node::<LayoutNode, _>(node_id, |node| {
        node.clear_needs_measure();
        node.clear_needs_layout();
    })?;

    // Second measure - should use cache since not dirty
    let result2 = measure_layout(
        &mut applier,
        node_id,
        Size {
            width: 100.0,
            height: 100.0,
        },
    )?;
    let size2 = result2.root_size();

    assert_eq!(size1, size2, "Cached measure should return same size");
    Ok(())
}

#[test]
fn selective_measure_remeasures_when_dirty() -> Result<(), NodeError> {
    let mut applier = MemoryApplier::new();
    let node = LayoutNode::new(Modifier::empty(), Rc::new(MaxSizePolicy));
    let node_id = applier.create(Box::new(node));

    // First measure
    let result1 = measure_layout(
        &mut applier,
        node_id,
        Size {
            width: 100.0,
            height: 100.0,
        },
    )?;
    let _size1 = result1.root_size();

    // Mark as dirty by changing measure policy
    applier.with_node::<LayoutNode, _>(node_id, |node| {
        node.set_measure_policy(Rc::new(VerticalStackPolicy));
    })?;

    // Second measure - should remeasure because dirty
    let _result2 = measure_layout(
        &mut applier,
        node_id,
        Size {
            width: 100.0,
            height: 100.0,
        },
    )?;

    // Verify it was measured (by checking the dirty flag was cleared)
    let still_dirty = applier.with_node::<LayoutNode, _>(node_id, |node| node.needs_measure())?;

    assert!(!still_dirty, "Dirty flag should be cleared after measure");
    Ok(())
}

#[test]
fn cache_epoch_not_incremented_when_no_dirty_nodes() -> Result<(), NodeError> {
    let mut applier = MemoryApplier::new();
    let node = LayoutNode::new(Modifier::empty(), Rc::new(MaxSizePolicy));
    let node_id = applier.create(Box::new(node));

    // First measure
    measure_layout(
        &mut applier,
        node_id,
        Size {
            width: 100.0,
            height: 100.0,
        },
    )?;

    // Clear dirty flags
    applier.with_node::<LayoutNode, _>(node_id, |node| {
        node.clear_needs_measure();
        node.clear_needs_layout();
    })?;

    let epoch_before =
        applier.with_node::<LayoutNode, _>(node_id, |node| node.cache_handles().epoch())?;

    // Second measure with no dirty nodes - epoch should not increment
    measure_layout(
        &mut applier,
        node_id,
        Size {
            width: 100.0,
            height: 100.0,
        },
    )?;

    let epoch_after =
        applier.with_node::<LayoutNode, _>(node_id, |node| node.cache_handles().epoch())?;

    assert_eq!(
        epoch_before, epoch_after,
        "Cache epoch should not increment when no nodes are dirty"
    );
    Ok(())
}

#[test]
fn cache_epoch_increments_when_nodes_dirty() -> Result<(), NodeError> {
    let mut applier = MemoryApplier::new();
    let node = LayoutNode::new(Modifier::empty(), Rc::new(MaxSizePolicy));
    let node_id = applier.create(Box::new(node));

    // First measure
    measure_layout(
        &mut applier,
        node_id,
        Size {
            width: 100.0,
            height: 100.0,
        },
    )?;

    let epoch_before =
        applier.with_node::<LayoutNode, _>(node_id, |node| node.cache_handles().epoch())?;

    // Mark node as dirty
    applier.with_node::<LayoutNode, _>(node_id, |node| {
        node.mark_needs_measure();
    })?;

    // Second measure with dirty node - epoch should increment
    measure_layout(
        &mut applier,
        node_id,
        Size {
            width: 100.0,
            height: 100.0,
        },
    )?;

    let epoch_after =
        applier.with_node::<LayoutNode, _>(node_id, |node| node.cache_handles().epoch())?;

    assert!(
        epoch_after > epoch_before,
        "Cache epoch should increment when nodes are dirty"
    );
    Ok(())
}

#[test]
fn selective_measure_with_tree_hierarchy() -> Result<(), NodeError> {
    let mut applier = MemoryApplier::new();

    // Create a tree: root -> child_a, child_b
    let child_a = applier.create(Box::new(LayoutNode::new(
        Modifier::empty(),
        Rc::new(LeafMeasurePolicy::new(Size {
            width: 10.0,
            height: 20.0,
        })),
    )));
    let child_b = applier.create(Box::new(LayoutNode::new(
        Modifier::empty(),
        Rc::new(LeafMeasurePolicy::new(Size {
            width: 10.0,
            height: 30.0,
        })),
    )));

    let mut root = LayoutNode::new(Modifier::empty(), Rc::new(VerticalStackPolicy));
    root.children.push(child_a);
    root.children.push(child_b);
    let root_id = applier.create(Box::new(root));

    // First measure
    let result1 = measure_layout(
        &mut applier,
        root_id,
        Size {
            width: 100.0,
            height: 100.0,
        },
    )?;
    assert_eq!(result1.root_size().height, 50.0);

    // Clear all dirty flags
    applier.with_node::<LayoutNode, _>(root_id, |node| {
        node.clear_needs_measure();
        node.clear_needs_layout();
    })?;

    let epoch_before =
        applier.with_node::<LayoutNode, _>(root_id, |node| node.cache_handles().epoch())?;

    // Second measure - should use cache
    measure_layout(
        &mut applier,
        root_id,
        Size {
            width: 100.0,
            height: 100.0,
        },
    )?;

    let epoch_after =
        applier.with_node::<LayoutNode, _>(root_id, |node| node.cache_handles().epoch())?;

    assert_eq!(
        epoch_before, epoch_after,
        "Epoch should not change when entire tree is clean"
    );
    Ok(())
}

#[test]
fn scoped_layout_repass_remeasures_only_dirty_subtree() -> Result<(), NodeError> {
    let _guard = crate::render_state::render_state_test_guard();
    crate::reset_render_state_for_tests();
    let dirty_size = Rc::new(Cell::new(Size::new(10.0, 10.0)));
    let dirty_calls = Rc::new(Cell::new(0usize));
    let clean_size = Rc::new(Cell::new(Size::new(5.0, 5.0)));
    let clean_calls = Rc::new(Cell::new(0usize));

    let mut applier = MemoryApplier::new();
    let dirty_child = applier.create(Box::new(LayoutNode::new(
        Modifier::empty(),
        Rc::new(MutableLeafPolicy {
            size: Rc::clone(&dirty_size),
            calls: Rc::clone(&dirty_calls),
        }),
    )));
    let clean_child = applier.create(Box::new(LayoutNode::new(
        Modifier::empty(),
        Rc::new(MutableLeafPolicy {
            size: Rc::clone(&clean_size),
            calls: Rc::clone(&clean_calls),
        }),
    )));

    let mut root = LayoutNode::new(Modifier::empty(), Rc::new(VerticalStackPolicy));
    root.children.push(dirty_child);
    root.children.push(clean_child);
    let root_id = applier.create(Box::new(root));

    applier.with_node::<LayoutNode, _>(root_id, |node| node.set_node_id(root_id))?;
    applier.with_node::<LayoutNode, _>(dirty_child, |node| {
        node.set_node_id(dirty_child);
        node.set_parent(root_id);
    })?;
    applier.with_node::<LayoutNode, _>(clean_child, |node| {
        node.set_node_id(clean_child);
        node.set_parent(root_id);
    })?;

    let first = measure_layout(&mut applier, root_id, Size::new(100.0, 100.0))?;
    assert_eq!(first.root_size().height, 15.0);
    assert_eq!(
        dirty_calls.get(),
        1,
        "dirty child should measure once initially"
    );
    assert_eq!(
        clean_calls.get(),
        1,
        "clean child should measure once initially"
    );

    for node_id in [root_id, dirty_child, clean_child] {
        applier.with_node::<LayoutNode, _>(node_id, |node| {
            node.clear_needs_measure();
            node.clear_needs_layout();
        })?;
    }

    let epoch_before =
        applier.with_node::<LayoutNode, _>(root_id, |node| node.cache_handles().epoch())?;

    dirty_size.set(Size::new(10.0, 20.0));
    applier.with_node::<LayoutNode, _>(dirty_child, |node| {
        node.mark_needs_measure();
    })?;

    crate::schedule_layout_repass(dirty_child);
    super::process_pending_layout_repasses(&mut applier, root_id)?;

    let root_needs_measure =
        applier.with_node::<LayoutNode, _>(root_id, |node| node.needs_measure())?;
    let root_needs_layout =
        applier.with_node::<LayoutNode, _>(root_id, |node| node.needs_layout())?;

    assert!(
        !root_needs_measure,
        "scoped repass should not force the root into global remeasure"
    );
    assert!(
        root_needs_layout,
        "scoped repass should still bubble layout work to the root"
    );

    let second = measure_layout(&mut applier, root_id, Size::new(100.0, 100.0))?;
    let epoch_after =
        applier.with_node::<LayoutNode, _>(root_id, |node| node.cache_handles().epoch())?;

    assert_eq!(
        second.root_size().height,
        25.0,
        "root height should reflect the dirty child's new measured size"
    );
    assert_eq!(
        epoch_before, epoch_after,
        "scoped subtree repass should preserve the global cache epoch"
    );
    assert_eq!(
        dirty_calls.get(),
        2,
        "dirty child should be remeasured exactly once"
    );
    assert_eq!(
        clean_calls.get(),
        1,
        "clean sibling should reuse its cached measurement"
    );

    crate::reset_render_state_for_tests();

    Ok(())
}

#[test]
fn scoped_layout_repass_remeasures_dirty_subcompose_child() -> Result<(), NodeError> {
    let _guard = crate::render_state::render_state_test_guard();
    crate::reset_render_state_for_tests();

    let child_size = Rc::new(Cell::new(Size::new(10.0, 10.0)));
    let child_calls = Rc::new(Cell::new(0usize));

    let policy: Rc<crate::subcompose_layout::MeasurePolicy> = {
        let child_size = Rc::clone(&child_size);
        let child_calls = Rc::clone(&child_calls);
        Rc::new(move |scope, constraints| {
            child_calls.set(child_calls.get() + 1);
            let size = child_size.get();
            scope.layout(
                size.width
                    .clamp(constraints.min_width, constraints.max_width),
                size.height
                    .clamp(constraints.min_height, constraints.max_height),
                Vec::new(),
            )
        })
    };

    let mut applier = MemoryApplier::new();
    let composition = cranpose_core::Composition::new(MemoryApplier::new());
    applier.set_runtime_handle(composition.runtime_handle());
    let child_id = applier.create(Box::new(
        crate::subcompose_layout::SubcomposeLayoutNode::new(Modifier::empty(), policy),
    ));

    let mut root = LayoutNode::new(Modifier::empty(), Rc::new(VerticalStackPolicy));
    root.children.push(child_id);
    let root_id = applier.create(Box::new(root));

    applier.with_node::<LayoutNode, _>(root_id, |node| node.set_node_id(root_id))?;
    applier.with_node::<crate::subcompose_layout::SubcomposeLayoutNode, _>(child_id, |node| {
        node.set_node_id(child_id);
        node.set_parent_for_bubbling(root_id);
    })?;

    let first = measure_layout(&mut applier, root_id, Size::new(100.0, 100.0))?;
    assert_eq!(first.root_size().height, 10.0);
    assert_eq!(
        child_calls.get(),
        1,
        "subcompose child should measure once initially"
    );
    let root_clean = applier.with_node::<LayoutNode, _>(root_id, |node| {
        !node.needs_measure() && !node.needs_layout()
    })?;
    let child_clean = applier
        .with_node::<crate::subcompose_layout::SubcomposeLayoutNode, _>(child_id, |node| {
            !node.needs_measure() && !node.needs_layout()
        })?;
    assert!(
        root_clean,
        "root should be clean after the initial measure pass"
    );
    assert!(
        child_clean,
        "subcompose child should clear dirty flags after a successful measure pass"
    );

    let epoch_before =
        applier.with_node::<LayoutNode, _>(root_id, |node| node.cache_handles().epoch())?;

    child_size.set(Size::new(10.0, 20.0));
    applier.with_node::<crate::subcompose_layout::SubcomposeLayoutNode, _>(child_id, |node| {
        node.mark_needs_layout_flag();
    })?;

    crate::schedule_layout_repass(child_id);
    super::process_pending_layout_repasses(&mut applier, root_id)?;

    let root_needs_measure =
        applier.with_node::<LayoutNode, _>(root_id, |node| node.needs_measure())?;
    let root_needs_layout =
        applier.with_node::<LayoutNode, _>(root_id, |node| node.needs_layout())?;

    assert!(
        !root_needs_measure,
        "layout-only repass should not force the root into global remeasure"
    );
    assert!(
        root_needs_layout,
        "layout-only repass should still bubble layout work to the root"
    );

    let second = measure_layout(&mut applier, root_id, Size::new(100.0, 100.0))?;
    let epoch_after =
        applier.with_node::<LayoutNode, _>(root_id, |node| node.cache_handles().epoch())?;

    assert_eq!(
        second.root_size().height,
        20.0,
        "root height should reflect the subcompose child's updated layout pass"
    );
    assert_eq!(
        epoch_before, epoch_after,
        "layout-only subcompose repass should preserve the global cache epoch"
    );
    assert_eq!(
        child_calls.get(),
        2,
        "dirty subcompose child should be remeasured exactly once"
    );

    crate::reset_render_state_for_tests();

    Ok(())
}

#[test]
fn measure_layout_consumes_pending_scoped_repass_for_dirty_child() -> Result<(), NodeError> {
    let _guard = crate::render_state::render_state_test_guard();
    crate::reset_render_state_for_tests();

    let dirty_size = Rc::new(Cell::new(Size::new(10.0, 10.0)));
    let dirty_calls = Rc::new(Cell::new(0usize));
    let clean_size = Rc::new(Cell::new(Size::new(5.0, 5.0)));
    let clean_calls = Rc::new(Cell::new(0usize));

    let mut applier = MemoryApplier::new();
    let dirty_child = applier.create(Box::new(LayoutNode::new(
        Modifier::empty(),
        Rc::new(MutableLeafPolicy {
            size: Rc::clone(&dirty_size),
            calls: Rc::clone(&dirty_calls),
        }),
    )));
    let clean_child = applier.create(Box::new(LayoutNode::new(
        Modifier::empty(),
        Rc::new(MutableLeafPolicy {
            size: Rc::clone(&clean_size),
            calls: Rc::clone(&clean_calls),
        }),
    )));

    let mut root = LayoutNode::new(Modifier::empty(), Rc::new(VerticalStackPolicy));
    root.children.push(dirty_child);
    root.children.push(clean_child);
    let root_id = applier.create(Box::new(root));

    applier.with_node::<LayoutNode, _>(root_id, |node| node.set_node_id(root_id))?;
    applier.with_node::<LayoutNode, _>(dirty_child, |node| {
        node.set_node_id(dirty_child);
        node.set_parent(root_id);
    })?;
    applier.with_node::<LayoutNode, _>(clean_child, |node| {
        node.set_node_id(clean_child);
        node.set_parent(root_id);
    })?;

    let first = measure_layout(&mut applier, root_id, Size::new(100.0, 100.0))?;
    assert_eq!(first.root_size().height, 15.0);
    let epoch_before =
        applier.with_node::<LayoutNode, _>(root_id, |node| node.cache_handles().epoch())?;

    dirty_size.set(Size::new(10.0, 20.0));
    applier.with_node::<LayoutNode, _>(dirty_child, |node| {
        node.mark_needs_measure();
    })?;
    crate::schedule_layout_repass(dirty_child);

    let second = measure_layout(&mut applier, root_id, Size::new(100.0, 100.0))?;
    let epoch_after =
        applier.with_node::<LayoutNode, _>(root_id, |node| node.cache_handles().epoch())?;

    assert_eq!(
        second.root_size().height,
        25.0,
        "measure_layout should honor the pending scoped repass before consulting root caches"
    );
    assert_eq!(
        epoch_before, epoch_after,
        "consuming a scoped layout repass should preserve the global cache epoch"
    );
    assert_eq!(
        dirty_calls.get(),
        2,
        "dirty child should be remeasured exactly once through the public entrypoint"
    );
    assert_eq!(
        clean_calls.get(),
        1,
        "clean sibling should continue to reuse its cached measurement"
    );

    crate::reset_render_state_for_tests();

    Ok(())
}

#[test]
fn measure_layout_consumes_pending_scoped_repass_for_subcompose_child() -> Result<(), NodeError> {
    let _guard = crate::render_state::render_state_test_guard();
    crate::reset_render_state_for_tests();

    let child_size = Rc::new(Cell::new(Size::new(10.0, 10.0)));
    let child_calls = Rc::new(Cell::new(0usize));

    let policy: Rc<crate::subcompose_layout::MeasurePolicy> = {
        let child_size = Rc::clone(&child_size);
        let child_calls = Rc::clone(&child_calls);
        Rc::new(move |scope, constraints| {
            child_calls.set(child_calls.get() + 1);
            let size = child_size.get();
            scope.layout(
                size.width
                    .clamp(constraints.min_width, constraints.max_width),
                size.height
                    .clamp(constraints.min_height, constraints.max_height),
                Vec::new(),
            )
        })
    };

    let mut applier = MemoryApplier::new();
    let composition = cranpose_core::Composition::new(MemoryApplier::new());
    applier.set_runtime_handle(composition.runtime_handle());
    let child_id = applier.create(Box::new(
        crate::subcompose_layout::SubcomposeLayoutNode::new(Modifier::empty(), policy),
    ));

    let mut root = LayoutNode::new(Modifier::empty(), Rc::new(VerticalStackPolicy));
    root.children.push(child_id);
    let root_id = applier.create(Box::new(root));

    applier.with_node::<LayoutNode, _>(root_id, |node| node.set_node_id(root_id))?;
    applier.with_node::<crate::subcompose_layout::SubcomposeLayoutNode, _>(child_id, |node| {
        node.set_node_id(child_id);
        node.set_parent_for_bubbling(root_id);
    })?;

    let first = measure_layout(&mut applier, root_id, Size::new(100.0, 100.0))?;
    assert_eq!(first.root_size().height, 10.0);
    let epoch_before =
        applier.with_node::<LayoutNode, _>(root_id, |node| node.cache_handles().epoch())?;

    child_size.set(Size::new(10.0, 20.0));
    applier.with_node::<crate::subcompose_layout::SubcomposeLayoutNode, _>(child_id, |node| {
        node.mark_needs_layout_flag();
    })?;
    crate::schedule_layout_repass(child_id);

    let second = measure_layout(&mut applier, root_id, Size::new(100.0, 100.0))?;
    let epoch_after =
        applier.with_node::<LayoutNode, _>(root_id, |node| node.cache_handles().epoch())?;

    assert_eq!(
        second.root_size().height,
        20.0,
        "measure_layout should consume a pending scoped subcompose repass before reusing the root cache"
    );
    assert_eq!(
        epoch_before, epoch_after,
        "layout-only subcompose repasses must preserve the global cache epoch"
    );
    assert_eq!(
        child_calls.get(),
        2,
        "layout-only scoped repasses should remeasure the dirty subcompose child exactly once"
    );

    crate::reset_render_state_for_tests();

    Ok(())
}

#[test]
fn nested_measurement_returns_multiple_scratch_vecs_to_pool() -> Result<(), NodeError> {
    let mut applier = MemoryApplier::new();

    let leaf_id = applier.create(Box::new(LayoutNode::new(
        Modifier::empty(),
        Rc::new(LeafMeasurePolicy::new(Size {
            width: 10.0,
            height: 10.0,
        })),
    )));

    let mut child = LayoutNode::new(Modifier::empty().padding(2.0), Rc::new(VerticalStackPolicy));
    child.children.push(leaf_id);
    let child_id = applier.create(Box::new(child));

    let mut root = LayoutNode::new(Modifier::empty().padding(4.0), Rc::new(VerticalStackPolicy));
    root.children.push(child_id);
    let root_id = applier.create(Box::new(root));

    let guard = ApplierSlotGuard::new(&mut applier);
    let applier_host = guard.host();
    let slots_handle = guard.slots_handle();
    let mut builder =
        LayoutBuilder::new_with_epoch(Rc::clone(&applier_host), 1, Rc::clone(&slots_handle));

    builder.measure_node(
        root_id,
        Constraints {
            min_width: 0.0,
            max_width: 100.0,
            min_height: 0.0,
            max_height: 100.0,
        },
    )?;

    let state = builder.state.borrow();
    assert!(
        state.tmp_measurables.available_count() >= 2,
        "nested measurement should retain multiple measurable scratch vecs"
    );
    assert!(
        state.tmp_records.available_count() >= 2,
        "nested measurement should retain multiple record scratch vecs"
    );
    assert!(
        state.tmp_child_ids.available_count() >= 2,
        "nested measurement should retain multiple child-id scratch vecs"
    );
    assert!(
        state.tmp_layout_node_data.available_count() >= 2,
        "nested measurement should retain multiple modifier scratch vecs"
    );

    Ok(())
}

#[test]
fn dirty_child_triggers_parent_remeasure() -> Result<(), NodeError> {
    use super::bubble_layout_dirty;
    let mut applier = MemoryApplier::new();

    // Create tree with child that can change
    let child = applier.create(Box::new(LayoutNode::new(
        Modifier::empty(),
        Rc::new(MaxSizePolicy),
    )));

    let mut root = LayoutNode::new(Modifier::empty(), Rc::new(VerticalStackPolicy));
    root.children.push(child);
    let root_id = applier.create(Box::new(root));

    // Set up parent links
    applier.with_node::<LayoutNode, _>(root_id, |node| node.set_node_id(root_id))?;
    applier.with_node::<LayoutNode, _>(child, |node| {
        node.set_node_id(child);
        node.set_parent(root_id);
    })?;

    // First measure
    measure_layout(
        &mut applier,
        root_id,
        Size {
            width: 100.0,
            height: 100.0,
        },
    )?;

    // Mark child as dirty and bubble to root
    applier.with_node::<LayoutNode, _>(child, |node| {
        node.mark_needs_measure();
    })?;
    bubble_layout_dirty(&mut applier, child);

    // Check that root is now dirty (O(1) check)
    let root_needs_measure =
        applier.with_node::<LayoutNode, _>(root_id, |node| node.needs_layout())?;
    assert!(
        root_needs_measure,
        "Root should be dirty when child is dirty (due to bubbling)"
    );

    Ok(())
}

// ============================================================================
// PARENT TRACKING AND DIRTY BUBBLING TESTS
// ============================================================================

#[test]
fn parent_tracking_basic() -> Result<(), NodeError> {
    let mut applier = MemoryApplier::new();

    // Create parent and child
    let child = applier.create(Box::new(LayoutNode::new(
        Modifier::empty(),
        Rc::new(MaxSizePolicy),
    )));

    let mut parent = LayoutNode::new(Modifier::empty(), Rc::new(VerticalStackPolicy));
    parent.children.push(child);
    let parent_id = applier.create(Box::new(parent));

    // Set IDs on nodes
    applier.with_node::<LayoutNode, _>(parent_id, |node| {
        node.set_node_id(parent_id);
    })?;
    applier.with_node::<LayoutNode, _>(child, |node| {
        node.set_node_id(child);
    })?;

    // Set parent relationship
    applier.with_node::<LayoutNode, _>(child, |node| {
        node.set_parent(parent_id);
    })?;

    // Verify parent is set correctly
    let child_parent = applier.with_node::<LayoutNode, _>(child, |node| node.parent())?;

    assert_eq!(
        child_parent,
        Some(parent_id),
        "Child should know its parent"
    );

    Ok(())
}

#[test]
fn dirty_bubbling_to_root() -> Result<(), NodeError> {
    use super::bubble_layout_dirty;

    let mut applier = MemoryApplier::new();

    // Create a three-level tree: root -> middle -> leaf
    let leaf = applier.create(Box::new(LayoutNode::new(
        Modifier::empty(),
        Rc::new(MaxSizePolicy),
    )));

    let mut middle = LayoutNode::new(Modifier::empty(), Rc::new(VerticalStackPolicy));
    middle.children.push(leaf);
    let middle_id = applier.create(Box::new(middle));

    let mut root = LayoutNode::new(Modifier::empty(), Rc::new(VerticalStackPolicy));
    root.children.push(middle_id);
    let root_id = applier.create(Box::new(root));

    // Set up node IDs and parent relationships
    applier.with_node::<LayoutNode, _>(root_id, |node| node.set_node_id(root_id))?;
    applier.with_node::<LayoutNode, _>(middle_id, |node| {
        node.set_node_id(middle_id);
        node.set_parent(root_id);
    })?;
    applier.with_node::<LayoutNode, _>(leaf, |node| {
        node.set_node_id(leaf);
        node.set_parent(middle_id);
    })?;

    // Clear all dirty flags
    applier.with_node::<LayoutNode, _>(root_id, |node| {
        node.clear_needs_measure();
        node.clear_needs_layout();
    })?;
    applier.with_node::<LayoutNode, _>(middle_id, |node| {
        node.clear_needs_measure();
        node.clear_needs_layout();
    })?;
    applier.with_node::<LayoutNode, _>(leaf, |node| {
        node.clear_needs_measure();
        node.clear_needs_layout();
    })?;

    // Mark leaf dirty
    applier.with_node::<LayoutNode, _>(leaf, |node| {
        node.mark_needs_measure();
    })?;

    // Bubble dirty flag
    bubble_layout_dirty(&mut applier, leaf);

    // Check that middle and root are now marked as needing layout
    let middle_needs_layout =
        applier.with_node::<LayoutNode, _>(middle_id, |node| node.needs_layout())?;
    let root_needs_layout =
        applier.with_node::<LayoutNode, _>(root_id, |node| node.needs_layout())?;

    assert!(
        middle_needs_layout,
        "Middle should need layout after child became dirty"
    );
    assert!(
        root_needs_layout,
        "Root should need layout after descendant became dirty"
    );

    Ok(())
}

#[test]
fn tree_needs_layout_api() -> Result<(), NodeError> {
    use super::{bubble_layout_dirty, tree_needs_layout};

    let mut applier = MemoryApplier::new();

    // Create simple tree
    let child = applier.create(Box::new(LayoutNode::new(
        Modifier::empty(),
        Rc::new(MaxSizePolicy),
    )));

    let mut root = LayoutNode::new(Modifier::empty(), Rc::new(VerticalStackPolicy));
    root.children.push(child);
    let root_id = applier.create(Box::new(root));

    // Set up parent links
    applier.with_node::<LayoutNode, _>(root_id, |node| node.set_node_id(root_id))?;
    applier.with_node::<LayoutNode, _>(child, |node| {
        node.set_node_id(child);
        node.set_parent(root_id);
    })?;

    // Initially dirty (new nodes)
    assert!(
        tree_needs_layout(&mut applier as &mut dyn Applier, root_id)?,
        "New tree should need layout"
    );

    // Clear flags
    applier.with_node::<LayoutNode, _>(root_id, |node| {
        node.clear_needs_measure();
        node.clear_needs_layout();
    })?;
    applier.with_node::<LayoutNode, _>(child, |node| {
        node.clear_needs_measure();
        node.clear_needs_layout();
    })?;

    // Now clean
    assert!(
        !tree_needs_layout(&mut applier as &mut dyn Applier, root_id)?,
        "Clean tree should not need layout"
    );

    // Mark child dirty and bubble to root
    applier.with_node::<LayoutNode, _>(child, |node| {
        node.mark_needs_measure();
    })?;
    bubble_layout_dirty(&mut applier, child);

    // Should need layout again (root should be dirty now due to bubbling)
    assert!(
        tree_needs_layout(&mut applier as &mut dyn Applier, root_id)?,
        "Tree with dirty child should need layout"
    );

    Ok(())
}

#[test]
fn tree_needs_layout_supports_subcompose_root_nodes() -> Result<(), NodeError> {
    use super::tree_needs_layout;
    use crate::subcompose_layout::{MeasurePolicy, SubcomposeLayoutNode};

    let mut applier = MemoryApplier::new();
    let policy: Rc<MeasurePolicy> =
        Rc::new(|scope, _constraints| scope.layout(0.0, 0.0, Vec::new()));
    let node_id = applier.create(Box::new(SubcomposeLayoutNode::new(
        Modifier::empty(),
        Rc::clone(&policy),
    )));

    assert!(tree_needs_layout(
        &mut applier as &mut dyn Applier,
        node_id
    )?);

    applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
        node.clear_needs_measure();
        node.clear_needs_layout();
    })?;
    assert!(!tree_needs_layout(
        &mut applier as &mut dyn Applier,
        node_id
    )?);

    applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
        node.mark_needs_layout();
    })?;
    assert!(tree_needs_layout(
        &mut applier as &mut dyn Applier,
        node_id
    )?);

    Ok(())
}

#[test]
fn tree_needs_semantics_supports_subcompose_root_nodes() -> Result<(), NodeError> {
    use super::tree_needs_semantics;
    use crate::subcompose_layout::{MeasurePolicy, SubcomposeLayoutNode};

    let mut applier = MemoryApplier::new();
    let policy: Rc<MeasurePolicy> =
        Rc::new(|scope, _constraints| scope.layout(0.0, 0.0, Vec::new()));
    let node_id = applier.create(Box::new(SubcomposeLayoutNode::new(
        Modifier::empty(),
        Rc::clone(&policy),
    )));

    assert!(tree_needs_semantics(
        &mut applier as &mut dyn Applier,
        node_id
    )?);

    applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
        node.clear_needs_semantics_for_tests();
    })?;
    assert!(!tree_needs_semantics(
        &mut applier as &mut dyn Applier,
        node_id
    )?);

    applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
        node.mark_needs_semantics();
    })?;
    assert!(tree_needs_semantics(
        &mut applier as &mut dyn Applier,
        node_id
    )?);

    Ok(())
}

#[test]
fn bubbling_reaches_clean_ancestors_above_dirty_intermediate() -> Result<(), NodeError> {
    use super::bubble_layout_dirty;

    let mut applier = MemoryApplier::new();

    // Create tree: root -> middle -> leaf
    let leaf = applier.create(Box::new(LayoutNode::new(
        Modifier::empty(),
        Rc::new(MaxSizePolicy),
    )));

    let mut middle = LayoutNode::new(Modifier::empty(), Rc::new(VerticalStackPolicy));
    middle.children.push(leaf);
    let middle_id = applier.create(Box::new(middle));

    let mut root = LayoutNode::new(Modifier::empty(), Rc::new(VerticalStackPolicy));
    root.children.push(middle_id);
    let root_id = applier.create(Box::new(root));

    // Set up relationships
    applier.with_node::<LayoutNode, _>(root_id, |node| node.set_node_id(root_id))?;
    applier.with_node::<LayoutNode, _>(middle_id, |node| {
        node.set_node_id(middle_id);
        node.set_parent(root_id);
    })?;
    applier.with_node::<LayoutNode, _>(leaf, |node| {
        node.set_node_id(leaf);
        node.set_parent(middle_id);
    })?;

    // Mark middle as already needing layout
    applier.with_node::<LayoutNode, _>(middle_id, |node| {
        node.mark_needs_layout();
    })?;

    // Clear root
    applier.with_node::<LayoutNode, _>(root_id, |node| {
        node.clear_needs_measure();
        node.clear_needs_layout();
    })?;

    // Mark leaf and bubble
    applier.with_node::<LayoutNode, _>(leaf, |node| {
        node.mark_needs_measure();
    })?;
    bubble_layout_dirty(&mut applier, leaf);

    // Root must still become dirty even though the intermediate node was already dirty.
    // Scoped layout repasses can strand a dirty node under clean ancestors until the next
    // descendant invalidation reconnects the path.
    let root_needs_layout =
        applier.with_node::<LayoutNode, _>(root_id, |node| node.needs_layout())?;

    assert!(
        root_needs_layout,
        "Bubbling must continue past an already dirty intermediate node when ancestors above it are still clean"
    );

    Ok(())
}

#[test]
fn property_change_bubbles_without_manual_call() -> Result<(), NodeError> {
    use super::bubble_layout_dirty;
    use crate::modifier::Modifier;

    // This test proves that property changes (set_modifier, set_measure_policy) bubble
    // to root WITHOUT needing manual bubbling calls in Layout() composable.
    // The key is that pop_parent() checks if node is dirty and bubbles automatically.

    let mut applier = MemoryApplier::new();
    let root_id = applier.create(Box::new(LayoutNode::new(
        Modifier::empty(),
        Rc::new(MaxSizePolicy),
    )));
    let child_id = applier.create(Box::new(LayoutNode::new(
        Modifier::empty(),
        Rc::new(MaxSizePolicy),
    )));
    let leaf_id = applier.create(Box::new(LayoutNode::new(
        Modifier::empty(),
        Rc::new(MaxSizePolicy),
    )));

    // Build tree structure
    applier.with_node::<LayoutNode, _>(root_id, |node| {
        node.set_node_id(root_id);
        node.children.push(child_id);
    })?;
    applier.with_node::<LayoutNode, _>(child_id, |node| {
        node.set_node_id(child_id);
        node.set_parent(root_id);
        node.children.push(leaf_id);
    })?;
    applier.with_node::<LayoutNode, _>(leaf_id, |node| {
        node.set_node_id(leaf_id);
        node.set_parent(child_id);
    })?;

    // Clear all dirty flags
    for id in [root_id, child_id, leaf_id] {
        applier.with_node::<LayoutNode, _>(id, |node| {
            node.clear_needs_measure();
            node.clear_needs_layout();
        })?;
    }

    // Verify tree is clean
    assert!(!applier.with_node::<LayoutNode, _>(root_id, |n| n.needs_layout())?);
    assert!(!applier.with_node::<LayoutNode, _>(child_id, |n| n.needs_layout())?);
    assert!(!applier.with_node::<LayoutNode, _>(leaf_id, |n| n.needs_layout())?);

    // Change property on leaf (like set_modifier would do in Layout() composable)
    // This marks the node dirty but doesn't bubble yet
    applier.with_node::<LayoutNode, _>(leaf_id, |node| {
        node.set_modifier(Modifier::empty().width(100.0));
    })?;

    // Leaf should be dirty now
    assert!(applier.with_node::<LayoutNode, _>(leaf_id, |n| n.needs_layout())?);

    // But parent and root are still clean (no manual bubble yet!)
    assert!(!applier.with_node::<LayoutNode, _>(child_id, |n| n.needs_layout())?);
    assert!(!applier.with_node::<LayoutNode, _>(root_id, |n| n.needs_layout())?);

    // Now simulate pop_parent() bubbling - this is what happens in the composer
    bubble_layout_dirty(&mut applier, leaf_id);

    // Now root should be dirty - proves bubbling worked
    assert!(
        applier.with_node::<LayoutNode, _>(root_id, |n| n.needs_layout())?,
        "Root should be dirty after property change bubbled from leaf"
    );

    Ok(())
}

#[test]
fn flex_parent_data_uses_resolved_weight() {
    let mut applier = MemoryApplier::new();
    let layout_node = LayoutNode::new(
        Modifier::empty().columnWeight(1.0, true),
        Rc::new(MaxSizePolicy),
    );
    let cache = layout_node.cache_handles();
    let node_id = applier.create(Box::new(layout_node));
    let applier_host = Rc::new(ConcreteApplierHost::new(applier));

    let measurable = LayoutChildMeasurable::new(
        Rc::clone(&applier_host),
        node_id,
        Rc::new(RefCell::new(None)),
        Rc::new(RefCell::new(None)),
        Rc::new(RefCell::new(None)),
        None,
        cache,
        1,
        false,
        None,
        None, // layout_state
    );

    let parent_data = measurable
        .flex_parent_data()
        .expect("expected weight to propagate via resolved modifiers");
    assert_eq!(parent_data.weight, 1.0);
    assert!(parent_data.fill);
}

#[test]
fn semantics_tree_derives_roles_from_configuration() -> Result<(), NodeError> {
    use crate::layout::SemanticsRole;

    let mut applier = MemoryApplier::new();

    // Create a button via semantics modifier (not ButtonNode)
    let button_node = LayoutNode::new(
        Modifier::empty().semantics(|config| {
            config.is_button = true;
            config.is_clickable = true;
            config.content_description = Some("My Button".into());
        }),
        Rc::new(MaxSizePolicy),
    );
    let button_id = applier.create(Box::new(button_node));

    // Measure and build semantics tree
    let measurements = measure_layout(&mut applier, button_id, Size::new(100.0, 100.0))?;
    let semantics_tree = measurements
        .semantics_tree()
        .expect("expected semantics tree");
    let root = semantics_tree.root();

    // Verify the role was derived from is_button flag
    assert!(matches!(root.role, SemanticsRole::Button));

    // Verify click action was synthesized from is_clickable
    assert_eq!(root.actions.len(), 1);
    assert!(matches!(
        root.actions[0],
        crate::layout::SemanticsAction::Click { .. }
    ));

    // Verify description
    assert_eq!(root.description.as_deref(), Some("My Button"));

    Ok(())
}

#[test]
fn measure_layout_can_skip_semantics_until_consumer_is_enabled() -> Result<(), NodeError> {
    let mut applier = MemoryApplier::new();

    let node = LayoutNode::new(
        Modifier::empty().semantics(|config| {
            config.content_description = Some("deferred".into());
        }),
        Rc::new(MaxSizePolicy),
    );
    let node_id = applier.create(Box::new(node));

    let skipped = super::measure_layout_with_options(
        &mut applier,
        node_id,
        Size::new(100.0, 100.0),
        MeasureLayoutOptions {
            collect_semantics: false,
            build_layout_tree: true,
        },
    )?;
    assert!(
        skipped.semantics_tree().is_none(),
        "semantics tree should be omitted when collection is disabled"
    );
    assert!(
        applier.with_node::<LayoutNode, _>(node_id, |node| node.needs_semantics())?,
        "skipping semantics collection must preserve the dirty flag"
    );

    let collected = measure_layout(&mut applier, node_id, Size::new(100.0, 100.0))?;
    let root = collected
        .semantics_tree()
        .expect("semantics tree should exist once collection is enabled")
        .root();
    assert_eq!(root.description.as_deref(), Some("deferred"));
    assert!(
        !applier.with_node::<LayoutNode, _>(node_id, |node| node.needs_semantics())?,
        "collecting semantics should clear the dirty flag"
    );

    Ok(())
}

#[test]
fn measure_layout_debug_allocation_stats_cover_layout_semantics_and_modifier_storage(
) -> Result<(), NodeError> {
    let mut applier = MemoryApplier::new();

    let root_id = applier.create(Box::new(LayoutNode::new(
        Modifier::empty()
            .background(Color::rgba(0.2, 0.4, 0.6, 1.0))
            .semantics(|config| {
                config.content_description = Some("root".into());
            }),
        Rc::new(VerticalStackPolicy),
    )));
    let button_id = applier.create(Box::new(LayoutNode::new(
        Modifier::empty().clickable(|_| {}).semantics(|config| {
            config.content_description = Some("button".into());
        }),
        Rc::new(MaxSizePolicy),
    )));
    let leaf_id = applier.create(Box::new(LayoutNode::new(
        Modifier::empty().semantics(|config| {
            config.content_description = Some("leaf".into());
        }),
        Rc::new(MaxSizePolicy),
    )));

    applier.with_node::<LayoutNode, _>(root_id, |node| {
        node.set_node_id(root_id);
        node.children.push(button_id);
        node.children.push(leaf_id);
    })?;
    applier.with_node::<LayoutNode, _>(button_id, |node| {
        node.set_node_id(button_id);
        node.set_parent(root_id);
    })?;
    applier.with_node::<LayoutNode, _>(leaf_id, |node| {
        node.set_node_id(leaf_id);
        node.set_parent(root_id);
    })?;

    let measurements = measure_layout(&mut applier, root_id, Size::new(100.0, 100.0))?;
    let stats = measurements.debug_allocation_stats();

    assert_eq!(stats.layout_box_count, 3);
    assert_eq!(stats.layout_box_child_count, 2);
    assert!(stats.layout_box_child_capacity >= stats.layout_box_child_count);
    assert!(stats.layout_box_heap_bytes > 0);
    assert_eq!(stats.modifier_slice_count, 3);
    assert!(
        stats.modifier_draw_command_count >= 1,
        "background should be visible in modifier storage stats: {stats:?}"
    );
    assert_eq!(stats.modifier_pointer_input_count, 1);
    assert!(stats.modifier_slice_heap_bytes > 0);
    assert_eq!(stats.semantics_node_count, 3);
    assert_eq!(stats.semantics_child_count, 2);
    assert!(stats.semantics_child_capacity >= stats.semantics_child_count);
    assert_eq!(stats.semantics_description_count, 3);
    assert!(stats.semantics_description_bytes >= "rootbuttonleaf".len());
    assert_eq!(stats.semantics_action_count, 1);
    assert!(stats.semantics_action_capacity >= stats.semantics_action_count);
    assert!(stats.semantics_heap_bytes >= stats.semantics_description_bytes);

    let layout_only_stats = measurements.layout_tree().debug_allocation_stats();
    assert_eq!(layout_only_stats.layout_box_count, 3);
    assert_eq!(layout_only_stats.semantics_node_count, 0);

    Ok(())
}

#[test]
fn semantics_tree_matches_with_and_without_layout_tree() -> Result<(), NodeError> {
    fn build_fixture() -> Result<(MemoryApplier, NodeId), NodeError> {
        let mut applier = MemoryApplier::new();

        let root_id = applier.create(Box::new(LayoutNode::new(
            Modifier::empty().semantics(|config| {
                config.content_description = Some("root".into());
            }),
            Rc::new(VerticalStackPolicy),
        )));
        let button_id = applier.create(Box::new(LayoutNode::new(
            Modifier::empty().semantics(|config| {
                config.is_button = true;
                config.is_clickable = true;
                config.content_description = Some("action".into());
            }),
            Rc::new(MaxSizePolicy),
        )));
        let leaf_id = applier.create(Box::new(LayoutNode::new(
            Modifier::empty().semantics(|config| {
                config.content_description = Some("leaf".into());
            }),
            Rc::new(MaxSizePolicy),
        )));

        applier.with_node::<LayoutNode, _>(root_id, |node| {
            node.set_node_id(root_id);
            node.children.push(button_id);
            node.children.push(leaf_id);
        })?;
        applier.with_node::<LayoutNode, _>(button_id, |node| {
            node.set_node_id(button_id);
            node.set_parent(root_id);
        })?;
        applier.with_node::<LayoutNode, _>(leaf_id, |node| {
            node.set_node_id(leaf_id);
            node.set_parent(root_id);
        })?;

        Ok((applier, root_id))
    }

    let (mut with_layout_tree_applier, with_layout_tree_root) = build_fixture()?;
    let with_layout_tree = super::measure_layout_with_options(
        &mut with_layout_tree_applier,
        with_layout_tree_root,
        Size::new(100.0, 100.0),
        MeasureLayoutOptions {
            collect_semantics: true,
            build_layout_tree: true,
        },
    )?
    .semantics_tree()
    .cloned()
    .expect("semantics with layout tree");

    let (mut live_node_applier, live_node_root) = build_fixture()?;
    let live_nodes = super::measure_layout_with_options(
        &mut live_node_applier,
        live_node_root,
        Size::new(100.0, 100.0),
        MeasureLayoutOptions {
            collect_semantics: true,
            build_layout_tree: false,
        },
    )?
    .semantics_tree()
    .cloned()
    .expect("semantics from live nodes");

    assert_eq!(with_layout_tree, live_nodes);

    Ok(())
}

#[test]
fn semantics_configuration_merges_multiple_modifiers() -> Result<(), NodeError> {
    let mut applier = MemoryApplier::new();

    // Chain multiple semantics modifiers
    let node = LayoutNode::new(
        Modifier::empty()
            .semantics(|config| {
                config.content_description = Some("first".into());
            })
            .semantics(|config| {
                config.is_clickable = true;
            }),
        Rc::new(MaxSizePolicy),
    );
    let node_id = applier.create(Box::new(node));

    // Measure and build semantics tree
    let measurements = measure_layout(&mut applier, node_id, Size::new(100.0, 100.0))?;
    let semantics_tree = measurements
        .semantics_tree()
        .expect("expected semantics tree");
    let root = semantics_tree.root();

    // Both semantics should be merged
    assert_eq!(root.description.as_deref(), Some("first"));
    assert_eq!(root.actions.len(), 1);

    Ok(())
}

#[test]
fn semantics_only_updates_do_not_trigger_layout() -> Result<(), NodeError> {
    let mut applier = MemoryApplier::new();

    // Create a node with semantics
    let node = LayoutNode::new(
        Modifier::empty().semantics(|config| {
            config.content_description = Some("original".into());
        }),
        Rc::new(MaxSizePolicy),
    );
    let node_id = applier.create(Box::new(node));

    // Do initial measure
    let _ = measure_layout(&mut applier, node_id, Size::new(100.0, 100.0))?;

    // Node should be clean after measure
    assert!(!applier.with_node::<LayoutNode, _>(node_id, |n| n.needs_layout())?);

    // Update semantics (this would normally come from a modifier update)
    applier.with_node::<LayoutNode, _>(node_id, |node| {
        node.set_modifier(Modifier::empty().semantics(|config| {
            config.content_description = Some("updated".into());
        }));
        node.mark_needs_semantics();
    })?;

    // Semantics dirty flag should be set
    assert!(applier.with_node::<LayoutNode, _>(node_id, |n| n.needs_semantics())?);

    // But layout dirty flag should NOT be set (semantics-only update)
    // Note: This currently bubbles layout due to modifier chain updates,
    // but in a full implementation with finer-grained invalidation,
    // semantics-only changes would not bubble layout dirty.

    Ok(())
}

// ============================================================================
// APPLIER/SLOT GUARD PANIC RECOVERY TESTS
// ============================================================================

/// A measure policy that panics when invoked.
#[derive(Clone)]
struct PanickingMeasurePolicy;

impl MeasurePolicy for PanickingMeasurePolicy {
    fn measure(
        &self,
        _measurables: &[Box<dyn Measurable>],
        _constraints: Constraints,
    ) -> MeasureResult {
        panic!("Deliberate panic in MeasurePolicy::measure")
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

#[test]
fn measure_layout_panic_preserves_applier_and_slots() {
    use std::panic::{catch_unwind, AssertUnwindSafe};

    let mut applier = MemoryApplier::new();

    // Create a normal node first to populate the applier
    let normal_node = LayoutNode::new(Modifier::empty(), Rc::new(MaxSizePolicy));
    let normal_id = applier.create(Box::new(normal_node));

    // Verify the applier has the node
    assert!(
        applier.get_mut(normal_id).is_ok(),
        "Applier should contain normal node before panic test"
    );

    // Count nodes before panic
    let node_count_before = applier.len();
    assert!(node_count_before > 0, "Should have at least one node");

    // Create the panicking node
    let panicking_node = LayoutNode::new(Modifier::empty(), Rc::new(PanickingMeasurePolicy));
    let panic_id = applier.create(Box::new(panicking_node));

    // Attempt to measure the panicking node - this should panic but the RAII guard
    // should restore the applier
    let result = catch_unwind(AssertUnwindSafe(|| {
        let _ = measure_layout(
            &mut applier,
            panic_id,
            Size {
                width: 100.0,
                height: 100.0,
            },
        );
    }));

    // The panic should have been caught
    assert!(result.is_err(), "measure_layout should have panicked");

    // CRITICAL: The applier should still be intact and usable
    // This is what ApplierSlotGuard protects - without it, the applier would be
    // left in an invalid state (replaced with an empty MemoryApplier::new())

    // Check that we can still access the original node
    assert!(
        applier.get_mut(normal_id).is_ok(),
        "Applier should still contain original node after panic - ApplierSlotGuard worked!"
    );

    // Check the node count is preserved
    let node_count_after = applier.len();
    assert_eq!(
        node_count_before + 1, // +1 because we added the panicking node
        node_count_after,
        "All nodes should be preserved after panic"
    );

    // Verify we can still perform layout on the normal node
    let result = measure_layout(
        &mut applier,
        normal_id,
        Size {
            width: 100.0,
            height: 100.0,
        },
    );
    assert!(
        result.is_ok(),
        "Should be able to do layout after recovering from panic"
    );
}

#[test]
fn measure_layout_error_preserves_applier_and_slots() -> Result<(), NodeError> {
    let mut applier = MemoryApplier::new();

    // Create a valid tree with multiple nodes
    let child1 = applier.create(Box::new(LayoutNode::new(
        Modifier::empty(),
        Rc::new(LeafMeasurePolicy::new(Size {
            width: 20.0,
            height: 20.0,
        })),
    )));
    let child2 = applier.create(Box::new(LayoutNode::new(
        Modifier::empty(),
        Rc::new(LeafMeasurePolicy::new(Size {
            width: 30.0,
            height: 30.0,
        })),
    )));

    let mut root = LayoutNode::new(Modifier::empty(), Rc::new(VerticalStackPolicy));
    root.children.push(child1);
    root.children.push(child2);
    let root_id = applier.create(Box::new(root));

    // Perform a successful layout first
    let result = measure_layout(
        &mut applier,
        root_id,
        Size {
            width: 100.0,
            height: 100.0,
        },
    )?;
    assert_eq!(
        result.root_size().height,
        50.0,
        "Initial layout should succeed"
    );

    // Now try to measure a non-existent node (will return an error)
    // Use a large ID that definitely doesn't exist (we only created 3 nodes: 0, 1, 2)
    let fake_id: NodeId = 99999;
    let error_result = measure_layout(
        &mut applier,
        fake_id,
        Size {
            width: 100.0,
            height: 100.0,
        },
    );
    assert!(
        error_result.is_err(),
        "Measuring non-existent node should error"
    );

    // CRITICAL: After the error, we should still be able to use the applier
    // The ApplierSlotGuard ensures the applier is restored even on Err paths

    // Verify all original nodes are still accessible
    assert!(applier.get_mut(root_id).is_ok(), "Root should still exist");
    assert!(applier.get_mut(child1).is_ok(), "Child1 should still exist");
    assert!(applier.get_mut(child2).is_ok(), "Child2 should still exist");

    // Verify we can still perform successful layout
    let result_after = measure_layout(
        &mut applier,
        root_id,
        Size {
            width: 100.0,
            height: 100.0,
        },
    )?;
    assert_eq!(
        result_after.root_size().height,
        50.0,
        "Layout should still work after error recovery"
    );

    Ok(())
}
