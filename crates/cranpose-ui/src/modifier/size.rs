use cranpose_ui_layout::IntrinsicSize;

use super::{DimensionConstraint, Modifier, Size, inspector_metadata};
use crate::modifier_nodes::{IntrinsicSizeElement, SizeElement};

impl Modifier {
    /// Declare the preferred size of the content to be exactly `size`.
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

    /// Declare the preferred size of the content to be exactly `width`dp by `height`dp.
    ///
    /// Convenience method for `size(Size { width, height })`.
    ///
    /// Example: `Modifier::empty().size_points(100.0, 200.0)`
    pub fn size_points(self, width: f32, height: f32) -> Self {
        self.size(Size { width, height })
    }

    /// Declare the preferred width of the content to be exactly `width`dp.
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

    /// Declare the preferred height of the content to be exactly `height`dp.
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

    /// Declare the size of the content to be exactly `size`, ignoring incoming constraints.
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

    /// Keep the width of the content between `min` and `max`, as far as the
    /// incoming constraints allow. `f32::INFINITY` leaves the upper side open.
    ///
    /// Matches Kotlin: `Modifier.widthIn(min: Dp, max: Dp)`
    ///
    /// Example: `Modifier::empty().width_in(48.0, f32::INFINITY)`
    pub fn width_in(self, min: f32, max: f32) -> Self {
        let modifier = Self::with_element(SizeElement::with_constraints(
            bound(min),
            bound(max),
            None,
            None,
            true,
        ))
        .with_inspector_metadata(inspector_metadata("widthIn", move |info| {
            info.add_dimension("minWidth", DimensionConstraint::Points(min));
        }));
        self.then(modifier)
    }

    /// Keep the height of the content between `min` and `max`, as far as the
    /// incoming constraints allow. `f32::INFINITY` leaves the upper side open.
    /// A text field or a chip that a finger has to find gets its 24 points
    /// this way without growing to the 48 of
    /// [`minimum_interactive_component_size`](Self::minimum_interactive_component_size).
    ///
    /// Matches Kotlin: `Modifier.heightIn(min: Dp, max: Dp)`
    ///
    /// Example: `Modifier::empty().height_in(24.0, f32::INFINITY)`
    pub fn height_in(self, min: f32, max: f32) -> Self {
        let modifier = Self::with_element(SizeElement::with_constraints(
            None,
            None,
            bound(min),
            bound(max),
            true,
        ))
        .with_inspector_metadata(inspector_metadata("heightIn", move |info| {
            info.add_dimension("minHeight", DimensionConstraint::Points(min));
        }));
        self.then(modifier)
    }

    /// Keep the size of the content between `min` and `max` on each side, as
    /// far as the incoming constraints allow. `f32::INFINITY` on a side of
    /// `max` leaves it open.
    ///
    /// Matches Kotlin: `Modifier.sizeIn(minWidth, minHeight, maxWidth, maxHeight)`
    ///
    /// Example: `Modifier::empty().size_in(Size::new(24.0, 24.0), Size::new(f32::INFINITY, 40.0))`
    pub fn size_in(self, min: Size, max: Size) -> Self {
        let modifier = Self::with_element(SizeElement::with_constraints(
            bound(min.width),
            bound(max.width),
            bound(min.height),
            bound(max.height),
            true,
        ))
        .with_inspector_metadata(inspector_metadata("sizeIn", move |info| {
            info.add_dimension("minWidth", DimensionConstraint::Points(min.width));
            info.add_dimension("minHeight", DimensionConstraint::Points(min.height));
        }));
        self.then(modifier)
    }
}

/// A finite side of a size bound; an infinite or zero bound is no bound.
fn bound(value: f32) -> Option<f32> {
    (value.is_finite() && value > 0.0).then_some(value)
}
