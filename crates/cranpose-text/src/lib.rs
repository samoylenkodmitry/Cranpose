#![doc = include_str!("../README.md")]

/// Styled text: spans, links and string annotations.
pub mod annotated_string;
/// The embedding levels of the Unicode bidirectional algorithm.
pub mod bidi_level;
mod cluster_widths;
mod coverage;
/// Text decorations (underline, line through) and text shadows.
pub mod decoration;
/// Font weight, style, family and synthesis settings.
pub mod font;
mod font_fallback;
/// Which typeface draws each part of a text: Compose's `FontFamily.Resolver`
/// with Android's fallback rules.
pub mod font_family_resolver;
/// OpenType feature settings, Compose's `fontFeatureSettings`.
pub mod font_feature;
/// Glyph indices.
pub mod glyph_id;
/// Glyph outlines drawn as coverage.
pub mod glyph_outline;
#[cfg(feature = "hyphenation")]
pub mod hyphenator;
/// Splitting text into the items a shaper takes.
pub mod itemizer;
/// Options that limit lines, control wrapping and choose the overflow behavior.
pub mod layout_options;
pub mod line_box;
mod line_breaker;
/// Paragraph settings: alignment, direction, line breaking, hyphenation and indent.
pub mod paragraph;
/// Writing systems and the script runs of text.
pub mod script;
mod shaped_item;
/// Shaping text items into glyphs with HarfRust.
pub mod shaper;
/// The stretches of a paragraph that shape alike.
pub mod shaping_span;
/// Text styles: `SpanStyle`, `ParagraphStyle` and `TextStyle`.
pub mod style;
/// A paragraph laid out in lines, and Compose's `TextLayoutResult` queries.
pub mod text_layout;
/// Laying paragraphs out in lines.
pub mod text_layouter;
/// A font loaded for text: Compose's `Typeface`.
pub mod typeface;
/// Text size units: scaled pixels and multiples of the font size.
pub mod unit;
/// The words of a text.
pub mod word_boundary;
mod word_cache;
mod word_split;

pub use annotated_string::{
    AnnotatedString, LinkAnnotation, LinkKey, RangeStyle, RenderString, SpanBoundaries,
    StringAnnotation,
};
pub use bidi_level::BidiLevel;
pub use decoration::{Shadow, TextDecoration};
pub use font::{
    FileBackedFontFamily, FontFamily, FontFile, FontStyle, FontSynthesis, FontWeight,
    LoadedTypefacePath,
};
pub use font_family_resolver::{FontFamilyResolver, FontRequest};
pub use font_feature::{FontFeature, font_feature_settings};
pub use glyph_id::GlyphId;
pub use glyph_outline::{GlyphOutline, OutlineSegment, OutlineTransform};
#[cfg(feature = "hyphenation")]
pub use hyphenator::{HyphenationError, Hyphenator};
pub use itemizer::{Itemizer, TextItem};
pub use layout_options::{TextLayoutOptions, TextOptions, TextOverflow};
pub use line_box::{FontExtent, LineBox, line_box, paragraph_line_box};
pub use paragraph::{
    Hyphens, LineBreak, ResolvedTextDirection, TextAlign, TextDirection, TextIndent,
    resolve_text_direction,
};
pub use script::Script;
pub use shaper::{ShapedGlyph, Shaper};
pub use shaping_span::ShapingSpan;
pub use style::{
    BaselineShift, LineHeightAlignment, LineHeightMode, LineHeightStyle, LineHeightTrim,
    LocaleList, ParagraphStyle, PlatformParagraphStyle, PlatformSpanStyle, PlatformTextStyle,
    SpanStyle, TextDrawStyle, TextGeometricTransform, TextMotion, TextShaping, TextStyle,
    text_align_fraction,
};
pub use text_layout::{CaretDirection, GlyphRun, PositionedGlyph, TextLayout};
pub use text_layouter::{LayoutInput, ParagraphIntrinsics, ParagraphLengths, TextLayouter};
pub use typeface::{
    DecorationMetrics, FontBytes, FontFamilyKey, LineMetrics, Synthesis, Typeface, TypefaceError,
};
pub use unit::TextUnit;
pub use word_boundary::word_boundary;
