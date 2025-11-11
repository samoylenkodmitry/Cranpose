use super::{inspector_metadata, Color, Modifier, RoundedCornerShape};
use crate::modifier_nodes::{BackgroundElement, CornerShapeElement};

impl Modifier {
    pub fn background(color: Color) -> Self {
        Self::with_element(BackgroundElement::new(color))
            .with_inspector_metadata(background_metadata(color))
    }

    pub fn rounded_corners(radius: f32) -> Self {
        let shape = RoundedCornerShape::uniform(radius);
        Self::with_element(CornerShapeElement::new(shape))
    }

    pub fn rounded_corner_shape(shape: RoundedCornerShape) -> Self {
        Self::with_element(CornerShapeElement::new(shape))
    }
}

fn background_metadata(color: Color) -> super::InspectorMetadata {
    inspector_metadata("background", |info| {
        info.add_property("backgroundColor", format!("{color:?}"));
    })
}
