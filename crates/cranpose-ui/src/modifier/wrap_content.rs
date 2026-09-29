//! Content measured at its own size along an axis, whatever minimum the parent
//! sets there, and placed in the room it leaves by an alignment: Compose's
//! `wrapContentWidth`, `wrapContentHeight` and `wrapContentSize`.

use std::hash::{Hash, Hasher};

use cranpose_foundation::{
    Constraints, DelegatableNode, InvalidationKind, LayoutModifierNode, Measurable, ModifierNode,
    ModifierNodeContext, ModifierNodeElement, NodeCapabilities, NodeState, Size,
};
use cranpose_ui_layout::{
    Alignment, HorizontalAlignment, LayoutModifierMeasureResult, VerticalAlignment,
};

use super::{Modifier, inspector_metadata};

/// The axes whose content is measured at its own size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum WrapAxes {
    /// The width only.
    Horizontal,
    /// The height only.
    Vertical,
    /// Both.
    Both,
}

impl WrapAxes {
    fn horizontal(self) -> bool {
        self != WrapAxes::Vertical
    }

    fn vertical(self) -> bool {
        self != WrapAxes::Horizontal
    }
}

/// The constraints the content is measured with: no minimum on a wrapped
/// axis, and no maximum there either when `unbounded`.
pub(crate) fn wrap_content_constraints(
    axes: WrapAxes,
    unbounded: bool,
    constraints: Constraints,
) -> Constraints {
    let (horizontal, vertical) = (axes.horizontal(), axes.vertical());
    Constraints {
        min_width: if horizontal {
            0.0
        } else {
            constraints.min_width
        },
        max_width: if horizontal && unbounded {
            f32::INFINITY
        } else {
            constraints.max_width
        },
        min_height: if vertical {
            0.0
        } else {
            constraints.min_height
        },
        max_height: if vertical && unbounded {
            f32::INFINITY
        } else {
            constraints.max_height
        },
    }
}

/// The size the wrapper takes, the content's coerced into `constraints`, and
/// where the content sits in it: aligned on a wrapped axis by `alignment` on
/// the device pixel grid of `density`, reaching outside the wrapper when it
/// is larger.
pub(crate) fn wrap_content_placement(
    axes: WrapAxes,
    alignment: Alignment,
    constraints: Constraints,
    content: Size,
    density: f32,
) -> LayoutModifierMeasureResult {
    let width = content
        .width
        .max(constraints.min_width)
        .min(constraints.max_width);
    let height = content
        .height
        .max(constraints.min_height)
        .min(constraints.max_height);
    let x = if axes.horizontal() {
        alignment.horizontal.align(width, content.width, density)
    } else {
        0.0
    };
    let y = if axes.vertical() {
        alignment.vertical.align(height, content.height, density)
    } else {
        0.0
    };
    LayoutModifierMeasureResult::new(Size { width, height }, x, y)
}

/// The element behind [`Modifier::wrap_content_size`] and its one-axis forms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WrapContentElement {
    axes: WrapAxes,
    alignment: Alignment,
    unbounded: bool,
}

impl Hash for WrapContentElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.axes.hash(state);
        self.alignment.horizontal.bias().to_bits().hash(state);
        self.alignment.vertical.bias().to_bits().hash(state);
        self.unbounded.hash(state);
    }
}

/// The layout node that measures its content at its own size.
pub struct WrapContentNode {
    axes: WrapAxes,
    alignment: Alignment,
    unbounded: bool,
    state: NodeState,
}

impl DelegatableNode for WrapContentNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

crate::modifier_nodes::impl_layout_modifier_node!(
    WrapContentNode,
    invalidate = InvalidationKind::Layout
);

impl LayoutModifierNode for WrapContentNode {
    fn measure(
        &self,
        context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> LayoutModifierMeasureResult {
        let placeable = measurable.measure(wrap_content_constraints(
            self.axes,
            self.unbounded,
            constraints,
        ));
        wrap_content_placement(
            self.axes,
            self.alignment,
            constraints,
            Size {
                width: placeable.width(),
                height: placeable.height(),
            },
            context.density(),
        )
    }
}

impl ModifierNodeElement for WrapContentElement {
    type Node = WrapContentNode;

    fn create(&self) -> Self::Node {
        WrapContentNode {
            axes: self.axes,
            alignment: self.alignment,
            unbounded: self.unbounded,
            state: NodeState::new(),
        }
    }

    fn update(&self, node: &mut Self::Node) {
        node.axes = self.axes;
        node.alignment = self.alignment;
        node.unbounded = self.unbounded;
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }

    fn update_invalidation_kind(&self) -> Option<InvalidationKind> {
        Some(InvalidationKind::Layout)
    }
}

impl Modifier {
    /// Measures the content at its own width, whatever minimum width the
    /// parent sets, and places it by `align` in the width the parent leaves.
    /// With `unbounded` the content may also be wider than the parent's
    /// maximum and reaches outside it. Compose's
    /// `Modifier.wrapContentWidth(align, unbounded)`.
    ///
    /// Example: `Modifier::empty().fill_max_width().wrap_content_width(HorizontalAlignment::End, false)`
    pub fn wrap_content_width(self, align: HorizontalAlignment, unbounded: bool) -> Self {
        self.wrap_content(
            WrapAxes::Horizontal,
            Alignment::new(align, VerticalAlignment::CenterVertically),
            unbounded,
        )
    }

    /// Measures the content at its own height, whatever minimum height the
    /// parent sets, and places it by `align` in the height the parent leaves.
    /// With `unbounded` the content may also be taller than the parent's
    /// maximum. Compose's `Modifier.wrapContentHeight(align, unbounded)`.
    pub fn wrap_content_height(self, align: VerticalAlignment, unbounded: bool) -> Self {
        self.wrap_content(
            WrapAxes::Vertical,
            Alignment::new(HorizontalAlignment::CenterHorizontally, align),
            unbounded,
        )
    }

    /// Measures the content at its own size on both axes and places it by
    /// `align`. With `unbounded` the content may be larger than the parent
    /// allows, as a fixed-size canvas scaled into a smaller window is.
    /// Compose's `Modifier.wrapContentSize(align, unbounded)`.
    ///
    /// Example: `Modifier::empty().wrap_content_size(Alignment::TOP_START, true).size_points(1280.0, 820.0)`
    pub fn wrap_content_size(self, align: Alignment, unbounded: bool) -> Self {
        self.wrap_content(WrapAxes::Both, align, unbounded)
    }

    fn wrap_content(self, axes: WrapAxes, alignment: Alignment, unbounded: bool) -> Self {
        let modifier = Self::with_element(WrapContentElement {
            axes,
            alignment,
            unbounded,
        })
        .with_inspector_metadata(inspector_metadata("wrapContent", move |info| {
            info.add_alignment("align", alignment);
            info.add_property("unbounded", format!("{unbounded}"));
        }));
        self.then(modifier)
    }
}

#[cfg(test)]
#[path = "tests/wrap_content_tests.rs"]
mod tests;
