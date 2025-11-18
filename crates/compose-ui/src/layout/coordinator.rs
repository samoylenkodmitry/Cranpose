//! Node coordinator system mirroring Jetpack Compose's NodeCoordinator pattern.
//!
//! Coordinators wrap modifier nodes and form a chain that drives measurement, placement,
//! drawing, and hit testing. Each LayoutModifierNode gets its own coordinator instance
//! that persists across recomposition, enabling proper state and invalidation tracking.

use compose_foundation::{LayoutModifierNode, ModifierNodeContext, NodeCapabilities};
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

/// Configuration extracted from a modifier node to allow measurement without holding applier borrow.
enum NodeConfig {
    Padding(crate::modifier::EdgeInsets),
    Size {
        min_width: Option<f32>,
        max_width: Option<f32>,
        min_height: Option<f32>,
        max_height: Option<f32>,
        enforce_incoming: bool,
    },
    Fill {
        direction: crate::modifier_nodes::FillDirection,
        fraction: f32,
    },
    Offset {
        offset: crate::modifier::Point,
        rtl_aware: bool,
    },
    Text(String),
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
    /// The inner (wrapped) coordinator.
    wrapped: Box<dyn NodeCoordinator + 'a>,
    /// The measured size from the last measure pass.
    measured_size: Cell<Size>,
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
        wrapped: Box<dyn NodeCoordinator + 'a>,
        context: Rc<RefCell<LayoutNodeContext>>,
    ) -> Self {
        Self {
            state_rc,
            node_id,
            node_index,
            wrapped,
            measured_size: Cell::new(Size::ZERO),
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
        // NOTE: Placement propagation not yet implemented.
        // In Jetpack Compose, this would call wrapped.place() with appropriate transformations.
        // Currently, placement is handled through MeasureResult placements after measurement.
        // This will be implemented when coordinators are extended to handle the full layout pass.
    }
}

impl<'a> Measurable for LayoutModifierCoordinator<'a> {
    fn measure(&self, constraints: Constraints) -> Box<dyn Placeable> {
        use crate::modifier_nodes::{PaddingNode, SizeNode, FillNode, OffsetNode};
        use crate::text_modifier_node::TextModifierNode;
        use compose_foundation::LayoutModifierNode;

        // Extract node configuration from the chain, then release the applier borrow
        // before invoking measure to avoid nested borrow conflicts
        let node_config = {
            let state = self.state_rc.borrow();
            let mut applier = state.applier.borrow_typed();

            applier
                .with_node::<LayoutNode, _>(self.node_id, |layout_node| {
                    let chain = layout_node.modifier_chain().chain();

                    if let Some(entry_ref) = chain.node_ref_at(self.node_index) {
                        if let Some(node) = entry_ref.node() {
                            let any = node.as_any();
                            // Extract node configuration for each type
                            if let Some(node) = any.downcast_ref::<PaddingNode>() {
                                Some(NodeConfig::Padding(node.padding()))
                            } else if let Some(node) = any.downcast_ref::<SizeNode>() {
                                Some(NodeConfig::Size {
                                    min_width: node.min_width(),
                                    max_width: node.max_width(),
                                    min_height: node.min_height(),
                                    max_height: node.max_height(),
                                    enforce_incoming: node.enforce_incoming(),
                                })
                            } else if let Some(node) = any.downcast_ref::<FillNode>() {
                                Some(NodeConfig::Fill {
                                    direction: node.direction(),
                                    fraction: node.fraction(),
                                })
                            } else if let Some(node) = any.downcast_ref::<OffsetNode>() {
                                Some(NodeConfig::Offset {
                                    offset: node.offset(),
                                    rtl_aware: node.rtl_aware(),
                                })
                            } else if let Some(node) = any.downcast_ref::<TextModifierNode>() {
                                Some(NodeConfig::Text(node.text().to_string()))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .ok()
                .flatten()
        };

        // Now measure using the extracted configuration (applier borrow is released)
        // Handle nested measurements by using try_borrow_mut
        let size = if let Some(config) = node_config {
            match self.context.try_borrow_mut() {
                Ok(mut ctx) => {
                    // Use the shared context directly
                    match config {
                        NodeConfig::Padding(padding) => {
                            let node = PaddingNode::new(padding);
                            node.measure(&mut *ctx, self.wrapped.as_ref(), constraints)
                        }
                        NodeConfig::Size { min_width, max_width, min_height, max_height, enforce_incoming } => {
                            let node = SizeNode::new(min_width, max_width, min_height, max_height, enforce_incoming);
                            node.measure(&mut *ctx, self.wrapped.as_ref(), constraints)
                        }
                        NodeConfig::Fill { direction, fraction } => {
                            let node = FillNode::new(direction, fraction);
                            node.measure(&mut *ctx, self.wrapped.as_ref(), constraints)
                        }
                        NodeConfig::Offset { offset, rtl_aware } => {
                            let node = OffsetNode::new(offset.x, offset.y, rtl_aware);
                            node.measure(&mut *ctx, self.wrapped.as_ref(), constraints)
                        }
                        NodeConfig::Text(text) => {
                            let node = TextModifierNode::new(text);
                            node.measure(&mut *ctx, self.wrapped.as_ref(), constraints)
                        }
                    }
                }
                Err(_) => {
                    // Context is already borrowed (nested measurement) - use a temporary context
                    let mut temp_context = LayoutNodeContext::new();
                    let size = match config {
                        NodeConfig::Padding(padding) => {
                            let node = PaddingNode::new(padding);
                            node.measure(&mut temp_context, self.wrapped.as_ref(), constraints)
                        }
                        NodeConfig::Size { min_width, max_width, min_height, max_height, enforce_incoming } => {
                            let node = SizeNode::new(min_width, max_width, min_height, max_height, enforce_incoming);
                            node.measure(&mut temp_context, self.wrapped.as_ref(), constraints)
                        }
                        NodeConfig::Fill { direction, fraction } => {
                            let node = FillNode::new(direction, fraction);
                            node.measure(&mut temp_context, self.wrapped.as_ref(), constraints)
                        }
                        NodeConfig::Offset { offset, rtl_aware } => {
                            let node = OffsetNode::new(offset.x, offset.y, rtl_aware);
                            node.measure(&mut temp_context, self.wrapped.as_ref(), constraints)
                        }
                        NodeConfig::Text(text) => {
                            let node = TextModifierNode::new(text);
                            node.measure(&mut temp_context, self.wrapped.as_ref(), constraints)
                        }
                    };

                    // Merge invalidations from temp context into shared context after measurement completes
                    if let Ok(mut shared) = self.context.try_borrow_mut() {
                        for kind in temp_context.take_invalidations() {
                            shared.invalidate(kind);
                        }
                    }

                    size
                }
            }
        } else {
            // Node not found or unknown type - fall back to wrapped
            let placeable = self.wrapped.measure(constraints);
            Size {
                width: placeable.width(),
                height: placeable.height(),
            }
        };

        self.measured_size.set(size);
        Box::new(CoordinatorPlaceable { size })
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        use crate::modifier_nodes::{PaddingNode, SizeNode, FillNode, OffsetNode};
        use crate::text_modifier_node::TextModifierNode;

        let state = self.state_rc.borrow();
        let mut applier = state.applier.borrow_typed();

        applier
            .with_node::<LayoutNode, _>(self.node_id, |layout_node| {
                let chain = layout_node.modifier_chain().chain();

                if let Some(node) = chain.node::<PaddingNode>(self.node_index) {
                    node.min_intrinsic_width(self.wrapped.as_ref(), height)
                } else if let Some(node) = chain.node::<SizeNode>(self.node_index) {
                    node.min_intrinsic_width(self.wrapped.as_ref(), height)
                } else if let Some(node) = chain.node::<FillNode>(self.node_index) {
                    node.min_intrinsic_width(self.wrapped.as_ref(), height)
                } else if let Some(node) = chain.node::<OffsetNode>(self.node_index) {
                    node.min_intrinsic_width(self.wrapped.as_ref(), height)
                } else if let Some(node) = chain.node::<TextModifierNode>(self.node_index) {
                    node.min_intrinsic_width(self.wrapped.as_ref(), height)
                } else {
                    self.wrapped.min_intrinsic_width(height)
                }
            })
            .unwrap_or_else(|_| self.wrapped.min_intrinsic_width(height))
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        use crate::modifier_nodes::{PaddingNode, SizeNode, FillNode, OffsetNode};
        use crate::text_modifier_node::TextModifierNode;

        let state = self.state_rc.borrow();
        let mut applier = state.applier.borrow_typed();

        applier
            .with_node::<LayoutNode, _>(self.node_id, |layout_node| {
                let chain = layout_node.modifier_chain().chain();

                if let Some(node) = chain.node::<PaddingNode>(self.node_index) {
                    node.max_intrinsic_width(self.wrapped.as_ref(), height)
                } else if let Some(node) = chain.node::<SizeNode>(self.node_index) {
                    node.max_intrinsic_width(self.wrapped.as_ref(), height)
                } else if let Some(node) = chain.node::<FillNode>(self.node_index) {
                    node.max_intrinsic_width(self.wrapped.as_ref(), height)
                } else if let Some(node) = chain.node::<OffsetNode>(self.node_index) {
                    node.max_intrinsic_width(self.wrapped.as_ref(), height)
                } else if let Some(node) = chain.node::<TextModifierNode>(self.node_index) {
                    node.max_intrinsic_width(self.wrapped.as_ref(), height)
                } else {
                    self.wrapped.max_intrinsic_width(height)
                }
            })
            .unwrap_or_else(|_| self.wrapped.max_intrinsic_width(height))
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        use crate::modifier_nodes::{PaddingNode, SizeNode, FillNode, OffsetNode};
        use crate::text_modifier_node::TextModifierNode;

        let state = self.state_rc.borrow();
        let mut applier = state.applier.borrow_typed();

        applier
            .with_node::<LayoutNode, _>(self.node_id, |layout_node| {
                let chain = layout_node.modifier_chain().chain();

                if let Some(node) = chain.node::<PaddingNode>(self.node_index) {
                    node.min_intrinsic_height(self.wrapped.as_ref(), width)
                } else if let Some(node) = chain.node::<SizeNode>(self.node_index) {
                    node.min_intrinsic_height(self.wrapped.as_ref(), width)
                } else if let Some(node) = chain.node::<FillNode>(self.node_index) {
                    node.min_intrinsic_height(self.wrapped.as_ref(), width)
                } else if let Some(node) = chain.node::<OffsetNode>(self.node_index) {
                    node.min_intrinsic_height(self.wrapped.as_ref(), width)
                } else if let Some(node) = chain.node::<TextModifierNode>(self.node_index) {
                    node.min_intrinsic_height(self.wrapped.as_ref(), width)
                } else {
                    self.wrapped.min_intrinsic_height(width)
                }
            })
            .unwrap_or_else(|_| self.wrapped.min_intrinsic_height(width))
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        use crate::modifier_nodes::{PaddingNode, SizeNode, FillNode, OffsetNode};
        use crate::text_modifier_node::TextModifierNode;

        let state = self.state_rc.borrow();
        let mut applier = state.applier.borrow_typed();

        applier
            .with_node::<LayoutNode, _>(self.node_id, |layout_node| {
                let chain = layout_node.modifier_chain().chain();

                if let Some(node) = chain.node::<PaddingNode>(self.node_index) {
                    node.max_intrinsic_height(self.wrapped.as_ref(), width)
                } else if let Some(node) = chain.node::<SizeNode>(self.node_index) {
                    node.max_intrinsic_height(self.wrapped.as_ref(), width)
                } else if let Some(node) = chain.node::<FillNode>(self.node_index) {
                    node.max_intrinsic_height(self.wrapped.as_ref(), width)
                } else if let Some(node) = chain.node::<OffsetNode>(self.node_index) {
                    node.max_intrinsic_height(self.wrapped.as_ref(), width)
                } else if let Some(node) = chain.node::<TextModifierNode>(self.node_index) {
                    node.max_intrinsic_height(self.wrapped.as_ref(), width)
                } else {
                    self.wrapped.max_intrinsic_height(width)
                }
            })
            .unwrap_or_else(|_| self.wrapped.max_intrinsic_height(width))
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
