use super::*;
use compose_core::Applier;
use std::rc::Rc;

use crate::modifier::{Modifier, Size};
use compose_ui_layout::{MeasurePolicy, MeasureResult, Placement};

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

#[derive(Clone)]
struct CountingPolicy {
    counter: Rc<std::cell::Cell<usize>>,
}

impl CountingPolicy {
    fn new(counter: Rc<std::cell::Cell<usize>>) -> Self {
        Self { counter }
    }
}

impl MeasurePolicy for CountingPolicy {
    fn measure(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        self.counter.set(self.counter.get().saturating_add(1));
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
fn selective_measure_reuses_clean_subtrees() -> Result<(), NodeError> {
    let mut applier = MemoryApplier::new();

    let spacer_a = applier.create(Box::new(SpacerNode {
        size: Size {
            width: 10.0,
            height: 20.0,
        },
    }));
    let spacer_b = applier.create(Box::new(SpacerNode {
        size: Size {
            width: 5.0,
            height: 30.0,
        },
    }));

    let counter_a = Rc::new(std::cell::Cell::new(0));
    let counter_b = Rc::new(std::cell::Cell::new(0));
    let counter_root = Rc::new(std::cell::Cell::new(0));

    let child_a_policy = Rc::new(CountingPolicy::new(Rc::clone(&counter_a)));
    let child_b_policy = Rc::new(CountingPolicy::new(Rc::clone(&counter_b)));

    let mut child_a = LayoutNode::new(Modifier::empty(), child_a_policy);
    child_a.children.insert(spacer_a);
    let child_a_id = applier.create(Box::new(child_a));

    let mut child_b = LayoutNode::new(Modifier::empty(), child_b_policy);
    child_b.children.insert(spacer_b);
    let child_b_id = applier.create(Box::new(child_b));

    let mut root_node = LayoutNode::new(
        Modifier::empty(),
        Rc::new(CountingPolicy::new(Rc::clone(&counter_root))),
    );
    root_node.children.insert(child_a_id);
    root_node.children.insert(child_b_id);
    let root_id = applier.create(Box::new(root_node));

    applier.with_node(root_id, |node: &mut LayoutNode| node.set_node_id(root_id))?;
    applier.with_node(child_a_id, |node: &mut LayoutNode| {
        node.set_node_id(child_a_id)
    })?;
    applier.with_node(child_b_id, |node: &mut LayoutNode| {
        node.set_node_id(child_b_id)
    })?;

    let size = Size {
        width: 200.0,
        height: 200.0,
    };

    measure_layout(&mut applier, root_id, size)?;

    assert_eq!(counter_root.get(), 1);
    assert_eq!(counter_a.get(), 1);
    assert_eq!(counter_b.get(), 1);

    mark_measure_dirty(child_a_id);

    measure_layout(&mut applier, root_id, size)?;

    assert_eq!(counter_root.get(), 2);
    assert_eq!(counter_a.get(), 2);
    assert_eq!(counter_b.get(), 1);

    Ok(())
}
