//! Offset modifier implementation following Jetpack Compose's layout/Offset.kt
//!
//! Reference: /media/huge/composerepo/compose/foundation/foundation-layout/src/commonMain/kotlin/androidx/compose/foundation/layout/Offset.kt

use super::{Modifier, Point, inspector_metadata};
use crate::modifier_nodes::{FractionalOffsetElement, OffsetElement};

impl Modifier {
    /// Offset the content by (x, y). The offsets can be positive or negative.
    ///
    /// This modifier is RTL-aware: positive x offsets move content right in LTR
    /// and left in RTL layouts.
    ///
    /// Matches Kotlin: `Modifier.offset(x: Dp, y: Dp)`
    ///
    /// Example: `Modifier::empty().offset(10.0, 20.0)`
    pub fn offset(self, x: f32, y: f32) -> Self {
        let modifier = Self::with_element(OffsetElement::new(x, y, true)).with_inspector_metadata(
            inspector_metadata("offset", move |info| {
                info.add_offset_components("offsetX", "offsetY", Point { x, y });
            }),
        );
        self.then(modifier)
    }

    /// Offset the content by (x, y) without considering layout direction.
    ///
    /// Positive x always moves content to the right regardless of RTL.
    ///
    /// Matches Kotlin: `Modifier.absoluteOffset(x: Dp, y: Dp)`
    ///
    /// Example: `Modifier::empty().absolute_offset(10.0, 20.0)`
    pub fn absolute_offset(self, x: f32, y: f32) -> Self {
        let modifier = Self::with_element(OffsetElement::new(x, y, false)).with_inspector_metadata(
            inspector_metadata("absoluteOffset", move |info| {
                info.add_offset_components("absoluteOffsetX", "absoluteOffsetY", Point { x, y });
            }),
        );
        self.then(modifier)
    }

    /// Offset the content by a fraction of its own measured size.
    ///
    /// `x_fraction` / `y_fraction` are multiplied by the measured width /
    /// height of the content when it is placed, so `offset_fraction(0.0,
    /// -0.5)` moves the content up by half its own height. Like
    /// [`Modifier::offset`], this only affects placement, not measurement.
    ///
    /// There is no direct Jetpack Compose equivalent; Compose's
    /// `slideInVertically`/`slideOutVertically` receive the measured size via
    /// a lambda instead. This modifier backs `slide_in_vertically` /
    /// `slide_out_vertically` in `AnimatedVisibility`.
    ///
    /// Example: `Modifier::empty().offset_fraction(0.0, -0.5)`
    pub fn offset_fraction(self, x_fraction: f32, y_fraction: f32) -> Self {
        let modifier = Self::with_element(FractionalOffsetElement::new(x_fraction, y_fraction))
            .with_inspector_metadata(inspector_metadata("offsetFraction", move |info| {
                info.add_property("offsetFractionX", x_fraction.to_string());
                info.add_property("offsetFractionY", y_fraction.to_string());
            }));
        self.then(modifier)
    }
}
