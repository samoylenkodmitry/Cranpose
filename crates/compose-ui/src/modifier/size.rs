//! Size modifier implementations following Jetpack Compose's layout/Size.kt
//!
//! Reference: /media/huge/composerepo/compose/foundation/foundation-layout/src/commonMain/kotlin/androidx/compose/foundation/layout/Size.kt

use super::{inspector_metadata, DimensionConstraint, Modifier, Size};
use crate::modifier_nodes::{IntrinsicSizeElement, SizeElement};
use compose_ui_layout::IntrinsicSize;

impl Modifier {
    /// Declare the preferred size of the content to be exactly [size].
    ///
    /// The incoming measurement constraints may override this value, forcing the content
    /// to be either smaller or larger.
    ///
    /// Matches Kotlin: `Modifier.size(size: Dp)`
    ///
    /// Example: `Modifier::empty().size(Size { width: 100.0, height: 200.0 })`
    pub fn size(self, size: Size) -> Self {
        let width = size.width;
        let height = size.height;
        let modifier = Self::with_element(SizeElement::new(Some(width), Some(height)))
            .with_inspector_metadata(inspector_metadata("size", move |info| {
                info.add_dimension("width", DimensionConstraint::Points(width));
                info.add_dimension("height", DimensionConstraint::Points(height));
            }));
        self.then(modifier)
    }

    /// Declare the preferred size of the content to be exactly [width]dp by [height]dp.
    ///
    /// Convenience method for `size(Size { width, height })`.
    ///
    /// Example: `Modifier::empty().size_points(100.0, 200.0)`
    pub fn size_points(self, width: f32, height: f32) -> Self {
        self.size(Size { width, height })
    }

    /// Declare the preferred width of the content to be exactly [width]dp.
    ///
    /// The incoming measurement constraints may override this value, forcing the content
    /// to be either smaller or larger.
    ///
    /// Matches Kotlin: `Modifier.width(width: Dp)`
    ///
    /// Example: `Modifier::empty().width(100.0).height(200.0)`
    pub fn width(self, width: f32) -> Self {
        let modifier = Self::with_element(SizeElement::new(Some(width), None))
            .with_inspector_metadata(inspector_metadata("width", move |info| {
                info.add_dimension("width", DimensionConstraint::Points(width));
            }));
        self.then(modifier)
    }

    /// Declare the preferred height of the content to be exactly [height]dp.
    ///
    /// The incoming measurement constraints may override this value, forcing the content
    /// to be either smaller or larger.
    ///
    /// Matches Kotlin: `Modifier.height(height: Dp)`
    ///
    /// Example: `Modifier::empty().width(100.0).height(200.0)`
    pub fn height(self, height: f32) -> Self {
        let modifier = Self::with_element(SizeElement::new(None, Some(height)))
            .with_inspector_metadata(inspector_metadata("height", move |info| {
                info.add_dimension("height", DimensionConstraint::Points(height));
            }));
        self.then(modifier)
    }

    /// Declare the width of the content based on its intrinsic size.
    ///
    /// Matches Kotlin: `Modifier.width(IntrinsicSize)`
    pub fn width_intrinsic(self, intrinsic: IntrinsicSize) -> Self {
        let modifier = Self::with_element(IntrinsicSizeElement::width(intrinsic))
            .with_inspector_metadata(inspector_metadata("widthIntrinsic", move |info| {
                info.add_dimension("width", DimensionConstraint::Intrinsic(intrinsic));
            }));
        self.then(modifier)
    }

    /// Declare the height of the content based on its intrinsic size.
    ///
    /// Matches Kotlin: `Modifier.height(IntrinsicSize)`
    pub fn height_intrinsic(self, intrinsic: IntrinsicSize) -> Self {
        let modifier = Self::with_element(IntrinsicSizeElement::height(intrinsic))
            .with_inspector_metadata(inspector_metadata("heightIntrinsic", move |info| {
                info.add_dimension("height", DimensionConstraint::Intrinsic(intrinsic));
            }));
        self.then(modifier)
    }

    /// Declare the size of the content to be exactly [size], ignoring incoming constraints.
    ///
    /// The incoming measurement constraints will not override this value. If the content
    /// chooses a size that does not satisfy the incoming constraints, the parent layout
    /// will be reported a size coerced in the constraints.
    ///
    /// Matches Kotlin: `Modifier.requiredSize(size: Dp)`
    pub fn required_size(self, size: Size) -> Self {
        let modifier = Self::with_element(SizeElement::with_constraints(
            Some(size.width),
            Some(size.width),
            Some(size.height),
            Some(size.height),
            false,
        ));
        self.then(modifier)
    }
}
