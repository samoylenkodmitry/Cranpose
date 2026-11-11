// The offset modifiers, by points and by fractions of the content's size:
// their nodes and elements.

use std::hash::{Hash, Hasher};

use cranpose_foundation::{
    Constraints, DelegatableNode, InvalidationKind, LayoutModifierNode, Measurable, ModifierNode,
    ModifierNodeContext, ModifierNodeElement, NodeCapabilities, NodeState, PlainElement,
    PlainLayout, PlainLayoutFns, PrepareAt, Size,
};
use cranpose_ui_layout::{LayoutDirection, round_to_px};

use super::{hash_f32_value, impl_layout_modifier_node, measure_pass_through};
use crate::modifier::Point;

/// Element that offsets its content by a fixed (x, y) amount.
///
/// Matches Kotlin: `OffsetElement` in foundation-layout/src/commonMain/kotlin/androidx/compose/foundation/layout/Offset.kt
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OffsetElement {
    x: f32,
    y: f32,
    flags: u32,
}

// A right-to-left layout direction mirrors `x`.
const RTL_AWARE: u32 = 1;
// A coordinator mirrored the offset for its direction and put it on the
// device pixels of its density already: see `PlainLayout::prepared`.
const PLACED: u32 = 1 << 1;

impl OffsetElement {
    /// Creates an element that moves content by `x` and `y`, in logical pixels. With `rtl_aware`
    /// set, a right-to-left layout direction mirrors `x`.
    pub fn new(x: f32, y: f32, rtl_aware: bool) -> Self {
        Self {
            x,
            y,
            flags: if rtl_aware { RTL_AWARE } else { 0 },
        }
    }

    /// Returns the offset in logical pixels as a layout of `direction` applies it: `x` mirrored
    /// when the element is RTL aware and the direction is right-to-left.
    pub fn offset(&self, direction: LayoutDirection) -> Point {
        Point {
            x: if self.rtl_aware() && direction.is_rtl() {
                -self.x
            } else {
                self.x
            },
            y: self.y,
        }
    }

    /// Returns whether a right-to-left layout direction mirrors `x`.
    pub fn rtl_aware(&self) -> bool {
        self.flags & RTL_AWARE != 0
    }

    // Where the element places its content: on a whole device pixel of
    // `at`'s density, where Compose's `OffsetNode` places it.
    pub(crate) fn placed(&self, at: PrepareAt) -> Point {
        if self.flags & PLACED != 0 {
            return Point {
                x: self.x,
                y: self.y,
            };
        }
        let offset = self.offset(at.direction);
        Point {
            x: round_to_px(offset.x, at.density),
            y: round_to_px(offset.y, at.density),
        }
    }
}

impl PlainElement for OffsetElement {
    const WORDS: usize = 3;
    const CAPABILITIES: NodeCapabilities = NodeCapabilities::LAYOUT;
    const UPDATE_INVALIDATION: Option<InvalidationKind> = Some(InvalidationKind::Layout);
    const LAYOUT: Option<&'static PlainLayoutFns> = Some(&PlainLayoutFns::of::<Self>());

    #[inline]
    fn to_words(&self, words: &mut [u32]) {
        words.copy_from_slice(&[self.x.to_bits(), self.y.to_bits(), self.flags]);
    }

    #[inline]
    fn from_words(words: &[u32]) -> Self {
        match *words {
            [x, y, flags, ..] => Self {
                x: f32::from_bits(x),
                y: f32::from_bits(y),
                flags,
            },
            _ => Self::new(0.0, 0.0, false),
        }
    }
}

impl PlainLayout for OffsetElement {
    const PREPARES: bool = true;

    fn prepared(self, at: PrepareAt) -> Self {
        let Point { x, y } = self.placed(at);
        Self {
            x,
            y,
            flags: PLACED,
        }
    }

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
        // An element no coordinator prepared knows no direction.
        let offset = self.placed(PrepareAt {
            density: context.density(),
            direction: LayoutDirection::Ltr,
        });
        measure_pass_through(measurable, constraints, |_| (offset.x, offset.y))
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
    const FROM_NODE: Option<fn(&FractionalOffsetNode) -> Self> = Some(|node| Self {
        x_fraction: node.x_fraction,
        y_fraction: node.y_fraction,
    });

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
