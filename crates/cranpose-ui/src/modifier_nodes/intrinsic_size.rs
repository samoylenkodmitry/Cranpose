// The intrinsic size modifiers: the node that sizes its content to an
// intrinsic size, and its element.

use std::hash::{Hash, Hasher};

use cranpose_foundation::{
    Constraints, DelegatableNode, InvalidationKind, LayoutModifierNode, Measurable, ModifierNode,
    ModifierNodeContext, ModifierNodeElement, NodeCapabilities, NodeState, Size,
};
use cranpose_ui_layout::IntrinsicSize;

use super::impl_layout_modifier_node;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum IntrinsicAxis {
    Width,
    Height,
}

#[derive(Debug)]
pub(crate) struct IntrinsicSizeNode {
    axis: IntrinsicAxis,
    size: IntrinsicSize,
    state: NodeState,
}

impl IntrinsicSizeNode {
    pub(crate) fn new(axis: IntrinsicAxis, size: IntrinsicSize) -> Self {
        Self {
            axis,
            size,
            state: NodeState::new(),
        }
    }

    /// The content's min or max intrinsic size along this node's axis, at
    /// the other axis' limit. Matches Kotlin: `calculateContentConstraints`
    /// in foundation-layout's `Intrinsic.kt`.
    fn content_extent(&self, measurable: &dyn Measurable, constraints: Constraints) -> f32 {
        let extent = match (self.axis, self.size) {
            (IntrinsicAxis::Width, IntrinsicSize::Min) => {
                measurable.min_intrinsic_width(constraints.max_height)
            }
            (IntrinsicAxis::Width, IntrinsicSize::Max) => {
                measurable.max_intrinsic_width(constraints.max_height)
            }
            (IntrinsicAxis::Height, IntrinsicSize::Min) => {
                measurable.min_intrinsic_height(constraints.max_width)
            }
            (IntrinsicAxis::Height, IntrinsicSize::Max) => {
                measurable.max_intrinsic_height(constraints.max_width)
            }
        };
        extent.max(0.0)
    }

    /// Along its own axis the node reports the one intrinsic size it lays
    /// out at, for both min and max; across it the content's pass through.
    fn intrinsic_width(&self, measurable: &dyn Measurable, height: f32) -> Option<f32> {
        (self.axis == IntrinsicAxis::Width).then(|| match self.size {
            IntrinsicSize::Min => measurable.min_intrinsic_width(height),
            IntrinsicSize::Max => measurable.max_intrinsic_width(height),
        })
    }

    fn intrinsic_height(&self, measurable: &dyn Measurable, width: f32) -> Option<f32> {
        (self.axis == IntrinsicAxis::Height).then(|| match self.size {
            IntrinsicSize::Min => measurable.min_intrinsic_height(width),
            IntrinsicSize::Max => measurable.max_intrinsic_height(width),
        })
    }
}

impl DelegatableNode for IntrinsicSizeNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl_layout_modifier_node!(IntrinsicSizeNode, invalidate = InvalidationKind::Layout);

/// Measures the content at its intrinsic size along the node's axis, kept
/// within the incoming constraints, and leaves the other axis as it came.
/// Matches Kotlin: `IntrinsicWidthNode` and `IntrinsicHeightNode` with
/// `enforceIncoming = true`.
impl LayoutModifierNode for IntrinsicSizeNode {
    fn measure(
        &self,
        _context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        let extent = self.content_extent(measurable, constraints);
        let content = match self.axis {
            IntrinsicAxis::Width => {
                let width = extent.max(constraints.min_width).min(constraints.max_width);
                Constraints {
                    min_width: width,
                    max_width: width,
                    ..constraints
                }
            }
            IntrinsicAxis::Height => {
                let height = extent
                    .max(constraints.min_height)
                    .min(constraints.max_height);
                Constraints {
                    min_height: height,
                    max_height: height,
                    ..constraints
                }
            }
        };
        let placeable = measurable.measure(content);
        cranpose_ui_layout::LayoutModifierMeasureResult::with_size(Size {
            width: placeable.width(),
            height: placeable.height(),
        })
    }

    fn min_intrinsic_width(&self, measurable: &dyn Measurable, height: f32, _density: f32) -> f32 {
        self.intrinsic_width(measurable, height)
            .unwrap_or_else(|| measurable.min_intrinsic_width(height))
    }

    fn max_intrinsic_width(&self, measurable: &dyn Measurable, height: f32, _density: f32) -> f32 {
        self.intrinsic_width(measurable, height)
            .unwrap_or_else(|| measurable.max_intrinsic_width(height))
    }

    fn min_intrinsic_height(&self, measurable: &dyn Measurable, width: f32, _density: f32) -> f32 {
        self.intrinsic_height(measurable, width)
            .unwrap_or_else(|| measurable.min_intrinsic_height(width))
    }

    fn max_intrinsic_height(&self, measurable: &dyn Measurable, width: f32, _density: f32) -> f32 {
        self.intrinsic_height(measurable, width)
            .unwrap_or_else(|| measurable.max_intrinsic_height(width))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IntrinsicSizeElement {
    axis: IntrinsicAxis,
    size: IntrinsicSize,
}

impl IntrinsicSizeElement {
    pub(crate) fn width(size: IntrinsicSize) -> Self {
        Self {
            axis: IntrinsicAxis::Width,
            size,
        }
    }

    pub(crate) fn height(size: IntrinsicSize) -> Self {
        Self {
            axis: IntrinsicAxis::Height,
            size,
        }
    }
}

impl Hash for IntrinsicSizeElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u8(match self.axis {
            IntrinsicAxis::Width => 0,
            IntrinsicAxis::Height => 1,
        });
        state.write_u8(match self.size {
            IntrinsicSize::Min => 0,
            IntrinsicSize::Max => 1,
        });
    }
}

impl ModifierNodeElement for IntrinsicSizeElement {
    type Node = IntrinsicSizeNode;
    const FROM_NODE: Option<fn(&IntrinsicSizeNode) -> Self> = Some(|node| Self {
        axis: node.axis,
        size: node.size,
    });

    fn create(&self) -> Self::Node {
        IntrinsicSizeNode::new(self.axis, self.size)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.axis != self.axis {
            node.axis = self.axis;
        }
        if node.size != self.size {
            node.size = self.size;
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}
