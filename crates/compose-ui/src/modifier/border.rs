use super::{Color, Modifier, RoundedCornerShape};
use crate::modifier_nodes::BorderElement;

impl Modifier {
    /// Draws a border around the content with the specified width and color.
    ///
    /// # Arguments
    /// * `width` - The width of the border in density-independent pixels
    /// * `color` - The color of the border
    ///
    /// # Example
    /// ```ignore
    /// Modifier::empty()
    ///     .padding(16.0)
    ///     .then(Modifier::border(2.0, Color::BLACK))
    /// ```
    pub fn border(width: f32, color: Color) -> Self {
        Self::with_element(BorderElement::new(width, color, None), |_state| {})
    }

    /// Draws a border around the content with the specified width, color, and shape.
    ///
    /// # Arguments
    /// * `width` - The width of the border in density-independent pixels
    /// * `color` - The color of the border
    /// * `shape` - The shape of the border (e.g., rounded corners)
    ///
    /// # Example
    /// ```ignore
    /// Modifier::empty()
    ///     .padding(16.0)
    ///     .then(Modifier::border_shape(
    ///         2.0,
    ///         Color::BLACK,
    ///         RoundedCornerShape::uniform(8.0)
    ///     ))
    /// ```
    pub fn border_shape(width: f32, color: Color, shape: RoundedCornerShape) -> Self {
        Self::with_element(BorderElement::new(width, color, Some(shape)), |_state| {})
    }
}
