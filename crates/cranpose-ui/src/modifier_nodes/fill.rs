// The fill modifiers: the element that fills a fraction of the room on an
// axis. It is a plain element: a chain keeps its values as words.

use std::hash::Hash;

use cranpose_foundation::{
    Constraints, Measurable, ModifierNodeContext, NodeCapabilities, PlainElement, PlainIntrinsic,
    PlainLayout, PlainLayoutFns, Size,
};

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

/// Element that fills a fraction of the room on an axis: a plain element,
/// which a chain keeps as words.
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

    /// Returns the axis or axes the element fills.
    pub fn direction(&self) -> FillDirection {
        self.direction
    }

    /// Returns the fraction of the available room the element fills.
    pub fn fraction(&self) -> f32 {
        self.fraction
    }

    // Whether the element fills the width, and whether the height.
    pub(crate) fn fills(&self) -> (bool, bool) {
        match self.direction {
            FillDirection::Horizontal => (true, false),
            FillDirection::Vertical => (false, true),
            FillDirection::Both => (true, true),
        }
    }

    // The extent the content gets of a given `extent` across the axis that
    // `other` fills: the filled fraction, unless only `other` fills.
    fn filled(&self, extent: f32, other: FillDirection, density: f32) -> f32 {
        if self.direction == other || !extent.is_finite() {
            return extent;
        }
        cranpose_ui_layout::round_to_px(extent * self.fraction, density)
    }
}

impl PlainElement for FillElement {
    const WORDS: usize = 2;
    const CAPABILITIES: NodeCapabilities = NodeCapabilities::LAYOUT;
    const LAYOUT: Option<&'static PlainLayoutFns> = Some(&PlainLayoutFns::of::<Self>());

    #[inline]
    fn to_words(&self, words: &mut [u32]) {
        words[0] = match self.direction {
            FillDirection::Horizontal => 0,
            FillDirection::Vertical => 1,
            FillDirection::Both => 2,
        };
        words[1] = self.fraction.to_bits();
    }

    #[inline]
    fn from_words(words: &[u32]) -> Self {
        let (direction, fraction) = match *words {
            [direction, fraction, ..] => (direction, fraction),
            _ => (2, 0),
        };
        Self {
            direction: match direction {
                0 => FillDirection::Horizontal,
                1 => FillDirection::Vertical,
                _ => FillDirection::Both,
            },
            fraction: f32::from_bits(fraction),
        }
    }
}

impl PlainLayout for FillElement {
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

    fn intrinsic(&self, measurable: &dyn Measurable, kind: PlainIntrinsic, density: f32) -> f32 {
        let other = if kind.is_width() {
            FillDirection::Horizontal
        } else {
            FillDirection::Vertical
        };
        kind.at(self.filled(kind.cross(), other, density))
            .of(measurable)
    }
}

/// One axis of what a fill modifier holds for: see
/// `FillElement::measure_hold`.
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
