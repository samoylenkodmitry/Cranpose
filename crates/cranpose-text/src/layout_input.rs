use crate::{layout_options::TextLayoutOptions, shaping_span::ShapingSpan, style::ParagraphStyle};

/// A paragraph to lay out and how.
#[derive(Clone, Debug)]
pub struct LayoutInput<'a> {
    /// The text.
    pub text: &'a str,
    /// The spans of the text, covering it in order with no gap.
    pub spans: &'a [ShapingSpan<'a>],
    /// The paragraph settings: direction, alignment and line height style.
    pub paragraph: &'a ParagraphStyle,
    /// The paragraph's lengths in pixels.
    pub lengths: ParagraphLengths,
    /// The narrowest the paragraph may be, in pixels.
    pub min_width: f32,
    /// The widest the paragraph may be, in pixels; infinite for lines that
    /// end only at line breaks. As Compose sizes a paragraph, it takes its
    /// max intrinsic width rounded up to whole device pixels, within these
    /// two, and the widest only when they are equal; its lines wrap at that
    /// width when they soft wrap, and ellipses cut to it.
    pub max_width: f32,
    /// Soft wrapping, the line limits and what shows where text overflows.
    pub options: TextLayoutOptions,
}

/// A paragraph style's lengths resolved to pixels.
#[derive(Clone, Copy, Debug)]
pub struct ParagraphLengths {
    /// The line height the style asks for, `None` for the fonts' own.
    pub line_height: Option<f32>,
    /// The indent of the first line of each paragraph.
    pub first_line_indent: f32,
    /// The indent of the other lines.
    pub rest_line_indent: f32,
    /// Device pixels per pixel of the layout, finite and positive: line boxes
    /// round to whole device pixels, as Android's do.
    pub grid: f32,
    /// Whether glyph advances round to that grid.
    pub advances: GlyphAdvances,
}

/// How a glyph's advance meets the device pixel grid.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GlyphAdvances {
    /// The font's advance at the text size: glyphs keep subpixel positions,
    /// as Compose places them on desktop, iOS and the web, and on Android
    /// for `TextMotion.Animated`.
    #[default]
    Fractional,
    /// The font's advance rounded to a whole device pixel, with the kerning
    /// shaping adds to it left whole: Android's `TextMotion.Static`, which
    /// turns hinting on and subpixel positioning off.
    WholePixels,
}

impl Default for ParagraphLengths {
    fn default() -> Self {
        Self {
            line_height: None,
            first_line_indent: 0.0,
            rest_line_indent: 0.0,
            grid: 1.0,
            advances: GlyphAdvances::Fractional,
        }
    }
}
