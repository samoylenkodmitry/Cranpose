//! The layout of a text, as Compose's `TextLayoutResult` gives it.

use cranpose_text::TextLayout;

/// The layout of a text: Compose's `TextLayoutResult`. Its queries are the
/// layout's, in logical pixels from the text's top left corner, with byte
/// offsets into the text.
#[derive(Clone, Debug)]
pub struct TextLayoutResult {
    layout: TextLayout,
}

impl TextLayoutResult {
    /// The result of `layout`.
    pub fn new(layout: TextLayout) -> Self {
        Self { layout }
    }

    /// The lines and glyphs, and every query on them.
    pub fn layout(&self) -> &TextLayout {
        &self.layout
    }
}
