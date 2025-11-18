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

use crate::modifier::{Size, Point, EdgeInsets};
use crate::layout::{MeasurePolicy, MeasureResult, LayoutNodeContext};
use crate::widgets::nodes::LayoutNode;

/// Trait for nodes that can create a measurement proxy to work around borrow checker constraints.
///
/// In Jetpack Compose, coordinators can hold direct references to nodes. In Rust, we need to
/// work around the borrow checker by extracting enough information to perform measurement
/// without holding a borrow to the modifier chain.
trait MeasurementProxy {
    fn measure_proxy(&self, context: &mut dyn ModifierNodeContext, wrapped: &dyn Measurable, constraints: Constraints) -> Size;
    fn min_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32;
    fn max_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32;
    fn min_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32;
    fn max_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32;
}

/// Generic proxy that works for any layout modifier node by recreating it from configuration data.
struct GenericMeasurementProxy<T> {
    data: T,
}

impl<T> GenericMeasurementProxy<T> {
    fn new(data: T) -> Self {
        Self { data }
    }
}

// Implement proxy for known node types
use crate::modifier_nodes::{PaddingNode, SizeNode, FillNode, OffsetNode, FillDirection};
use crate::text_modifier_node::TextModifierNode;

// Proxy implementations for each known node type
impl MeasurementProxy for GenericMeasurementProxy<EdgeInsets> {
    fn measure_proxy(&self, context: &mut dyn ModifierNodeContext, wrapped: &dyn Measurable, constraints: Constraints) -> Size {
        let node = PaddingNode::new(self.data);
        node.measure(context, wrapped, constraints)
    }

    fn min_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
        let node = PaddingNode::new(self.data);
        node.min_intrinsic_width(wrapped, height)
    }

    fn max_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
        let node = PaddingNode::new(self.data);
        node.max_intrinsic_width(wrapped, height)
    }

    fn min_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32 {
        let node = PaddingNode::new(self.data);
        node.min_intrinsic_height(wrapped, width)
    }

    fn max_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32 {
        let node = PaddingNode::new(self.data);
        node.max_intrinsic_height(wrapped, width)
    }
}

struct SizeNodeConfig {
    min_width: Option<f32>,
    max_width: Option<f32>,
    min_height: Option<f32>,
    max_height: Option<f32>,
    enforce: bool,
}

impl MeasurementProxy for GenericMeasurementProxy<SizeNodeConfig> {
    fn measure_proxy(&self, context: &mut dyn ModifierNodeContext, wrapped: &dyn Measurable, constraints: Constraints) -> Size {
        let node = SizeNode::new(self.data.min_width, self.data.max_width, self.data.min_height, self.data.max_height, self.data.enforce);
        node.measure(context, wrapped, constraints)
    }

    fn min_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
        let node = SizeNode::new(self.data.min_width, self.data.max_width, self.data.min_height, self.data.max_height, self.data.enforce);
        node.min_intrinsic_width(wrapped, height)
    }

    fn max_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
        let node = SizeNode::new(self.data.min_width, self.data.max_width, self.data.min_height, self.data.max_height, self.data.enforce);
        node.max_intrinsic_width(wrapped, height)
    }

    fn min_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32 {
        let node = SizeNode::new(self.data.min_width, self.data.max_width, self.data.min_height, self.data.max_height, self.data.enforce);
        node.min_intrinsic_height(wrapped, width)
    }

    fn max_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32 {
        let node = SizeNode::new(self.data.min_width, self.data.max_width, self.data.min_height, self.data.max_height, self.data.enforce);
        node.max_intrinsic_height(wrapped, width)
    }
}

struct FillNodeConfig {
    direction: FillDirection,
    fraction: f32,
}

impl MeasurementProxy for GenericMeasurementProxy<FillNodeConfig> {
    fn measure_proxy(&self, context: &mut dyn ModifierNodeContext, wrapped: &dyn Measurable, constraints: Constraints) -> Size {
        let node = FillNode::new(self.data.direction, self.data.fraction);
        node.measure(context, wrapped, constraints)
    }

    fn min_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
        let node = FillNode::new(self.data.direction, self.data.fraction);
        node.min_intrinsic_width(wrapped, height)
    }

    fn max_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
        let node = FillNode::new(self.data.direction, self.data.fraction);
        node.max_intrinsic_width(wrapped, height)
    }

    fn min_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32 {
        let node = FillNode::new(self.data.direction, self.data.fraction);
        node.min_intrinsic_height(wrapped, width)
    }

    fn max_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32 {
        let node = FillNode::new(self.data.direction, self.data.fraction);
        node.max_intrinsic_height(wrapped, width)
    }
}

struct OffsetNodeConfig {
    offset: Point,
    rtl_aware: bool,
}

impl MeasurementProxy for GenericMeasurementProxy<OffsetNodeConfig> {
    fn measure_proxy(&self, context: &mut dyn ModifierNodeContext, wrapped: &dyn Measurable, constraints: Constraints) -> Size {
        let node = OffsetNode::new(self.data.offset.x, self.data.offset.y, self.data.rtl_aware);
        node.measure(context, wrapped, constraints)
    }

    fn min_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
        let node = OffsetNode::new(self.data.offset.x, self.data.offset.y, self.data.rtl_aware);
        node.min_intrinsic_width(wrapped, height)
    }

    fn max_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
        let node = OffsetNode::new(self.data.offset.x, self.data.offset.y, self.data.rtl_aware);
        node.max_intrinsic_width(wrapped, height)
    }

    fn min_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32 {
        let node = OffsetNode::new(self.data.offset.x, self.data.offset.y, self.data.rtl_aware);
        node.min_intrinsic_height(wrapped, width)
    }

    fn max_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32 {
        let node = OffsetNode::new(self.data.offset.x, self.data.offset.y, self.data.rtl_aware);
        node.max_intrinsic_height(wrapped, width)
    }
}

impl MeasurementProxy for GenericMeasurementProxy<String> {
    fn measure_proxy(&self, context: &mut dyn ModifierNodeContext, wrapped: &dyn Measurable, constraints: Constraints) -> Size {
        let node = TextModifierNode::new(self.data.clone());
        node.measure(context, wrapped, constraints)
    }

    fn min_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
        let node = TextModifierNode::new(self.data.clone());
        node.min_intrinsic_width(wrapped, height)
    }

    fn max_intrinsic_width_proxy(&self, wrapped: &dyn Measurable, height: f32) -> f32 {
        let node = TextModifierNode::new(self.data.clone());
        node.max_intrinsic_width(wrapped, height)
    }

    fn min_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32 {
        let node = TextModifierNode::new(self.data.clone());
        node.min_intrinsic_height(wrapped, width)
    }

    fn max_intrinsic_height_proxy(&self, wrapped: &dyn Measurable, width: f32) -> f32 {
        let node = TextModifierNode::new(self.data.clone());
        node.max_intrinsic_height(wrapped, width)
    }
}

/// Extracts a measurement proxy from a layout modifier node.
/// Returns None if the node type is unknown (for future extensibility).
fn extract_measurement_proxy(node: &dyn LayoutModifierNode) -> Option<Box<dyn MeasurementProxy>> {
    let any = node as &dyn std::any::Any;

    if let Some(padding_node) = any.downcast_ref::<PaddingNode>() {
        Some(Box::new(GenericMeasurementProxy::new(padding_node.padding())))
    } else if let Some(size_node) = any.downcast_ref::<SizeNode>() {
        Some(Box::new(GenericMeasurementProxy::new(SizeNodeConfig {
            min_width: size_node.min_width(),
            max_width: size_node.max_width(),
            min_height: size_node.min_height(),
            max_height: size_node.max_height(),
            enforce: size_node.enforce_incoming(),
        })))
    } else if let Some(fill_node) = any.downcast_ref::<FillNode>() {
        Some(Box::new(GenericMeasurementProxy::new(FillNodeConfig {
            direction: fill_node.direction(),
            fraction: fill_node.fraction(),
        })))
    } else if let Some(offset_node) = any.downcast_ref::<OffsetNode>() {
        Some(Box::new(GenericMeasurementProxy::new(OffsetNodeConfig {
            offset: offset_node.offset(),
            rtl_aware: offset_node.rtl_aware(),
        })))
    } else if let Some(text_node) = any.downcast_ref::<TextModifierNode>() {
        Some(Box::new(GenericMeasurementProxy::new(text_node.text().to_string())))
    } else {
        None
    }
}

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
        // Propagate placement through the coordinator chain
        self.wrapped.place(x, y);
    }
}

impl<'a> Measurable for LayoutModifierCoordinator<'a> {
    fn measure(&self, constraints: Constraints) -> Box<dyn Placeable> {
        // Extract a measurement proxy from the node to work around Rust's borrow checker.
        // In Jetpack Compose, coordinators can hold direct references to nodes, but in Rust
        // we need to extract configuration data first, release the borrow, then perform
        // measurement. This preserves node behavior while avoiding nested borrow panics.

        let proxy = {
            let state = self.state_rc.borrow();
            let mut applier = state.applier.borrow_typed();

            applier
                .with_node::<LayoutNode, _>(self.node_id, |layout_node| {
                    let chain = layout_node.modifier_chain().chain();

                    if let Some(entry_ref) = chain.node_ref_at(self.node_index) {
                        if let Some(node) = entry_ref.node() {
                            if let Some(layout_modifier) = node.as_layout_node() {
                                return extract_measurement_proxy(layout_modifier);
                            }
                        }
                    }
                    None
                })
                .unwrap_or(None)
        };

        let size = if let Some(proxy) = proxy {
            // Use the proxy to measure with the applier borrow released
            match self.context.try_borrow_mut() {
                Ok(mut ctx) => proxy.measure_proxy(&mut *ctx, self.wrapped.as_ref(), constraints),
                Err(_) => {
                    // Context already borrowed - use a temporary context
                    let mut temp = LayoutNodeContext::new();
                    let size = proxy.measure_proxy(&mut temp, self.wrapped.as_ref(), constraints);

                    // Merge invalidations from temp context to shared
                    if let Ok(mut shared) = self.context.try_borrow_mut() {
                        for kind in temp.take_invalidations() {
                            shared.invalidate(kind);
                        }
                    }

                    size
                }
            }
        } else {
            // Unknown node type - pass through to wrapped coordinator
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
        let proxy = {
            let state = self.state_rc.borrow();
            let mut applier = state.applier.borrow_typed();

            applier
                .with_node::<LayoutNode, _>(self.node_id, |layout_node| {
                    let chain = layout_node.modifier_chain().chain();

                    if let Some(entry_ref) = chain.node_ref_at(self.node_index) {
                        if let Some(node) = entry_ref.node() {
                            if let Some(layout_modifier) = node.as_layout_node() {
                                return extract_measurement_proxy(layout_modifier);
                            }
                        }
                    }
                    None
                })
                .unwrap_or(None)
        };

        if let Some(proxy) = proxy {
            proxy.min_intrinsic_width_proxy(self.wrapped.as_ref(), height)
        } else {
            self.wrapped.min_intrinsic_width(height)
        }
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        let proxy = {
            let state = self.state_rc.borrow();
            let mut applier = state.applier.borrow_typed();

            applier
                .with_node::<LayoutNode, _>(self.node_id, |layout_node| {
                    let chain = layout_node.modifier_chain().chain();

                    if let Some(entry_ref) = chain.node_ref_at(self.node_index) {
                        if let Some(node) = entry_ref.node() {
                            if let Some(layout_modifier) = node.as_layout_node() {
                                return extract_measurement_proxy(layout_modifier);
                            }
                        }
                    }
                    None
                })
                .unwrap_or(None)
        };

        if let Some(proxy) = proxy {
            proxy.max_intrinsic_width_proxy(self.wrapped.as_ref(), height)
        } else {
            self.wrapped.max_intrinsic_width(height)
        }
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        let proxy = {
            let state = self.state_rc.borrow();
            let mut applier = state.applier.borrow_typed();

            applier
                .with_node::<LayoutNode, _>(self.node_id, |layout_node| {
                    let chain = layout_node.modifier_chain().chain();

                    if let Some(entry_ref) = chain.node_ref_at(self.node_index) {
                        if let Some(node) = entry_ref.node() {
                            if let Some(layout_modifier) = node.as_layout_node() {
                                return extract_measurement_proxy(layout_modifier);
                            }
                        }
                    }
                    None
                })
                .unwrap_or(None)
        };

        if let Some(proxy) = proxy {
            proxy.min_intrinsic_height_proxy(self.wrapped.as_ref(), width)
        } else {
            self.wrapped.min_intrinsic_height(width)
        }
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        let proxy = {
            let state = self.state_rc.borrow();
            let mut applier = state.applier.borrow_typed();

            applier
                .with_node::<LayoutNode, _>(self.node_id, |layout_node| {
                    let chain = layout_node.modifier_chain().chain();

                    if let Some(entry_ref) = chain.node_ref_at(self.node_index) {
                        if let Some(node) = entry_ref.node() {
                            if let Some(layout_modifier) = node.as_layout_node() {
                                return extract_measurement_proxy(layout_modifier);
                            }
                        }
                    }
                    None
                })
                .unwrap_or(None)
        };

        if let Some(proxy) = proxy {
            proxy.max_intrinsic_height_proxy(self.wrapped.as_ref(), width)
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
