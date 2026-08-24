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

    /// Add padding in reading order: `start` is the left edge in a
    /// left-to-right layout and the right edge in a right-to-left one.
    ///
    /// The direction comes from `crate::layout_direction`, so a screen that
    /// reverses direction reverses this padding with it.
    ///
    /// Example: `Modifier::empty().padding_relative(16.0, 8.0, 4.0, 8.0)`
    pub fn padding_relative(self, start: f32, top: f32, end: f32, bottom: f32) -> Self {
        self.padding_relative_in(
            crate::layout_direction::layout_direction(),
            start,
            top,
            end,
            bottom,
        )
    }

    /// Add reading-order padding resolved against an explicit direction, for
    /// callers that already know it — a layout that measured one, a test.
    pub fn padding_relative_in(
        self,
        direction: crate::layout_direction::LayoutDirection,
        start: f32,
        top: f32,
        end: f32,
        bottom: f32,
    ) -> Self {
        let (left, right) = direction.resolve(start, end);
        self.padding_each(left, top, right, bottom)
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
