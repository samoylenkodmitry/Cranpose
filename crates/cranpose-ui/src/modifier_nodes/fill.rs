// The fill modifiers: the node that fills a fraction of the room on an
// axis, and its element.

use std::hash::{Hash, Hasher};

use cranpose_foundation::{
    Constraints, DelegatableNode, InvalidationKind, LayoutModifierNode, Measurable, ModifierNode,
    ModifierNodeContext, ModifierNodeElement, NodeCapabilities, NodeState, Size,
};

use super::{hash_f32_value, impl_layout_modifier_node};

/// Direction for fill modifiers (horizontal, vertical, or both).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FillDirection {
    /// Fill the available width.
    Horizontal,
    /// Fill the available height.
    Vertical,
    /// Fill the available width and height.
    Both,
}

/// Node that fills the maximum available space in one or both dimensions.
///
/// Matches Kotlin: `FillNode` in foundation-layout/src/commonMain/kotlin/androidx/compose/foundation/layout/Size.kt
#[derive(Debug)]
pub struct FillNode {
    direction: FillDirection,
    fraction: f32,
    state: NodeState,
}

impl FillNode {
    /// Creates a node that fills `fraction` of the available space along `direction`.
    pub fn new(direction: FillDirection, fraction: f32) -> Self {
        Self {
            direction,
            fraction,
            state: NodeState::new(),
        }
    }

    /// Returns the axes the node fills.
    pub fn direction(&self) -> FillDirection {
        self.direction
    }

    /// Returns the share of the available space the node fills, from 0.0 to 1.0.
    pub fn fraction(&self) -> f32 {
        self.fraction
    }

    /// The extent the content gets of a given `extent` across the axis that
    /// `other` fills: the filled fraction, unless only `other` fills.
    fn filled(&self, extent: f32, other: FillDirection, density: f32) -> f32 {
        if self.direction == other || !extent.is_finite() {
            return extent;
        }
        cranpose_ui_layout::round_to_px(extent * self.fraction, density)
    }
}

impl DelegatableNode for FillNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl_layout_modifier_node!(FillNode, invalidate = InvalidationKind::Layout);

impl LayoutModifierNode for FillNode {
    /// A filled axis holds while the same max fills it the same; an axis
    /// the node leaves alone passes the bounds on and holds what its content
    /// holds.
    fn measure_hold(
        &self,
        density: f32,
        constraints: Constraints,
        size: Size,
        wrapped: cranpose_ui_layout::WrappedHold,
    ) -> Option<cranpose_ui_layout::ConstraintsHold> {
        Some(cranpose_ui_layout::ConstraintsHold {
            width: fill_axis_hold(
                self.direction != FillDirection::Vertical,
                (constraints.max_width, size.width),
                (self.fraction, density),
                wrapped.hold.map(|hold| hold.width),
            )?,
            height: fill_axis_hold(
                self.direction != FillDirection::Horizontal,
                (constraints.max_height, size.height),
                (self.fraction, density),
                wrapped.hold.map(|hold| hold.height),
            )?,
        })
    }

    fn measure(
        &self,
        context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        let density = context.density();
        let (fill_width, child_min_width, child_max_width) = if self.direction
            != FillDirection::Vertical
            && constraints.max_width != f32::INFINITY
        {
            let width =
                cranpose_ui_layout::round_to_px(constraints.max_width * self.fraction, density)
                    .clamp(constraints.min_width, constraints.max_width);
            (width, width, width)
        } else {
            (
                constraints.max_width,
                constraints.min_width,
                constraints.max_width,
            )
        };

        let (fill_height, child_min_height, child_max_height) = if self.direction
            != FillDirection::Horizontal
            && constraints.max_height != f32::INFINITY
        {
            let height =
                cranpose_ui_layout::round_to_px(constraints.max_height * self.fraction, density)
                    .clamp(constraints.min_height, constraints.max_height);
            (height, height, height)
        } else {
            (
                constraints.max_height,
                constraints.min_height,
                constraints.max_height,
            )
        };

        let fill_constraints = Constraints {
            min_width: child_min_width,
            max_width: child_max_width,
            min_height: child_min_height,
            max_height: child_max_height,
        };

        let placeable = measurable.measure(fill_constraints);

        let result_width = if self.direction != FillDirection::Vertical
            && constraints.max_width != f32::INFINITY
        {
            fill_width
        } else {
            placeable.width()
        };

        let result_height = if self.direction != FillDirection::Horizontal
            && constraints.max_height != f32::INFINITY
        {
            fill_height
        } else {
            placeable.height()
        };

        cranpose_ui_layout::LayoutModifierMeasureResult::with_size(Size {
            width: result_width,
            height: result_height,
        })
    }

    fn min_intrinsic_width(&self, measurable: &dyn Measurable, height: f32, density: f32) -> f32 {
        measurable.min_intrinsic_width(self.filled(height, FillDirection::Horizontal, density))
    }

    fn max_intrinsic_width(&self, measurable: &dyn Measurable, height: f32, density: f32) -> f32 {
        measurable.max_intrinsic_width(self.filled(height, FillDirection::Horizontal, density))
    }

    fn min_intrinsic_height(&self, measurable: &dyn Measurable, width: f32, density: f32) -> f32 {
        measurable.min_intrinsic_height(self.filled(width, FillDirection::Vertical, density))
    }

    fn max_intrinsic_height(&self, measurable: &dyn Measurable, width: f32, density: f32) -> f32 {
        measurable.max_intrinsic_height(self.filled(width, FillDirection::Vertical, density))
    }
}

/// Element that creates and updates fill nodes.
///
/// Matches Kotlin: `FillElement` in foundation-layout/src/commonMain/kotlin/androidx/compose/foundation/layout/Size.kt
#[derive(Debug, Clone, PartialEq)]
pub struct FillElement {
    direction: FillDirection,
    fraction: f32,
}

impl FillElement {
    /// Creates an element that fills `fraction` of the available width.
    pub fn width(fraction: f32) -> Self {
        Self {
            direction: FillDirection::Horizontal,
            fraction,
        }
    }

    /// Creates an element that fills `fraction` of the available height.
    pub fn height(fraction: f32) -> Self {
        Self {
            direction: FillDirection::Vertical,
            fraction,
        }
    }

    /// Creates an element that fills `fraction` of the available width and height.
    pub fn size(fraction: f32) -> Self {
        Self {
            direction: FillDirection::Both,
            fraction,
        }
    }
}

impl Hash for FillElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.direction.hash(state);
        hash_f32_value(state, self.fraction);
    }
}

impl ModifierNodeElement for FillElement {
    type Node = FillNode;

    fn create(&self) -> Self::Node {
        FillNode::new(self.direction, self.fraction)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.direction != self.direction || node.fraction != self.fraction {
            node.direction = self.direction;
            node.fraction = self.fraction;
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

/// One axis of what a fill modifier holds for: see
/// [`FillNode::measure_hold`].
fn fill_axis_hold(
    fills: bool,
    (max, size): (f32, f32),
    (fraction, density): (f32, f32),
    wrapped: Option<cranpose_ui_layout::AxisHold>,
) -> Option<cranpose_ui_layout::AxisHold> {
    use cranpose_ui_layout::{AxisHold, BoundRange};
    if !fills {
        return wrapped;
    }
    if max == f32::INFINITY {
        let wrapped = wrapped?;
        return Some(AxisHold {
            min: wrapped.min,
            max: wrapped.max.intersect(BoundRange::exactly(f32::INFINITY))?,
        });
    }
    (size == cranpose_ui_layout::round_to_px(max * fraction, density)).then_some(AxisHold {
        min: BoundRange::up_to(size),
        max: BoundRange::exactly(max),
    })
}
