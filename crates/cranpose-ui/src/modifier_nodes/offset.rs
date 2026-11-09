// The offset modifiers, by points and by fractions of the content's size:
// their nodes and elements.

use std::hash::{Hash, Hasher};

use cranpose_foundation::{
    Constraints, DelegatableNode, InvalidationKind, LayoutModifierNode, Measurable, ModifierNode,
    ModifierNodeContext, ModifierNodeElement, NodeCapabilities, NodeState, Size,
};

use super::{hash_f32_value, impl_layout_modifier_node, measure_pass_through};
use crate::modifier::Point;

/// Node that offsets its content by a fixed (x, y) amount.
///
/// Matches Kotlin: `OffsetNode` in foundation-layout/src/commonMain/kotlin/androidx/compose/foundation/layout/Offset.kt
#[derive(Debug)]
pub struct OffsetNode {
    x: f32,
    y: f32,
    rtl_aware: bool,
    layout_direction: crate::LayoutDirection,
    state: NodeState,
}

impl OffsetNode {
    /// Creates a node that moves its content by `x` and `y`, in logical pixels. With `rtl_aware`
    /// set, a right-to-left layout direction mirrors `x`.
    pub fn new(x: f32, y: f32, rtl_aware: bool) -> Self {
        Self {
            x,
            y,
            rtl_aware,
            layout_direction: crate::LayoutDirection::Ltr,
            state: NodeState::new(),
        }
    }

    /// Returns the offset in logical pixels, with `x` mirrored when the node is RTL aware and the
    /// layout direction is right-to-left.
    pub fn offset(&self) -> Point {
        Point {
            x: if self.rtl_aware && self.layout_direction.is_rtl() {
                -self.x
            } else {
                self.x
            },
            y: self.y,
        }
    }

    /// The offset on a whole device pixel of `density`, where Compose's
    /// `OffsetNode` places its content.
    pub fn device_offset(&self, density: f32) -> Point {
        use cranpose_ui_layout::round_to_px;
        let offset = self.offset();
        Point {
            x: round_to_px(offset.x, density),
            y: round_to_px(offset.y, density),
        }
    }

    /// Returns whether a right-to-left layout direction mirrors `x`.
    pub fn rtl_aware(&self) -> bool {
        self.rtl_aware
    }

    pub(crate) fn set_layout_direction(&mut self, direction: crate::LayoutDirection) {
        self.layout_direction = direction;
    }
}

impl DelegatableNode for OffsetNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl_layout_modifier_node!(OffsetNode, invalidate = InvalidationKind::Layout);

impl LayoutModifierNode for OffsetNode {
    fn measure_hold(
        &self,
        _density: f32,
        _constraints: Constraints,
        _size: Size,
        wrapped: cranpose_ui_layout::WrappedHold,
    ) -> Option<cranpose_ui_layout::ConstraintsHold> {
        wrapped.hold
    }

    fn measure(
        &self,
        context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        let offset = self.device_offset(context.density());
        measure_pass_through(measurable, constraints, |_| (offset.x, offset.y))
    }
}

/// Element that creates and updates offset nodes.
///
/// Matches Kotlin: `OffsetElement` in foundation-layout/src/commonMain/kotlin/androidx/compose/foundation/layout/Offset.kt
#[derive(Debug, Clone, PartialEq)]
pub struct OffsetElement {
    x: f32,
    y: f32,
    rtl_aware: bool,
}

impl OffsetElement {
    /// Creates an element that moves content by `x` and `y`, in logical pixels. With `rtl_aware`
    /// set, a right-to-left layout direction mirrors `x`.
    pub fn new(x: f32, y: f32, rtl_aware: bool) -> Self {
        Self { x, y, rtl_aware }
    }
}

impl Hash for OffsetElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_f32_value(state, self.x);
        hash_f32_value(state, self.y);
        self.rtl_aware.hash(state);
    }
}

impl ModifierNodeElement for OffsetElement {
    type Node = OffsetNode;

    fn create(&self) -> Self::Node {
        OffsetNode::new(self.x, self.y, self.rtl_aware)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.x != self.x || node.y != self.y || node.rtl_aware != self.rtl_aware {
            node.x = self.x;
            node.y = self.y;
            node.rtl_aware = self.rtl_aware;
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }

    fn update_invalidation_kind(&self) -> Option<InvalidationKind> {
        Some(InvalidationKind::Layout)
    }
}

/// Node that offsets its content by a fraction of its own measured size.
///
/// There is no direct Jetpack Compose modifier equivalent; Compose's slide
/// transitions receive the measured size through a lambda instead. This node
/// backs `slide_in_vertically` / `slide_out_vertically` in
/// `AnimatedVisibility`, where the offset is expressed as a fraction of the
/// content height.
#[derive(Debug)]
pub struct FractionalOffsetNode {
    x_fraction: f32,
    y_fraction: f32,
    state: NodeState,
}

impl FractionalOffsetNode {
    /// Creates a node that moves its content by `x_fraction` of its own width and `y_fraction` of
    /// its own height.
    pub fn new(x_fraction: f32, y_fraction: f32) -> Self {
        Self {
            x_fraction,
            y_fraction,
            state: NodeState::new(),
        }
    }

    /// Returns the fractions as a point: `x` is the share of the width and `y` the share of the
    /// height.
    pub fn fractions(&self) -> Point {
        Point {
            x: self.x_fraction,
            y: self.y_fraction,
        }
    }
}

impl DelegatableNode for FractionalOffsetNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl_layout_modifier_node!(FractionalOffsetNode, invalidate = InvalidationKind::Layout);

impl LayoutModifierNode for FractionalOffsetNode {
    fn measure_hold(
        &self,
        _density: f32,
        _constraints: Constraints,
        _size: Size,
        wrapped: cranpose_ui_layout::WrappedHold,
    ) -> Option<cranpose_ui_layout::ConstraintsHold> {
        wrapped.hold
    }

    fn measure(
        &self,
        _context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        measure_pass_through(measurable, constraints, |size| {
            (self.x_fraction * size.width, self.y_fraction * size.height)
        })
    }
}

/// Element that creates and updates fractional offset nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct FractionalOffsetElement {
    x_fraction: f32,
    y_fraction: f32,
}

impl FractionalOffsetElement {
    /// Creates an element for a node that moves its content by `x_fraction` of its own width and
    /// `y_fraction` of its own height.
    pub fn new(x_fraction: f32, y_fraction: f32) -> Self {
        Self {
            x_fraction,
            y_fraction,
        }
    }
}

impl Hash for FractionalOffsetElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "fractional_offset".hash(state);
        hash_f32_value(state, self.x_fraction);
        hash_f32_value(state, self.y_fraction);
    }
}

impl ModifierNodeElement for FractionalOffsetElement {
    type Node = FractionalOffsetNode;

    fn create(&self) -> Self::Node {
        FractionalOffsetNode::new(self.x_fraction, self.y_fraction)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.x_fraction != self.x_fraction || node.y_fraction != self.y_fraction {
            node.x_fraction = self.x_fraction;
            node.y_fraction = self.y_fraction;
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }

    fn update_invalidation_kind(&self) -> Option<InvalidationKind> {
        Some(InvalidationKind::Layout)
    }
}
