//! Node coordinator system mirroring Jetpack Compose's NodeCoordinator pattern.
//!
//! Coordinators wrap modifier nodes and form a chain that drives measurement, placement,
//! drawing, and hit testing. Each LayoutModifierNode gets its own coordinator instance
//! that persists across recomposition, enabling proper state and invalidation tracking.


use compose_foundation::ModifierNodeContext;
use compose_ui_layout::{Constraints, Measurable, Placeable};
use compose_core::NodeId;
use std::cell::{RefCell, Cell};
use std::rc::Rc;

use crate::modifier::{Size, Point};
use crate::layout::{MeasurePolicy, MeasureResult, LayoutNodeContext};

/// Core coordinator trait that all coordinators implement.
///
/// Coordinators are chained together, with each one wrapping the next inner coordinator.
/// This forms a measurement and placement chain that mirrors the modifier chain.
pub trait NodeCoordinator: Measurable {
}


/// Coordinator that wraps a single LayoutModifierNode from the reconciled chain.
///
/// This is analogous to Jetpack Compose's LayoutModifierNodeCoordinator.
/// It delegates measurement to the wrapped node, passing the inner coordinator as the measurable.
pub struct LayoutModifierCoordinator<'a> {
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
    /// Shared context for invalidation tracking.
    context: Rc<RefCell<LayoutNodeContext>>,
}

impl<'a> LayoutModifierCoordinator<'a> {
    /// Creates a new coordinator wrapping the specified node.
    #[allow(private_interfaces)]
    pub fn new(
        node: Rc<RefCell<Box<dyn compose_foundation::ModifierNode>>>,
        wrapped: Box<dyn NodeCoordinator + 'a>,
        context: Rc<RefCell<LayoutNodeContext>>,
    ) -> Self {
        Self {
            node,
            wrapped,
            measured_size: Cell::new(Size::ZERO),
            placement_offset: Cell::new(Point::default()),
            context,
        }
    }

}

impl<'a> NodeCoordinator for LayoutModifierCoordinator<'a> {
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
            result_holder,
        }
    }
}

impl<'a> NodeCoordinator for InnerCoordinator<'a> {
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
