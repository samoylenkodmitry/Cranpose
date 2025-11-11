use super::Modifier;
use crate::modifier_nodes::AlphaElement;

impl Modifier {
    /// Sets the alpha (opacity) of the content.
    ///
    /// # Arguments
    /// * `alpha` - The alpha value from 0.0 (fully transparent) to 1.0 (fully opaque)
    ///
    /// # Example
    /// ```ignore
    /// Modifier::empty()
    ///     .then(Modifier::alpha(0.5)) // 50% transparency
    /// ```
    pub fn alpha(alpha: f32) -> Self {
        Self::with_element(AlphaElement::new(alpha), |_state| {})
    }
}
