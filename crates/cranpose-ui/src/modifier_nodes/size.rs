// The size modifier: the element that bounds its content's size. It is a
// plain element: a chain keeps its bounds as words.

use cranpose_foundation::{
    Constraints, InvalidationKind, Measurable, ModifierNodeContext, NodeCapabilities, PlainElement,
    PlainIntrinsic, PlainLayout, PlainLayoutFns, PrepareAt, Size,
};

// Which of a size element's bounds a slot of its values holds, and the bit
// of its flags that says the slot holds a bound.
const MIN_WIDTH: usize = 0;
const MAX_WIDTH: usize = 1;
const MIN_HEIGHT: usize = 2;
const MAX_HEIGHT: usize = 3;
// The incoming constraints take precedence over the bounds.
const ENFORCE_INCOMING: u32 = 1 << 4;
// The values are the constraints the bounds ask for at a coordinator's
// density, each on a whole device pixel; the two bits after tell whether
// the bounds fixed the width and the height.
const PREPARED: u32 = 1 << 5;
const FIXED_WIDTH: u32 = 1 << 6;
const FIXED_HEIGHT: u32 = 1 << 7;

/// Element that bounds the size of its content: a plain element, which a
/// chain keeps as words.
///
/// Matches Kotlin: `SizeElement` in foundation-layout/src/commonMain/kotlin/androidx/compose/foundation/layout/Size.kt
#[derive(Debug, Clone, PartialEq)]
pub struct SizeElement {
    // The bounds in logical pixels, 0 for one left open.
    values: [f32; 4],
    flags: u32,
}

impl SizeElement {
    /// Creates an element that fixes the width and height, in logical pixels. A `None` length is
    /// left free. The incoming constraints take precedence.
    pub fn new(width: Option<f32>, height: Option<f32>) -> Self {
        Self::with_constraints(width, width, height, height, true)
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
        let mut values = [0.0; 4];
        let mut flags = if enforce_incoming {
            ENFORCE_INCOMING
        } else {
            0
        };
        let bounds = [min_width, max_width, min_height, max_height];
        for (slot, bound) in bounds.into_iter().enumerate() {
            if let Some(value) = bound {
                values[slot] = value;
                flags |= 1 << slot;
            }
        }
        Self { values, flags }
    }

    fn bound(&self, slot: usize) -> Option<f32> {
        (self.flags & (1 << slot) != 0).then_some(self.values[slot])
    }

    /// Returns the smallest width in logical pixels, or `None` for no minimum.
    pub fn min_width(&self) -> Option<f32> {
        self.bound(MIN_WIDTH)
    }

    /// Returns the largest width in logical pixels, or `None` for no maximum.
    pub fn max_width(&self) -> Option<f32> {
        self.bound(MAX_WIDTH)
    }

    /// Returns the smallest height in logical pixels, or `None` for no minimum.
    pub fn min_height(&self) -> Option<f32> {
        self.bound(MIN_HEIGHT)
    }

    /// Returns the largest height in logical pixels, or `None` for no maximum.
    pub fn max_height(&self) -> Option<f32> {
        self.bound(MAX_HEIGHT)
    }

    /// Returns whether the incoming constraints take precedence over these bounds.
    pub fn enforce_incoming(&self) -> bool {
        self.flags & ENFORCE_INCOMING != 0
    }

    // Whether the bounds of the axis whose slots are `min` and `max` fix
    // its size: both set, and equal.
    fn fixes(&self, min: usize, max: usize, fixed: u32) -> bool {
        if self.flags & PREPARED != 0 {
            return self.flags & fixed != 0;
        }
        self.bound(min).is_some() && self.bound(min) == self.bound(max)
    }

    fn fixes_width(&self) -> bool {
        self.fixes(MIN_WIDTH, MAX_WIDTH, FIXED_WIDTH)
    }

    fn fixes_height(&self) -> bool {
        self.fixes(MIN_HEIGHT, MAX_HEIGHT, FIXED_HEIGHT)
    }

    // Whether no bound of the axis whose slots are `min` and `max` is set.
    fn frees(&self, min: usize, max: usize) -> bool {
        self.flags & ((1 << min) | (1 << max)) == 0
    }

    // The constraints the sizes ask for, each on a whole device pixel of
    // `density` as Compose's `SizeNode` rounds them: those a coordinator
    // prepared, or worked out here.
    fn target_constraints(&self, density: f32) -> Constraints {
        if self.flags & PREPARED != 0 {
            let [min_width, max_width, min_height, max_height] = self.values;
            return Constraints {
                min_width,
                max_width,
                min_height,
                max_height,
            };
        }
        self.round_targets(density)
    }

    fn round_targets(&self, density: f32) -> Constraints {
        use cranpose_ui_layout::round_to_px;

        let max_width = self
            .max_width()
            .map_or(f32::INFINITY, |v| round_to_px(v, density).max(0.0));
        let max_height = self
            .max_height()
            .map_or(f32::INFINITY, |v| round_to_px(v, density).max(0.0));

        let min_width = self.min_width().map_or(0.0, |v| {
            let clamped = round_to_px(v, density).clamp(0.0, max_width);
            if clamped == f32::INFINITY {
                0.0
            } else {
                clamped
            }
        });

        let min_height = self.min_height().map_or(0.0, |v| {
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
}

impl PlainElement for SizeElement {
    const WORDS: usize = 5;
    const CAPABILITIES: NodeCapabilities = NodeCapabilities::LAYOUT;
    const UPDATE_INVALIDATION: Option<InvalidationKind> = Some(InvalidationKind::Layout);
    const LAYOUT: Option<&'static PlainLayoutFns> = Some(&PlainLayoutFns::of::<Self>());

    #[inline]
    fn to_words(&self, words: &mut [u32]) {
        let [a, b, c, d] = self.values.map(f32::to_bits);
        words.copy_from_slice(&[a, b, c, d, self.flags]);
    }

    // One length check for the five words, where an index each would
    // check five times. A coordinator hands more words than the element's.
    #[inline]
    fn from_words(words: &[u32]) -> Self {
        let (values, flags) = match *words {
            [a, b, c, d, flags, ..] => ([a, b, c, d], flags),
            _ => ([0; 4], 0),
        };
        Self {
            values: values.map(f32::from_bits),
            flags,
        }
    }
}

impl PlainLayout for SizeElement {
    const PREPARES: bool = true;

    fn prepared(self, at: PrepareAt) -> Self {
        let target = self.round_targets(at.density);
        let mut flags = self.flags | PREPARED;
        if self.fixes_width() {
            flags |= FIXED_WIDTH;
        }
        if self.fixes_height() {
            flags |= FIXED_HEIGHT;
        }
        Self {
            values: [
                target.min_width,
                target.max_width,
                target.min_height,
                target.max_height,
            ],
            flags,
        }
    }

    fn measure_hold(
        &self,
        density: f32,
        _constraints: Constraints,
        size: Size,
        wrapped: cranpose_ui_layout::WrappedHold,
    ) -> Option<cranpose_ui_layout::ConstraintsHold> {
        if !self.enforce_incoming() {
            return None;
        }
        let target = self.target_constraints(density);
        Some(cranpose_ui_layout::ConstraintsHold {
            width: size_axis_hold(
                (self.fixes_width(), self.frees(MIN_WIDTH, MAX_WIDTH)),
                target.min_width,
                size.width,
                wrapped.hold.map(|hold| hold.width),
            )?,
            height: size_axis_hold(
                (self.fixes_height(), self.frees(MIN_HEIGHT, MAX_HEIGHT)),
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

        let wrapped_constraints = if self.enforce_incoming() {
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
            let resolved_min_width = if self.min_width().is_some() {
                target.min_width
            } else {
                constraints.min_width.min(target.max_width)
            };
            let resolved_max_width = if self.max_width().is_some() {
                target.max_width
            } else {
                constraints.max_width.max(target.min_width)
            };
            let resolved_min_height = if self.min_height().is_some() {
                target.min_height
            } else {
                constraints.min_height.min(target.max_height)
            };
            let resolved_max_height = if self.max_height().is_some() {
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

        let result_width = if self.fixes_width()
            && target.min_width >= wrapped_constraints.min_width
            && target.min_width <= wrapped_constraints.max_width
        {
            target.min_width
        } else {
            measured_width
        };

        let result_height = if self.fixes_height()
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

    fn intrinsic(&self, measurable: &dyn Measurable, kind: PlainIntrinsic, density: f32) -> f32 {
        let axis = if kind.is_width() {
            SizeAxis::Width
        } else {
            SizeAxis::Height
        };
        size_intrinsic(
            self.target_constraints(density),
            axis,
            self.enforce_incoming(),
            kind.cross(),
            |cross| kind.at(cross).of(measurable),
        )
    }
}

// One axis of what a size modifier that enforces incoming constraints
// holds for: a fixed size that fit holds while the bounds allow it, and an
// axis it leaves unsized, as `free` tells, passes the bounds to its content
// and holds what the content holds.
fn size_axis_hold(
    (fixed, free): (bool, bool),
    target: f32,
    size: f32,
    wrapped: Option<cranpose_ui_layout::AxisHold>,
) -> Option<cranpose_ui_layout::AxisHold> {
    if fixed && size == target {
        Some(cranpose_ui_layout::AxisHold::sized(target))
    } else if free {
        wrapped
    } else {
        None
    }
}

#[derive(Clone, Copy)]
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
