use super::{Brush, Modifier, RoundedCornerShape, inspector_metadata};
use crate::modifier_nodes::BorderElement;

impl Modifier {
    /// Strokes the edge of `shape` in `brush`, `width` wide, over the content
    /// and inside the node's bounds, as Compose's
    /// `Modifier.border(width, brush, shape)` does; a colour paints as a
    /// solid brush. `RoundedCornerShape::uniform(0.0)` borders a rectangle.
    ///
    /// Example: `Modifier::empty().border(1.0, Color::BLACK, RoundedCornerShape::uniform(6.0))`
    pub fn border(self, width: f32, brush: impl Into<Brush>, shape: RoundedCornerShape) -> Self {
        let brush = brush.into();
        let metadata = inspector_metadata("border", |info| {
            info.add_property("width", format!("{width}"));
            info.add_property("brush", format!("{brush:?}"));
        });
        let modifier = Self::with_element(BorderElement::new(width, brush.clone(), shape))
            .with_inspector_metadata(metadata);
        self.then(modifier)
    }
}

#[cfg(test)]
#[path = "tests/border_tests.rs"]
mod tests;
