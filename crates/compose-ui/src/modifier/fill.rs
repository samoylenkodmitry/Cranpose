//! Fill modifier implementation following Jetpack Compose's layout/Size.kt (fillMax* modifiers)
//!
//! Reference: /media/huge/composerepo/compose/foundation/foundation-layout/src/commonMain/kotlin/androidx/compose/foundation/layout/Size.kt

use super::{inspector_metadata, DimensionConstraint, Modifier};
use crate::modifier_nodes::FillElement;

impl Modifier {
    /// Have the content fill the maximum available width.
    ///
    /// The [fraction] parameter allows filling only a portion of the available width (0.0 to 1.0).
    ///
    /// Matches Kotlin: `Modifier.fillMaxWidth(fraction: Float)`
    ///
    /// Example: `Modifier::empty().fill_max_width()`
    pub fn fill_max_width(self) -> Self {
        self.fill_max_width_fraction(1.0)
    }

    /// Fill a fraction of the maximum available width.
    ///
    /// Example: `Modifier::empty().fill_max_width_fraction(0.5)`
    pub fn fill_max_width_fraction(self, fraction: f32) -> Self {
        let clamped = fraction.clamp(0.0, 1.0);
        let modifier = Self::with_element(FillElement::width(clamped)).with_inspector_metadata(
            inspector_metadata("fillMaxWidth", move |info| {
                info.add_dimension("width", DimensionConstraint::Fraction(clamped));
            }),
        );
        self.then(modifier)
    }

    /// Have the content fill the maximum available height.
    ///
    /// The [fraction] parameter allows filling only a portion of the available height (0.0 to 1.0).
    ///
    /// Matches Kotlin: `Modifier.fillMaxHeight(fraction: Float)`
    ///
    /// Example: `Modifier::empty().fill_max_height()`
    pub fn fill_max_height(self) -> Self {
        self.fill_max_height_fraction(1.0)
    }

    /// Fill a fraction of the maximum available height.
    ///
    /// Example: `Modifier::empty().fill_max_height_fraction(0.5)`
    pub fn fill_max_height_fraction(self, fraction: f32) -> Self {
        let clamped = fraction.clamp(0.0, 1.0);
        let modifier = Self::with_element(FillElement::height(clamped)).with_inspector_metadata(
            inspector_metadata("fillMaxHeight", move |info| {
                info.add_dimension("height", DimensionConstraint::Fraction(clamped));
            }),
        );
        self.then(modifier)
    }

    /// Have the content fill the maximum available size (both width and height).
    ///
    /// The [fraction] parameter allows filling only a portion of the available size (0.0 to 1.0).
    ///
    /// Matches Kotlin: `Modifier.fillMaxSize(fraction: Float)`
    ///
    /// Example: `Modifier::empty().fill_max_size()`
    pub fn fill_max_size(self) -> Self {
        self.fill_max_size_fraction(1.0)
    }

    /// Fill a fraction of the maximum available size.
    ///
    /// Example: `Modifier::empty().fill_max_size_fraction(0.8)`
    pub fn fill_max_size_fraction(self, fraction: f32) -> Self {
        let clamped = fraction.clamp(0.0, 1.0);
        let modifier = Self::with_element(FillElement::size(clamped)).with_inspector_metadata(
            inspector_metadata("fillMaxSize", move |info| {
                info.add_dimension("width", DimensionConstraint::Fraction(clamped));
                info.add_dimension("height", DimensionConstraint::Fraction(clamped));
            }),
        );
        self.then(modifier)
    }
}
