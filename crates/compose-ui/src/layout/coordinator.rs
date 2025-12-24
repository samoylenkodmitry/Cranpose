//! Node coordinator system mirroring Jetpack Compose's NodeCoordinator pattern.
//!
//! Coordinators wrap modifier nodes and form a chain that drives measurement, placement,
//! drawing, and hit testing. Each LayoutModifierNode gets its own coordinator instance
//! that persists across recomposition, enabling proper state and invalidation tracking.
//!
//! **Content Offset Tracking**: Each coordinator contributes its placement offset to a
//! shared accumulator during measurement. The final `content_offset()` is read from this
//! accumulator via the outermost CoordinatorPlaceable.

use compose_core::NodeId;
use compose_foundation::ModifierNodeContext;
use compose_ui_layout::{Constraints, Measurable, Placeable};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::layout::{LayoutNodeContext, MeasurePolicy, MeasureResult};
use crate::modifier::{Point, Size};

/// Core coordinator trait that all coordinators implement.
///
/// Coordinators are chained together, with each one wrapping the next.
/// Coordinators wrap either other coordinators or the inner coordinator.
pub trait NodeCoordinator: Measurable {
    /// Returns the accumulated placement offset from this coordinator
    /// down through the wrapped chain (inner-most coordinator).
    fn total_content_offset(&self) -> Point;
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
    /// The ACCUMULATED placement offset from this coordinator through the entire chain.
    /// This is local_offset + wrapped.total_content_offset(), stored for O(1) access.
    accumulated_offset: Cell<Point>,
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
            measured_size: Cell::new(Size::default()),
            accumulated_offset: Cell::new(Point::default()),
            context,
        }
    }
}

impl<'a> NodeCoordinator for LayoutModifierCoordinator<'a> {
    fn total_content_offset(&self) -> Point {
        // O(1): just return the pre-computed accumulated offset
        self.accumulated_offset.get()
    }
}

impl<'a> Measurable for LayoutModifierCoordinator<'a> {
    /// Measure through this coordinator
    fn measure(&self, constraints: Constraints) -> Box<dyn Placeable> {
        let node_borrow = self.node.borrow();

        let result = {
            if let Some(layout_node) = node_borrow.as_layout_node() {
                match self.context.try_borrow_mut() {
                    Ok(mut context) => {
                        layout_node.measure(&mut *context, self.wrapped.as_ref(), constraints)
                    }
                    Err(_) => {
                        // Context already borrowed - use a temporary context
                        let mut temp = LayoutNodeContext::new();
                        let result =
                            layout_node.measure(&mut temp, self.wrapped.as_ref(), constraints);

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
                // Pass through the child's accumulated offset (stored from its measure())
                let child_accumulated = self.wrapped.total_content_offset();
                self.accumulated_offset.set(child_accumulated);
                return Box::new(CoordinatorPlaceable {
                    size: Size {
                        width: placeable.width(),
                        height: placeable.height(),
                    },
                    content_offset: child_accumulated,
                });
            }
        };

        // Store size
        self.measured_size.set(result.size);

        // Compute local offset from this coordinator
        let local_offset = Point {
            x: result.placement_offset_x,
            y: result.placement_offset_y,
        };

        // Get wrapped's accumulated offset (O(1) - just reads its stored value)
        // Note: wrapped.measure() was called by layout_node.measure(), so its offset is already set
        let child_accumulated = self.wrapped.total_content_offset();

        // Store OUR accumulated offset (local + child)
        let accumulated = Point {
            x: local_offset.x + child_accumulated.x,
            y: local_offset.y + child_accumulated.y,
        };
        self.accumulated_offset.set(accumulated);

        Box::new(CoordinatorPlaceable {
            size: result.size,
            content_offset: accumulated,
        })
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
    fn total_content_offset(&self) -> Point {
        Point::default()
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

        // InnerCoordinator has no offset contribution
        Box::new(CoordinatorPlaceable {
            size,
            content_offset: Point::default(),
        })
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.measure_policy
            .min_intrinsic_width(self.measurables, height)
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.measure_policy
            .max_intrinsic_width(self.measurables, height)
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.measure_policy
            .min_intrinsic_height(self.measurables, width)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.measure_policy
            .max_intrinsic_height(self.measurables, width)
    }
}

/// Placeable implementation for coordinators.
/// Carries the accumulated content offset from the coordinator chain.
struct CoordinatorPlaceable {
    size: Size,
    /// Accumulated content offset (sum of all offsets from this coordinator down).
    content_offset: Point,
}

impl Placeable for CoordinatorPlaceable {
    fn width(&self) -> f32 {
        self.size.width
    }

    fn height(&self) -> f32 {
        self.size.height
    }

    fn place(&self, _x: f32, _y: f32) {
        // Placement is handled externally by the layout system
    }

    fn node_id(&self) -> NodeId {
        NodeId::default()
    }

    fn content_offset(&self) -> (f32, f32) {
        (self.content_offset.x, self.content_offset.y)
    }
}
