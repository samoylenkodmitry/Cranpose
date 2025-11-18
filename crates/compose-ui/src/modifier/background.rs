use super::{inspector_metadata, Color, Modifier, RoundedCornerShape};
use crate::modifier_nodes::{BackgroundElement, CornerShapeElement};

impl Modifier {
    /// Set the background color.
    ///
    /// Example: `Modifier::empty().background(Color::rgb(1.0, 0.0, 0.0))`
    pub fn background(self, color: Color) -> Self {
        let modifier = Self::with_element(BackgroundElement::new(color))
            .with_inspector_metadata(background_metadata(color));
        self.then(modifier)
    }

    /// Add rounded corners with uniform radius.
    ///
    /// Example: `Modifier::empty().rounded_corners(8.0)`
    pub fn rounded_corners(self, radius: f32) -> Self {
        let shape = RoundedCornerShape::uniform(radius);
        let modifier = Self::with_element(CornerShapeElement::new(shape));
        self.then(modifier)
    }

    /// Add rounded corners with a custom shape.
    ///
    /// Example: `Modifier::empty().rounded_corner_shape(shape)`
    pub fn rounded_corner_shape(self, shape: RoundedCornerShape) -> Self {
        let modifier = Self::with_element(CornerShapeElement::new(shape));
        self.then(modifier)
    }
}

fn background_metadata(color: Color) -> super::InspectorMetadata {
    inspector_metadata("background", |info| {
        info.add_property("backgroundColor", format!("{color:?}"));
    })
}
