//! The bold and the italic a layout's glyphs ask of faces that lack them.

use crate::typeface::Synthesis;

/// The bold and the italic that the glyphs of a layout ask of faces that
/// draw neither, which a renderer synthesizes where the text's
/// `FontSynthesis` allows it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SynthesizedStyle {
    /// The size in pixels of the largest glyphs whose face lacks their bold.
    pub bold_size: Option<f32>,
    /// Whether the face of a glyph lacks its italic.
    pub italic: bool,
}

impl SynthesizedStyle {
    // Adds glyphs `size` pixels large that ask `synthesis` of their face.
    pub(crate) fn add(&mut self, synthesis: Synthesis, size: f32) {
        if synthesis.bold {
            self.bold_size = Some(self.bold_size.map_or(size, |bold| bold.max(size)));
        }
        self.italic |= synthesis.italic;
    }
}
