// The intrinsic size modifier: the element that sizes its content to an
// intrinsic size.

use cranpose_foundation::{
    Constraints, Measurable, ModifierNodeContext, NodeCapabilities, PlainElement, PlainIntrinsic,
    PlainLayout, PlainLayoutFns, Size,
};
use cranpose_ui_layout::IntrinsicSize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IntrinsicAxis {
    Width,
    Height,
}

// Measures the content at its intrinsic size along the element's axis, kept
// within the incoming constraints, and leaves the other axis as it came.
// Matches Kotlin: `IntrinsicWidthNode` and `IntrinsicHeightNode` with
// `enforceIncoming = true`.
#[derive(Debug, Clone, Copy, PartialEq)]
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

    // The one intrinsic size the element lays its content out at, asked at
    // `cross`, the limit of the other axis. Matches Kotlin:
    // `calculateContentConstraints` in foundation-layout's `Intrinsic.kt`.
    fn along(self, cross: f32) -> PlainIntrinsic {
        match (self.axis, self.size) {
            (IntrinsicAxis::Width, IntrinsicSize::Min) => PlainIntrinsic::MinWidth(cross),
            (IntrinsicAxis::Width, IntrinsicSize::Max) => PlainIntrinsic::MaxWidth(cross),
            (IntrinsicAxis::Height, IntrinsicSize::Min) => PlainIntrinsic::MinHeight(cross),
            (IntrinsicAxis::Height, IntrinsicSize::Max) => PlainIntrinsic::MaxHeight(cross),
        }
    }
}

impl PlainElement for IntrinsicSizeElement {
    const WORDS: usize = 2;
    const CAPABILITIES: NodeCapabilities = NodeCapabilities::LAYOUT;
    const LAYOUT: Option<&'static PlainLayoutFns> = Some(&PlainLayoutFns::of::<Self>());

    #[inline]
    fn to_words(&self, words: &mut [u32]) {
        words.copy_from_slice(&[
            match self.axis {
                IntrinsicAxis::Width => 0,
                IntrinsicAxis::Height => 1,
            },
            match self.size {
                IntrinsicSize::Min => 0,
                IntrinsicSize::Max => 1,
            },
        ]);
    }

    #[inline]
    fn from_words(words: &[u32]) -> Self {
        let (axis, size) = match *words {
            [axis, size, ..] => (axis, size),
            _ => (0, 0),
        };
        Self {
            axis: if axis == 0 {
                IntrinsicAxis::Width
            } else {
                IntrinsicAxis::Height
            },
            size: if size == 0 {
                IntrinsicSize::Min
            } else {
                IntrinsicSize::Max
            },
        }
    }
}

impl PlainLayout for IntrinsicSizeElement {
    fn measure(
        &self,
        _context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        let content = match self.axis {
            IntrinsicAxis::Width => {
                let extent = self.along(constraints.max_height).of(measurable).max(0.0);
                let width = extent.max(constraints.min_width).min(constraints.max_width);
                Constraints {
                    min_width: width,
                    max_width: width,
                    ..constraints
                }
            }
            IntrinsicAxis::Height => {
                let extent = self.along(constraints.max_width).of(measurable).max(0.0);
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

    // Along its own axis the element reports the one intrinsic size it lays
    // out at, for both min and max; across it the content's pass through.
    fn intrinsic(&self, measurable: &dyn Measurable, kind: PlainIntrinsic, _density: f32) -> f32 {
        if kind.is_width() == (self.axis == IntrinsicAxis::Width) {
            self.along(kind.cross()).of(measurable)
        } else {
            kind.of(measurable)
        }
    }
}
