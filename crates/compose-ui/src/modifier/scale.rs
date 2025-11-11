use super::Modifier;
use crate::modifier_nodes::ScaleElement;

impl Modifier {
    /// Scales the content uniformly by the specified factor.
    ///
    /// The scaling is performed around the center of the content.
    ///
    /// # Arguments
    /// * `scale` - The scale factor (1.0 = no scaling, 2.0 = double size, 0.5 = half size)
    ///
    /// # Example
    /// ```ignore
    /// Modifier::empty()
    ///     .then(Modifier::scale(1.5)) // Scale to 150% of original size
    /// ```
    pub fn scale(scale: f32) -> Self {
        Self::with_element(ScaleElement::new(scale, scale), |_state| {})
    }

    /// Scales the content by different factors in X and Y dimensions.
    ///
    /// The scaling is performed around the center of the content.
    ///
    /// # Arguments
    /// * `scale_x` - The horizontal scale factor
    /// * `scale_y` - The vertical scale factor
    ///
    /// # Example
    /// ```ignore
    /// Modifier::empty()
    ///     .then(Modifier::scale_xy(2.0, 1.0)) // Stretch horizontally
    /// ```
    pub fn scale_xy(scale_x: f32, scale_y: f32) -> Self {
        Self::with_element(ScaleElement::new(scale_x, scale_y), |_state| {})
    }
}
