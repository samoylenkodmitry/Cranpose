use super::Modifier;
use crate::modifier_nodes::RotateElement;

impl Modifier {
    /// Rotates the content by the specified angle in degrees.
    ///
    /// The rotation is performed around the center of the content.
    ///
    /// # Arguments
    /// * `degrees` - The rotation angle in degrees (positive = clockwise)
    ///
    /// # Example
    /// ```ignore
    /// Modifier::empty()
    ///     .then(Modifier::rotate(45.0)) // Rotate 45 degrees clockwise
    /// ```
    pub fn rotate(degrees: f32) -> Self {
        Self::with_element(RotateElement::new(degrees), |_state| {})
    }
}
