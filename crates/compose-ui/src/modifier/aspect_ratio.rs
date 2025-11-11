use super::Modifier;
use crate::modifier_nodes::AspectRatioElement;

impl Modifier {
    /// Sets the aspect ratio of the content.
    ///
    /// The aspect ratio is defined as width / height.
    /// For example, aspectRatio(16.0 / 9.0) creates a 16:9 aspect ratio.
    ///
    /// # Arguments
    /// * `ratio` - The aspect ratio (width / height)
    /// * `match_height_constraints_first` - If true, height constraints are matched first.
    ///   Otherwise, width constraints are matched first.
    pub fn aspectRatio(ratio: f32, match_height_constraints_first: bool) -> Self {
        Self::with_element(
            AspectRatioElement::new(ratio, match_height_constraints_first),
            |_state| {},
        )
    }
}
