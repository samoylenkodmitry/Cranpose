//! The target a finger or a switch scan reaches with ease: a control keeps at
//! least 48 by 48 points for a press, whatever it draws.

use cranpose_foundation::{
    Constraints, DelegatableNode, InvalidationKind, LayoutModifierNode, Measurable, ModifierNode,
    ModifierNodeContext, ModifierNodeElement, NodeCapabilities, NodeState, Size,
};
use cranpose_ui_layout::{
    HorizontalAlignment, LayoutModifierMeasureResult, VerticalAlignment, round_to_px,
};

use super::Modifier;

/// The side of the smallest target a person presses with ease: Material's
/// 48 dp, above Apple's 44 pt.
pub const MINIMUM_INTERACTIVE_SIZE: f32 = 48.0;

/// The size a control takes and where its content sits inside it: at least
/// the minimum on each side, as far as the incoming constraints allow, with
/// the content in the middle on the device pixel grid of `density`.
pub fn minimum_interactive_placement(
    constraints: Constraints,
    content: Size,
    density: f32,
) -> LayoutModifierMeasureResult {
    let minimum = round_to_px(MINIMUM_INTERACTIVE_SIZE, density);
    let width = content.width.max(
        minimum
            .max(constraints.min_width)
            .min(constraints.max_width),
    );
    let height = content.height.max(
        minimum
            .max(constraints.min_height)
            .min(constraints.max_height),
    );
    LayoutModifierMeasureResult::new(
        Size { width, height },
        HorizontalAlignment::CenterHorizontally.align(width, content.width, density),
        VerticalAlignment::CenterVertically.align(height, content.height, density),
    )
}

/// The element behind [`Modifier::minimum_interactive_component_size`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct MinimumInteractiveElement;

/// The layout node that widens a control to the minimum target size.
pub struct MinimumInteractiveNode {
    state: NodeState,
}

impl DelegatableNode for MinimumInteractiveNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

crate::modifier_nodes::impl_layout_modifier_node!(
    MinimumInteractiveNode,
    invalidate = InvalidationKind::Layout
);

impl LayoutModifierNode for MinimumInteractiveNode {
    fn measure(
        &self,
        context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> LayoutModifierMeasureResult {
        let placeable = measurable.measure(constraints);
        minimum_interactive_placement(
            constraints,
            Size {
                width: placeable.width(),
                height: placeable.height(),
            },
            context.density(),
        )
    }

    fn min_intrinsic_width(&self, measurable: &dyn Measurable, height: f32, density: f32) -> f32 {
        measurable
            .min_intrinsic_width(height)
            .max(round_to_px(MINIMUM_INTERACTIVE_SIZE, density))
    }

    fn max_intrinsic_width(&self, measurable: &dyn Measurable, height: f32, density: f32) -> f32 {
        measurable
            .max_intrinsic_width(height)
            .max(round_to_px(MINIMUM_INTERACTIVE_SIZE, density))
    }

    fn min_intrinsic_height(&self, measurable: &dyn Measurable, width: f32, density: f32) -> f32 {
        measurable
            .min_intrinsic_height(width)
            .max(round_to_px(MINIMUM_INTERACTIVE_SIZE, density))
    }

    fn max_intrinsic_height(&self, measurable: &dyn Measurable, width: f32, density: f32) -> f32 {
        measurable
            .max_intrinsic_height(width)
            .max(round_to_px(MINIMUM_INTERACTIVE_SIZE, density))
    }
}

impl ModifierNodeElement for MinimumInteractiveElement {
    type Node = MinimumInteractiveNode;

    fn create(&self) -> Self::Node {
        MinimumInteractiveNode {
            state: NodeState::new(),
        }
    }

    fn update(&self, _node: &mut Self::Node) {}

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

impl Modifier {
    /// Keeps at least 48 by 48 points for a press on this control, as far as
    /// the parent allows, and puts the drawn content in the middle. A small
    /// icon or a checkbox stays easy to hit with a finger, a switch scan or a
    /// shaky hand; the drawn size does not change. Compose's
    /// `Modifier.minimumInteractiveComponentSize()`.
    pub fn minimum_interactive_component_size(self) -> Self {
        self.then(Self::with_element(MinimumInteractiveElement))
    }
}

#[cfg(test)]
#[path = "tests/minimum_interactive_tests.rs"]
mod tests;
