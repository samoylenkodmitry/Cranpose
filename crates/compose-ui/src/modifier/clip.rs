use super::{Modifier, RoundedCornerShape};
use crate::modifier_nodes::ClipElement;

impl Modifier {
    /// Clips the content to the specified shape.
    ///
    /// # Arguments
    /// * `shape` - The shape to clip to (e.g., RoundedCornerShape)
    ///
    /// # Example
    /// ```ignore
    /// Modifier::empty()
    ///     .then(Modifier::clip(RoundedCornerShape::uniform(8.0)))
    /// ```
    pub fn clip(shape: RoundedCornerShape) -> Self {
        Self::with_element(ClipElement::new(shape), |_state| {})
    }
}
