//! Node coordinator system mirroring Jetpack Compose's NodeCoordinator pattern.
//!
//! Coordinators wrap modifier nodes and form a chain that drives measurement, placement,
//! drawing, and hit testing. Each LayoutModifierNode gets its own coordinator instance
//! that persists across recomposition, enabling proper state and invalidation tracking.

use compose_foundation::{LayoutModifierNode, MeasurementProxy, ModifierNodeContext, NodeCapabilities};
use compose_ui_layout::{Constraints, Measurable, Placeable};
use compose_core::NodeId;
use std::cell::{RefCell, Cell};
use std::rc::Rc;

use crate::modifier::{Size, Point};
use crate::layout::{MeasurePolicy, MeasureResult, LayoutNodeContext};
use crate::widgets::nodes::LayoutNode;

/// Identifies what type of coordinator this is for debugging and downcast purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorKind {
    /// The innermost coordinator that wraps the layout node's intrinsic content.
    Inner,
    /// A coordinator wrapping a single layout modifier node.
    LayoutModifier,
}

/// Core coordinator trait that all coordinators implement.
///
/// Coordinators are chained together, with each one wrapping the next inner coordinator.
/// This forms a measurement and placement chain that mirrors the modifier chain.
pub trait NodeCoordinator: Measurable {
    /// Returns the kind of this coordinator.
    fn kind(&self) -> CoordinatorKind;

    /// Returns the measured size after a successful measure pass.
    fn measured_size(&self) -> Size;

    /// Returns the position of this coordinator relative to its parent.
    fn position(&self) -> Point;

    /// Sets the position of this coordinator relative to its parent.
    fn set_position(&mut self, position: Point);

    /// Performs placement, which may recursively trigger child placement.
    fn place(&mut self, x: f32, y: f32);
}


/// Coordinator that wraps a single LayoutModifierNode from the reconciled chain.
///
/// This is analogous to Jetpack Compose's LayoutModifierNodeCoordinator.
/// It delegates measurement to the wrapped node, passing the inner coordinator as the measurable.
pub struct LayoutModifierCoordinator<'a> {
    /// Reference to the applier state to access the chain.
    state_rc: Rc<RefCell<crate::layout::LayoutBuilderState>>,
    /// The node ID to locate the LayoutNode.
    node_id: NodeId,
    /// The index of this node in the modifier chain.
    node_index: usize,
    /// Direct reference to the layout modifier node.
    /// This Rc<RefCell<>> allows the coordinator to hold a shared reference
    /// and call the node's measure() method directly without proxies.
    node: Rc<RefCell<Box<dyn compose_foundation::ModifierNode>>>,
    /// The inner (wrapped) coordinator.
    wrapped: Box<dyn NodeCoordinator + 'a>,
    /// The measured size from the last measure pass.
    measured_size: Cell<Size>,
    /// The placement offset from the last measure pass.
    /// This tells us where to place the wrapped content relative to this coordinator's position.
    placement_offset: Cell<Point>,
    /// Position relative to parent.
    position: Cell<Point>,
    /// Shared context for invalidation tracking.
    context: Rc<RefCell<LayoutNodeContext>>,
}

impl<'a> LayoutModifierCoordinator<'a> {
    /// Creates a new coordinator wrapping the specified node.
    pub fn new(
        state_rc: Rc<RefCell<crate::layout::LayoutBuilderState>>,
        node_id: NodeId,
        node_index: usize,
        node: Rc<RefCell<Box<dyn compose_foundation::ModifierNode>>>,
        wrapped: Box<dyn NodeCoordinator + 'a>,
        context: Rc<RefCell<LayoutNodeContext>>,
    ) -> Self {
        Self {
            state_rc,
            node_id,
            node_index,
            node,
            wrapped,
            measured_size: Cell::new(Size::ZERO),
            placement_offset: Cell::new(Point::default()),
            position: Cell::new(Point::default()),
            context,
        }
    }

    /// Returns the index of the wrapped node in the modifier chain.
    pub fn node_index(&self) -> usize {
        self.node_index
    }
}

impl<'a> NodeCoordinator for LayoutModifierCoordinator<'a> {
    fn kind(&self) -> CoordinatorKind {
        CoordinatorKind::LayoutModifier
    }

    fn measured_size(&self) -> Size {
        self.measured_size.get()
    }

    fn position(&self) -> Point {
        self.position.get()
    }

    fn set_position(&mut self, position: Point) {
        self.position.set(position);
    }

    fn place(&mut self, x: f32, y: f32) {
        self.set_position(Point { x, y });
        // Apply the placement offset from the measure result when placing the wrapped content.
        // This allows modifiers like Padding to offset their children appropriately.
        let offset = self.placement_offset.get();
        self.wrapped.place(x + offset.x, y + offset.y);
    }
}

impl<'a> Measurable for LayoutModifierCoordinator<'a> {
    fn measure(&self, constraints: Constraints) -> Box<dyn Placeable> {
        // Call the node's measure method directly using the Rc<RefCell<>> we hold.
        // This achieves true 1:1 parity with Jetpack Compose where coordinators
        // hold direct references to nodes and call them without proxies.
        let result = {
            let node_borrow = self.node.borrow();
            if let Some(layout_node) = node_borrow.as_layout_node() {
                match self.context.try_borrow_mut() {
                    Ok(mut ctx) => layout_node.measure(&mut *ctx, self.wrapped.as_ref(), constraints),
                    Err(_) => {
                        // Context already borrowed - use a temporary context
                        let mut temp = LayoutNodeContext::new();
                        let result = layout_node.measure(&mut temp, self.wrapped.as_ref(), constraints);

                        // Merge invalidations from temp context to shared
                        if let Ok(mut shared) = self.context.try_borrow_mut() {
                            for kind in temp.take_invalidations() {
                                shared.invalidate(kind);
                            }
                        }

                        result
                    }
                }
            } else {
                // Node is not a layout modifier - pass through to wrapped coordinator
                let placeable = self.wrapped.measure(constraints);
                compose_ui_layout::LayoutModifierMeasureResult::with_size(Size {
                    width: placeable.width(),
                    height: placeable.height(),
                })
            }
        };

        // Store both the size and the placement offset for use during placement
        self.measured_size.set(result.size);
        self.placement_offset.set(Point {
            x: result.placement_offset_x,
            y: result.placement_offset_y,
        });
        Box::new(CoordinatorPlaceable { size: result.size })
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        let node_borrow = self.node.borrow();
        if let Some(layout_node) = node_borrow.as_layout_node() {
            layout_node.min_intrinsic_width(self.wrapped.as_ref(), height)
        } else {
            self.wrapped.min_intrinsic_width(height)
        }
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        let node_borrow = self.node.borrow();
        if let Some(layout_node) = node_borrow.as_layout_node() {
            layout_node.max_intrinsic_width(self.wrapped.as_ref(), height)
        } else {
            self.wrapped.max_intrinsic_width(height)
        }
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        let node_borrow = self.node.borrow();
        if let Some(layout_node) = node_borrow.as_layout_node() {
            layout_node.min_intrinsic_height(self.wrapped.as_ref(), width)
        } else {
            self.wrapped.min_intrinsic_height(width)
        }
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        let node_borrow = self.node.borrow();
        if let Some(layout_node) = node_borrow.as_layout_node() {
            layout_node.max_intrinsic_height(self.wrapped.as_ref(), width)
        } else {
            self.wrapped.max_intrinsic_height(width)
        }
    }
}

/// Inner coordinator that wraps the layout node's intrinsic content (MeasurePolicy).
///
/// This is analogous to Jetpack Compose's InnerNodeCoordinator.
pub struct InnerCoordinator<'a> {
    /// The measure policy to execute.
    measure_policy: Rc<dyn MeasurePolicy>,
    /// Child measurables.
    measurables: &'a [Box<dyn Measurable>],
    /// Measured size from last measure pass.
    measured_size: Cell<Size>,
    /// Position relative to parent.
    position: Cell<Point>,
    /// Shared result holder to store the measure result for placement.
    result_holder: Rc<RefCell<Option<MeasureResult>>>,
}

impl<'a> InnerCoordinator<'a> {
    /// Creates a new inner coordinator with the given measure policy and children.
    pub fn new(
        measure_policy: Rc<dyn MeasurePolicy>,
        measurables: &'a [Box<dyn Measurable>],
        result_holder: Rc<RefCell<Option<MeasureResult>>>,
    ) -> Self {
        Self {
            measure_policy,
            measurables,
            measured_size: Cell::new(Size::ZERO),
            position: Cell::new(Point::default()),
            result_holder,
        }
    }
}

impl<'a> NodeCoordinator for InnerCoordinator<'a> {
    fn kind(&self) -> CoordinatorKind {
        CoordinatorKind::Inner
    }

    fn measured_size(&self) -> Size {
        self.measured_size.get()
    }

    fn position(&self) -> Point {
        self.position.get()
    }

    fn set_position(&mut self, position: Point) {
        self.position.set(position);
    }

    fn place(&mut self, x: f32, y: f32) {
        self.set_position(Point { x, y });
        // Execute placements from the last measure result
        if let Some(result) = self.result_holder.borrow().as_ref() {
            for placement in &result.placements {
                // Place children - actual implementation would go here
                let _ = placement;
            }
        }
    }
}

impl<'a> Measurable for InnerCoordinator<'a> {
    fn measure(&self, constraints: Constraints) -> Box<dyn Placeable> {
        // Execute the measure policy
        let result = self.measure_policy.measure(self.measurables, constraints);

        // Store measured size
        let size = result.size;
        self.measured_size.set(size);

        // Store the result in the shared holder for placement extraction
        *self.result_holder.borrow_mut() = Some(result);

        Box::new(CoordinatorPlaceable { size })
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.measure_policy.min_intrinsic_width(self.measurables, height)
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.measure_policy.max_intrinsic_width(self.measurables, height)
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.measure_policy.min_intrinsic_height(self.measurables, width)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.measure_policy.max_intrinsic_height(self.measurables, width)
    }
}

/// Placeable implementation for coordinators.
struct CoordinatorPlaceable {
    size: Size,
}

impl Placeable for CoordinatorPlaceable {
    fn width(&self) -> f32 {
        self.size.width
    }

    fn height(&self) -> f32 {
        self.size.height
    }

    fn place(&self, _x: f32, _y: f32) {
        // Placement is handled by the coordinator
    }

    fn node_id(&self) -> NodeId {
        NodeId::default()
    }
}
