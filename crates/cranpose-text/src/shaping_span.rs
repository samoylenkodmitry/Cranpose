use std::ops::Range;

use crate::{font_family_resolver::FontRequest, font_feature::FontFeature};

/// A stretch of a paragraph with one font, size and set of shaping settings:
/// a style span of the text. Spans that differ only in what does not change
/// shaping, such as color, are one span, since a ligature such as Arabic
/// lam-alef forms only inside one span.
#[derive(Clone, Debug)]
pub struct ShapingSpan<'a> {
    /// The byte range in the text.
    pub range: Range<usize>,
    /// The family, weight and style. The weight and style also set the axes
    /// of a variable font read without a declaration.
    pub font: FontRequest<'a>,
    /// The font size in pixels.
    pub size: f32,
    /// The OpenType features to turn on or off.
    pub features: &'a [FontFeature],
    /// A BCP 47 language tag such as `ja` or `sr-Latn`, which picks the
    /// font's language-specific forms.
    pub language: Option<&'a str>,
}
