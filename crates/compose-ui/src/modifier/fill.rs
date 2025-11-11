//! Fill modifier implementation following Jetpack Compose's layout/Size.kt (fillMax* modifiers)
//!
//! Reference: /media/huge/composerepo/compose/foundation/foundation-layout/src/commonMain/kotlin/androidx/compose/foundation/layout/Size.kt

use super::{inspector_metadata, DimensionConstraint, InspectorMetadata, Modifier};
use crate::modifier_nodes::FillElement;

impl Modifier {
    /// Have the content fill the maximum available width.
    ///
    /// The [fraction] parameter allows filling only a portion of the available width (0.0 to 1.0).
    ///
    /// Matches Kotlin: `Modifier.fillMaxWidth(fraction: Float)`
    pub fn fill_max_width() -> Self {
        Self::fill_max_width_fraction(1.0)
    }

    pub fn fill_max_width_fraction(fraction: f32) -> Self {
        let clamped = fraction.clamp(0.0, 1.0);
        Self::with_element(FillElement::width(clamped)).with_inspector_metadata(inspector_metadata(
            "fillMaxWidth",
            move |info| {
                info.add_dimension("width", DimensionConstraint::Fraction(clamped));
            },
        ))
    }

    /// Have the content fill the maximum available height.
    ///
    /// The [fraction] parameter allows filling only a portion of the available height (0.0 to 1.0).
    ///
    /// Matches Kotlin: `Modifier.fillMaxHeight(fraction: Float)`
    pub fn fill_max_height() -> Self {
        Self::fill_max_height_fraction(1.0)
    }

    pub fn fill_max_height_fraction(fraction: f32) -> Self {
        let clamped = fraction.clamp(0.0, 1.0);
        Self::with_element(FillElement::height(clamped)).with_inspector_metadata(
            inspector_metadata("fillMaxHeight", move |info| {
                info.add_dimension("height", DimensionConstraint::Fraction(clamped));
            }),
        )
    }

    /// Have the content fill the maximum available size (both width and height).
    ///
    /// The [fraction] parameter allows filling only a portion of the available size (0.0 to 1.0).
    ///
    /// Matches Kotlin: `Modifier.fillMaxSize(fraction: Float)`
    pub fn fill_max_size() -> Self {
        Self::fill_max_size_fraction(1.0)
    }

    pub fn fill_max_size_fraction(fraction: f32) -> Self {
        let clamped = fraction.clamp(0.0, 1.0);
        Self::with_element(FillElement::size(clamped)).with_inspector_metadata(inspector_metadata(
            "fillMaxSize",
            move |info| {
                info.add_dimension("width", DimensionConstraint::Fraction(clamped));
                info.add_dimension("height", DimensionConstraint::Fraction(clamped));
            },
        ))
    }
}
