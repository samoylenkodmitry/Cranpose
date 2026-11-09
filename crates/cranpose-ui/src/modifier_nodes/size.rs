// The size modifier: the node that bounds its content's size, and its
// element.

use std::{
    cell::Cell,
    hash::{Hash, Hasher},
};

use cranpose_foundation::{
    Constraints, DelegatableNode, InvalidationKind, LayoutModifierNode, Measurable, ModifierNode,
    ModifierNodeContext, ModifierNodeElement, NodeCapabilities, NodeState, Size,
};

use super::{hash_option_f32, impl_layout_modifier_node};

/// Node that enforces size constraints on its content.
///
/// Matches Kotlin: `SizeNode` in foundation-layout/src/commonMain/kotlin/androidx/compose/foundation/layout/Size.kt
#[derive(Debug)]
pub struct SizeNode {
    min_width: Option<f32>,
    max_width: Option<f32>,
    min_height: Option<f32>,
    max_height: Option<f32>,
    enforce_incoming: bool,
    /// The bounds rounded to the device grid of the density they were last
    /// asked for: each measure asks again for the same.
    rounded: Cell<Option<(f32, Constraints)>>,
    state: NodeState,
}

impl SizeNode {
    /// Creates a node that limits its content to the given widths and heights, in logical pixels. A
    /// `None` bound is left open. When `enforce_incoming` is true, the incoming constraints take
    /// precedence over these bounds.
    pub fn new(
        min_width: Option<f32>,
        max_width: Option<f32>,
        min_height: Option<f32>,
        max_height: Option<f32>,
        enforce_incoming: bool,
    ) -> Self {
        Self {
            min_width,
            max_width,
            min_height,
            max_height,
            enforce_incoming,
            rounded: Cell::new(None),
            state: NodeState::new(),
        }
    }

    /// The constraints the sizes ask for, each on a whole device pixel of
    /// `density` as Compose's `SizeNode` rounds them.
    fn target_constraints(&self, density: f32) -> Constraints {
        if let Some((rounded_for, target)) = self.rounded.get()
            && rounded_for == density
        {
            return target;
        }
        let target = self.round_targets(density);
        self.rounded.set(Some((density, target)));
        target
    }

    fn round_targets(&self, density: f32) -> Constraints {
        use cranpose_ui_layout::round_to_px;

        let max_width = self
            .max_width
            .map_or(f32::INFINITY, |v| round_to_px(v, density).max(0.0));
        let max_height = self
            .max_height
            .map_or(f32::INFINITY, |v| round_to_px(v, density).max(0.0));

        let min_width = self.min_width.map_or(0.0, |v| {
            let clamped = round_to_px(v, density).clamp(0.0, max_width);
            if clamped == f32::INFINITY {
                0.0
            } else {
                clamped
            }
        });

        let min_height = self.min_height.map_or(0.0, |v| {
            let clamped = round_to_px(v, density).clamp(0.0, max_height);
            if clamped == f32::INFINITY {
                0.0
            } else {
                clamped
            }
        });

        Constraints {
            min_width,
            max_width,
            min_height,
            max_height,
        }
    }

    /// Returns the smallest width in logical pixels, or `None` for no minimum.
    pub fn min_width(&self) -> Option<f32> {
        self.min_width
    }

    /// Returns the largest width in logical pixels, or `None` for no maximum.
    pub fn max_width(&self) -> Option<f32> {
        self.max_width
    }

    /// Returns the smallest height in logical pixels, or `None` for no minimum.
    pub fn min_height(&self) -> Option<f32> {
        self.min_height
    }

    /// Returns the largest height in logical pixels, or `None` for no maximum.
    pub fn max_height(&self) -> Option<f32> {
        self.max_height
    }

    /// Returns whether the incoming constraints take precedence over these bounds.
    pub fn enforce_incoming(&self) -> bool {
        self.enforce_incoming
    }
}

impl DelegatableNode for SizeNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl_layout_modifier_node!(SizeNode, invalidate = InvalidationKind::Layout);

impl LayoutModifierNode for SizeNode {
    fn measure_hold(
        &self,
        density: f32,
        _constraints: Constraints,
        size: Size,
        wrapped: cranpose_ui_layout::WrappedHold,
    ) -> Option<cranpose_ui_layout::ConstraintsHold> {
        if !self.enforce_incoming {
            return None;
        }
        let target = self.target_constraints(density);
        Some(cranpose_ui_layout::ConstraintsHold {
            width: size_axis_hold(
                (self.min_width, self.max_width),
                target.min_width,
                size.width,
                wrapped.hold.map(|hold| hold.width),
            )?,
            height: size_axis_hold(
                (self.min_height, self.max_height),
                target.min_height,
                size.height,
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
        let target = self.target_constraints(context.density());

        let wrapped_constraints = if self.enforce_incoming {
            Constraints {
                min_width: target
                    .min_width
                    .max(constraints.min_width)
                    .min(constraints.max_width),
                max_width: target
                    .max_width
                    .min(constraints.max_width)
                    .max(constraints.min_width),
                min_height: target
                    .min_height
                    .max(constraints.min_height)
                    .min(constraints.max_height),
                max_height: target
                    .max_height
                    .min(constraints.max_height)
                    .max(constraints.min_height),
            }
        } else {
            let resolved_min_width = if self.min_width.is_some() {
                target.min_width
            } else {
                constraints.min_width.min(target.max_width)
            };
            let resolved_max_width = if self.max_width.is_some() {
                target.max_width
            } else {
                constraints.max_width.max(target.min_width)
            };
            let resolved_min_height = if self.min_height.is_some() {
                target.min_height
            } else {
                constraints.min_height.min(target.max_height)
            };
            let resolved_max_height = if self.max_height.is_some() {
                target.max_height
            } else {
                constraints.max_height.max(target.min_height)
            };

            Constraints {
                min_width: resolved_min_width,
                max_width: resolved_max_width,
                min_height: resolved_min_height,
                max_height: resolved_max_height,
            }
        };

        let placeable = measurable.measure(wrapped_constraints);
        let measured_width = placeable.width();
        let measured_height = placeable.height();

        let result_width = if self.min_width.is_some()
            && self.max_width.is_some()
            && self.min_width == self.max_width
            && target.min_width >= wrapped_constraints.min_width
            && target.min_width <= wrapped_constraints.max_width
        {
            target.min_width
        } else {
            measured_width
        };

        let result_height = if self.min_height.is_some()
            && self.max_height.is_some()
            && self.min_height == self.max_height
            && target.min_height >= wrapped_constraints.min_height
            && target.min_height <= wrapped_constraints.max_height
        {
            target.min_height
        } else {
            measured_height
        };

        cranpose_ui_layout::LayoutModifierMeasureResult::with_size(Size {
            width: result_width,
            height: result_height,
        })
    }

    fn min_intrinsic_width(&self, measurable: &dyn Measurable, height: f32, density: f32) -> f32 {
        size_intrinsic(
            self.target_constraints(density),
            SizeAxis::Width,
            self.enforce_incoming,
            height,
            |h| measurable.min_intrinsic_width(h),
        )
    }

    fn max_intrinsic_width(&self, measurable: &dyn Measurable, height: f32, density: f32) -> f32 {
        size_intrinsic(
            self.target_constraints(density),
            SizeAxis::Width,
            self.enforce_incoming,
            height,
            |h| measurable.max_intrinsic_width(h),
        )
    }

    fn min_intrinsic_height(&self, measurable: &dyn Measurable, width: f32, density: f32) -> f32 {
        size_intrinsic(
            self.target_constraints(density),
            SizeAxis::Height,
            self.enforce_incoming,
            width,
            |w| measurable.min_intrinsic_height(w),
        )
    }

    fn max_intrinsic_height(&self, measurable: &dyn Measurable, width: f32, density: f32) -> f32 {
        size_intrinsic(
            self.target_constraints(density),
            SizeAxis::Height,
            self.enforce_incoming,
            width,
            |w| measurable.max_intrinsic_height(w),
        )
    }
}

/// Element that creates and updates size nodes.
///
/// Matches Kotlin: `SizeElement` in foundation-layout/src/commonMain/kotlin/androidx/compose/foundation/layout/Size.kt
#[derive(Debug, Clone, PartialEq)]
pub struct SizeElement {
    min_width: Option<f32>,
    max_width: Option<f32>,
    min_height: Option<f32>,
    max_height: Option<f32>,
    enforce_incoming: bool,
}

impl SizeElement {
    /// Creates an element that fixes the width and height, in logical pixels. A `None` length is
    /// left free. The incoming constraints take precedence.
    pub fn new(width: Option<f32>, height: Option<f32>) -> Self {
        Self {
            min_width: width,
            max_width: width,
            min_height: height,
            max_height: height,
            enforce_incoming: true,
        }
    }

    /// Creates an element with the given widths and heights, in logical pixels. A `None` bound is
    /// left open. When `enforce_incoming` is true, the incoming constraints take precedence over
    /// these bounds.
    pub fn with_constraints(
        min_width: Option<f32>,
        max_width: Option<f32>,
        min_height: Option<f32>,
        max_height: Option<f32>,
        enforce_incoming: bool,
    ) -> Self {
        Self {
            min_width,
            max_width,
            min_height,
            max_height,
            enforce_incoming,
        }
    }
}

impl Hash for SizeElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_option_f32(state, self.min_width);
        hash_option_f32(state, self.max_width);
        hash_option_f32(state, self.min_height);
        hash_option_f32(state, self.max_height);
        self.enforce_incoming.hash(state);
    }
}

impl ModifierNodeElement for SizeElement {
    type Node = SizeNode;
    const FROM_NODE: Option<fn(&SizeNode) -> Self> = Some(|node| Self {
        min_width: node.min_width,
        max_width: node.max_width,
        min_height: node.min_height,
        max_height: node.max_height,
        enforce_incoming: node.enforce_incoming,
    });

    fn create(&self) -> Self::Node {
        SizeNode::new(
            self.min_width,
            self.max_width,
            self.min_height,
            self.max_height,
            self.enforce_incoming,
        )
    }

    fn update(&self, node: &mut Self::Node) {
        if node.min_width != self.min_width
            || node.max_width != self.max_width
            || node.min_height != self.min_height
            || node.max_height != self.max_height
            || node.enforce_incoming != self.enforce_incoming
        {
            node.min_width = self.min_width;
            node.max_width = self.max_width;
            node.min_height = self.min_height;
            node.max_height = self.max_height;
            node.enforce_incoming = self.enforce_incoming;
            node.rounded.set(None);
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }

    fn update_invalidation_kind(&self) -> Option<InvalidationKind> {
        Some(InvalidationKind::Layout)
    }
}

/// One axis of what a size modifier that enforces incoming constraints
/// holds for: a fixed size that fit holds while the bounds allow it, and an
/// axis it leaves unsized passes the bounds to its content and holds what
/// the content holds.
fn size_axis_hold(
    (min, max): (Option<f32>, Option<f32>),
    target: f32,
    size: f32,
    wrapped: Option<cranpose_ui_layout::AxisHold>,
) -> Option<cranpose_ui_layout::AxisHold> {
    match (min, max) {
        (Some(min), Some(max)) if min == max && size == target => {
            Some(cranpose_ui_layout::AxisHold::sized(target))
        }
        (None, None) => wrapped,
        _ => None,
    }
}

enum SizeAxis {
    Width,
    Height,
}

fn size_intrinsic(
    target: Constraints,
    axis: SizeAxis,
    enforce_incoming: bool,
    cross: f32,
    intrinsic: impl FnOnce(f32) -> f32,
) -> f32 {
    let (target_min, target_max, cross_min, cross_max) = match axis {
        SizeAxis::Width => (
            target.min_width,
            target.max_width,
            target.min_height,
            target.max_height,
        ),
        SizeAxis::Height => (
            target.min_height,
            target.max_height,
            target.min_width,
            target.max_width,
        ),
    };
    if target_min == target_max && target_max != f32::INFINITY {
        target_max
    } else {
        let child_cross = if enforce_incoming {
            cross
        } else {
            cross.clamp(cross_min, cross_max)
        };
        intrinsic(child_cross).clamp(target_min, target_max)
    }
}
