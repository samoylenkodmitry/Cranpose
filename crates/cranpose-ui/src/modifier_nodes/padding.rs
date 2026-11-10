// The padding modifier: the element that insets its content. It is a plain
// element: a chain keeps its insets as words.

use cranpose_foundation::{
    Constraints, Measurable, ModifierNodeContext, NodeCapabilities, PlainElement, PlainLayout,
    PlainLayoutFns, Size,
};

use crate::modifier::EdgeInsets;

/// Element that insets its content: a plain element, which a chain keeps
/// as words.
#[derive(Debug, Clone, PartialEq)]
pub struct PaddingElement {
    padding: EdgeInsets,
}

impl PaddingElement {
    /// Creates an element that pads its content by `padding`, in logical pixels.
    pub fn new(padding: EdgeInsets) -> Self {
        Self { padding }
    }

    /// Returns the padding on each edge, in logical pixels.
    pub fn padding(&self) -> EdgeInsets {
        self.padding
    }
}

impl PlainElement for PaddingElement {
    const WORDS: usize = 4;
    const CAPABILITIES: NodeCapabilities = NodeCapabilities::LAYOUT;
    const LAYOUT: Option<&'static PlainLayoutFns> = Some(&PlainLayoutFns::of::<Self>());

    #[inline]
    fn to_words(&self, words: &mut [u32]) {
        let EdgeInsets {
            left,
            top,
            right,
            bottom,
        } = self.padding;
        words.copy_from_slice(&[left, top, right, bottom].map(f32::to_bits));
    }

    #[inline]
    fn from_words(words: &[u32]) -> Self {
        let sides = match *words {
            [left, top, right, bottom, ..] => [left, top, right, bottom],
            _ => [0; 4],
        };
        let [left, top, right, bottom] = sides.map(f32::from_bits);
        Self {
            padding: EdgeInsets {
                left,
                top,
                right,
                bottom,
            },
        }
    }
}

impl PlainLayout for PaddingElement {
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
