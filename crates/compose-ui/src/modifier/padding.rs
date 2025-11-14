use super::{inspector_metadata, EdgeInsets, InspectorMetadata, Modifier};
use crate::modifier_nodes::PaddingElement;

impl Modifier {
    /// Add uniform padding to all sides.
    ///
    /// Example: `Modifier::empty().padding(16.0)`
    pub fn padding(self, p: f32) -> Self {
        let padding = EdgeInsets::uniform(p);
        let modifier = Self::with_element(PaddingElement::new(padding))
            .with_inspector_metadata(padding_metadata(padding));
        self.then(modifier)
    }

    /// Add horizontal padding (left and right).
    ///
    /// Example: `Modifier::empty().padding_horizontal(16.0)`
    pub fn padding_horizontal(self, horizontal: f32) -> Self {
        let padding = EdgeInsets::horizontal(horizontal);
        let modifier = Self::with_element(PaddingElement::new(padding))
            .with_inspector_metadata(padding_metadata(padding));
        self.then(modifier)
    }

    /// Add vertical padding (top and bottom).
    ///
    /// Example: `Modifier::empty().padding_vertical(8.0)`
    pub fn padding_vertical(self, vertical: f32) -> Self {
        let padding = EdgeInsets::vertical(vertical);
        let modifier = Self::with_element(PaddingElement::new(padding))
            .with_inspector_metadata(padding_metadata(padding));
        self.then(modifier)
    }

    /// Add symmetric padding (horizontal and vertical).
    ///
    /// Example: `Modifier::empty().padding_symmetric(16.0, 8.0)`
    pub fn padding_symmetric(self, horizontal: f32, vertical: f32) -> Self {
        let padding = EdgeInsets::symmetric(horizontal, vertical);
        let modifier = Self::with_element(PaddingElement::new(padding))
            .with_inspector_metadata(padding_metadata(padding));
        self.then(modifier)
    }

    /// Add padding to each side individually.
    ///
    /// Example: `Modifier::empty().padding_each(8.0, 4.0, 8.0, 4.0)`
    pub fn padding_each(self, left: f32, top: f32, right: f32, bottom: f32) -> Self {
        let padding = EdgeInsets::from_components(left, top, right, bottom);
        let modifier = Self::with_element(PaddingElement::new(padding))
            .with_inspector_metadata(padding_metadata(padding));
        self.then(modifier)
    }
}

fn padding_metadata(padding: EdgeInsets) -> InspectorMetadata {
    inspector_metadata("padding", |info| {
        info.add_property("paddingLeft", padding.left.to_string());
        info.add_property("paddingTop", padding.top.to_string());
        info.add_property("paddingRight", padding.right.to_string());
        info.add_property("paddingBottom", padding.bottom.to_string());
    })
}
