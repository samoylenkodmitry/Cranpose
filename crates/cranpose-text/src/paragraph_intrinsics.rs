/// The widths of a paragraph that do not depend on the width it is laid
/// out at: Compose's `ParagraphIntrinsics`. Each includes the indent of the
/// line it is measured on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParagraphIntrinsics {
    /// The widest word, the narrowest width no line breaks inside a word at.
    pub min_intrinsic_width: f32,
    /// The widest paragraph laid out on one line, the narrowest width no
    /// paragraph wraps at.
    pub max_intrinsic_width: f32,
}
