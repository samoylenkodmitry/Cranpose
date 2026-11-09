// The padding modifier: the node that insets its content, and its element.

use std::hash::{Hash, Hasher};

use cranpose_foundation::{
    Constraints, DelegatableNode, InvalidationKind, LayoutModifierNode, Measurable, ModifierNode,
    ModifierNodeContext, ModifierNodeElement, NodeCapabilities, NodeState, Size,
};

use super::{hash_f32_value, impl_layout_modifier_node};
use crate::modifier::EdgeInsets;

/// Node that adds padding around its content.
#[derive(Debug)]
pub struct PaddingNode {
    padding: EdgeInsets,
    state: NodeState,
}

impl PaddingNode {
    /// Creates a node that pads its content by `padding`, in logical pixels.
    pub fn new(padding: EdgeInsets) -> Self {
        Self {
            padding,
            state: NodeState::new(),
        }
    }

    /// Returns the padding on each edge, in logical pixels.
    pub fn padding(&self) -> EdgeInsets {
        self.padding
    }
}

impl DelegatableNode for PaddingNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl_layout_modifier_node!(PaddingNode, invalidate = InvalidationKind::Layout);

impl LayoutModifierNode for PaddingNode {
    fn measure_hold(
        &self,
        density: f32,
        _constraints: Constraints,
        size: Size,
        wrapped: cranpose_ui_layout::WrappedHold,
    ) -> Option<cranpose_ui_layout::ConstraintsHold> {
        let inner = wrapped.hold?;
        let padding = device_padding(self.padding, density);
        let horizontal = padding.horizontal_sum();
        let vertical = padding.vertical_sum();
        // A padded size the bounds cut changes with them.
        if size.width != wrapped.size.width + horizontal
            || size.height != wrapped.size.height + vertical
        {
            return None;
        }
        cranpose_ui_layout::ConstraintsHold {
            width: inner.width.outset(horizontal),
            height: inner.height.outset(vertical),
        }
        .intersect(cranpose_ui_layout::ConstraintsHold::sized(
            size.width,
            size.height,
        ))
    }

    fn measure(
        &self,
        context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        let padding = device_padding(self.padding, context.density());
        let horizontal_padding = padding.horizontal_sum();
        let vertical_padding = padding.vertical_sum();

        let inner_constraints = Constraints {
            min_width: (constraints.min_width - horizontal_padding).max(0.0),
            max_width: (constraints.max_width - horizontal_padding).max(0.0),
            min_height: (constraints.min_height - vertical_padding).max(0.0),
            max_height: (constraints.max_height - vertical_padding).max(0.0),
        };

        let inner_placeable = measurable.measure(inner_constraints);
        let inner_width = inner_placeable.width();
        let inner_height = inner_placeable.height();

        let (width, height) = constraints.constrain(
            inner_width + horizontal_padding,
            inner_height + vertical_padding,
        );

        cranpose_ui_layout::LayoutModifierMeasureResult::new(
            Size { width, height },
            padding.left,
            padding.top,
        )
    }

    fn min_intrinsic_width(&self, measurable: &dyn Measurable, height: f32, density: f32) -> f32 {
        let padding = device_padding(self.padding, density);
        let inner_height = (height - padding.vertical_sum()).max(0.0);
        measurable.min_intrinsic_width(inner_height) + padding.horizontal_sum()
    }

    fn max_intrinsic_width(&self, measurable: &dyn Measurable, height: f32, density: f32) -> f32 {
        let padding = device_padding(self.padding, density);
        let inner_height = (height - padding.vertical_sum()).max(0.0);
        measurable.max_intrinsic_width(inner_height) + padding.horizontal_sum()
    }

    fn min_intrinsic_height(&self, measurable: &dyn Measurable, width: f32, density: f32) -> f32 {
        let padding = device_padding(self.padding, density);
        let inner_width = (width - padding.horizontal_sum()).max(0.0);
        measurable.min_intrinsic_height(inner_width) + padding.vertical_sum()
    }

    fn max_intrinsic_height(&self, measurable: &dyn Measurable, width: f32, density: f32) -> f32 {
        let padding = device_padding(self.padding, density);
        let inner_width = (width - padding.horizontal_sum()).max(0.0);
        measurable.max_intrinsic_height(inner_width) + padding.vertical_sum()
    }
}

/// `padding` with each side on a whole device pixel of `density`, as
/// Compose's `roundToPx` puts every padding side.
pub(crate) fn device_padding(padding: EdgeInsets, density: f32) -> EdgeInsets {
    use cranpose_ui_layout::round_to_px;
    EdgeInsets {
        left: round_to_px(padding.left, density),
        top: round_to_px(padding.top, density),
        right: round_to_px(padding.right, density),
        bottom: round_to_px(padding.bottom, density),
    }
}

/// Element that creates and updates padding nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct PaddingElement {
    padding: EdgeInsets,
}

impl PaddingElement {
    /// Creates an element for a node that pads its content by `padding`, in logical pixels.
    pub fn new(padding: EdgeInsets) -> Self {
        Self { padding }
    }
}

impl Hash for PaddingElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_f32_value(state, self.padding.left);
        hash_f32_value(state, self.padding.top);
        hash_f32_value(state, self.padding.right);
        hash_f32_value(state, self.padding.bottom);
    }
}

impl ModifierNodeElement for PaddingElement {
    type Node = PaddingNode;

    fn create(&self) -> Self::Node {
        PaddingNode::new(self.padding)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.padding != self.padding {
            node.padding = self.padding;
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}
