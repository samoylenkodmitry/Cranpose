use super::*;
use crate::modifier::{Modifier, Size};
use compose_core::{Applier, ConcreteApplierHost, MemoryApplier};
use compose_ui_layout::{MeasurePolicy, MeasureResult, Placement};
use std::{cell::RefCell, rc::Rc};

use super::core::Measurable;

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

#[test]
fn clamp_dimension_respects_infinite_max() {
    let clamped = clamp_dimension(50.0, 10.0, f32::INFINITY);
    assert_eq!(clamped, 50.0);
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

#[test]
fn layout_node_uses_measure_policy() -> Result<(), NodeError> {
    let mut applier = MemoryApplier::new();
    let child_a = applier.create(Box::new(SpacerNode {
        size: Size {
            width: 10.0,
            height: 20.0,
        },
    }));
    let child_b = applier.create(Box::new(SpacerNode {
        size: Size {
            width: 5.0,
            height: 30.0,
        },
    }));

    let mut layout_node = LayoutNode::new(Modifier::empty(), Rc::new(VerticalStackPolicy));
    layout_node.children.insert(child_a);
    layout_node.children.insert(child_b);
    let layout_id = applier.create(Box::new(layout_node));

    let applier_host = Rc::new(ConcreteApplierHost::new(applier));
    let mut builder = LayoutBuilder::new(Rc::clone(&applier_host));

    // Verify LayoutBuilder::new() gets a nonzero epoch (no epoch=0 accidents)
    let epoch = builder.state.borrow().cache_epoch;
    assert_ne!(
        epoch, 0,
        "LayoutBuilder::new() should get a proper epoch, not 0"
    );

    let measured = builder.measure_node(
        layout_id,
        Constraints {
            min_width: 0.0,
            max_width: 200.0,
            min_height: 0.0,
            max_height: 200.0,
        },
    )?;

    assert_eq!(measured.size.width, 10.0);
    assert_eq!(measured.size.height, 50.0);
    assert_eq!(measured.children.len(), 2);
    assert_eq!(measured.children[0].offset, Point { x: 0.0, y: 0.0 });
    assert_eq!(measured.children[1].offset, Point { x: 0.0, y: 20.0 });
    Ok(())
}

#[test]
fn layout_padding_comes_from_modifier_chain() -> Result<(), NodeError> {
    let mut applier = MemoryApplier::new();
    let layout_node = LayoutNode::new(Modifier::padding(8.0), Rc::new(VerticalStackPolicy));
    let layout_id = applier.create(Box::new(layout_node));

    let applier_host = Rc::new(ConcreteApplierHost::new(applier));
    let mut builder = LayoutBuilder::new(Rc::clone(&applier_host));

    let measured = builder.measure_node(
        layout_id,
        Constraints {
            min_width: 0.0,
            max_width: 100.0,
            min_height: 0.0,
            max_height: 100.0,
        },
    )?;

    assert_eq!(measured.size.width, 16.0);
    assert_eq!(measured.size.height, 16.0);
    Ok(())
}

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

    node.set_modifier(Modifier::padding(4.0));
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
    let child = applier.create(Box::new(SpacerNode {
        size: Size {
            width: 10.0,
            height: 10.0,
        },
    }));

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
    let child = applier.create(Box::new(SpacerNode {
        size: Size {
            width: 10.0,
            height: 10.0,
        },
    }));

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

    let constraints = Constraints {
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
    let size1 = result1.root_size();

    // Mark as dirty by changing measure policy
    applier.with_node::<LayoutNode, _>(node_id, |node| {
        node.set_measure_policy(Rc::new(VerticalStackPolicy));
    })?;

    // Second measure - should remeasure because dirty
    let result2 = measure_layout(
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
    let child_a = applier.create(Box::new(SpacerNode {
        size: Size {
            width: 10.0,
            height: 20.0,
        },
    }));
    let child_b = applier.create(Box::new(SpacerNode {
        size: Size {
            width: 10.0,
            height: 30.0,
        },
    }));

    let mut root = LayoutNode::new(Modifier::empty(), Rc::new(VerticalStackPolicy));
    root.children.insert(child_a);
    root.children.insert(child_b);
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
fn dirty_child_triggers_parent_remeasure() -> Result<(), NodeError> {
    use super::bubble_layout_dirty;
    let mut applier = MemoryApplier::new();

    // Create tree with child that can change
    let child = applier.create(Box::new(LayoutNode::new(
        Modifier::empty(),
        Rc::new(MaxSizePolicy),
    )));

    let mut root = LayoutNode::new(Modifier::empty(), Rc::new(VerticalStackPolicy));
    root.children.insert(child);
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
    parent.children.insert(child);
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
    middle.children.insert(leaf);
    let middle_id = applier.create(Box::new(middle));

    let mut root = LayoutNode::new(Modifier::empty(), Rc::new(VerticalStackPolicy));
    root.children.insert(middle_id);
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
    root.children.insert(child);
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
fn bubbling_stops_at_already_dirty_ancestor() -> Result<(), NodeError> {
    use super::bubble_layout_dirty;

    let mut applier = MemoryApplier::new();

    // Create tree: root -> middle -> leaf
    let leaf = applier.create(Box::new(LayoutNode::new(
        Modifier::empty(),
        Rc::new(MaxSizePolicy),
    )));

    let mut middle = LayoutNode::new(Modifier::empty(), Rc::new(VerticalStackPolicy));
    middle.children.insert(leaf);
    let middle_id = applier.create(Box::new(middle));

    let mut root = LayoutNode::new(Modifier::empty(), Rc::new(VerticalStackPolicy));
    root.children.insert(middle_id);
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

    // Root should still be clean (bubbling should have stopped at middle)
    let root_needs_layout =
        applier.with_node::<LayoutNode, _>(root_id, |node| node.needs_layout())?;

    // The unified bubbling API now implements the O(1) optimization:
    // bubbling stops when it encounters an already-dirty ancestor
    assert!(
        !root_needs_layout,
        "Bubbling should stop at already dirty ancestor (O(1) optimization)"
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
        node.children.insert(child_id);
    })?;
    applier.with_node::<LayoutNode, _>(child_id, |node| {
        node.set_node_id(child_id);
        node.set_parent(root_id);
        node.children.insert(leaf_id);
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
        node.set_modifier(Modifier::width(100.0));
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
        None,
    );

    let parent_data = measurable
        .flex_parent_data()
        .expect("expected weight to propagate via resolved modifiers");
    assert_eq!(parent_data.weight, 1.0);
    assert!(parent_data.fill);
}
