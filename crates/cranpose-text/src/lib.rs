#![doc = include_str!("../README.md")]

/// Styled text: spans, links and string annotations.
pub mod annotated_string;
mod coverage;
/// Text decorations (underline, line through) and text shadows.
pub mod decoration;
/// Font weight, style, family and synthesis settings.
pub mod font;
mod font_fallback;
/// Which typeface draws each part of a text: Compose's `FontFamily.Resolver`
/// with Android's fallback rules.
pub mod font_family_resolver;
/// Options that limit lines, control wrapping and choose the overflow behavior.
pub mod layout_options;
pub mod line_box;
/// Paragraph settings: alignment, direction, line breaking, hyphenation and indent.
pub mod paragraph;
/// Text styles: `SpanStyle`, `ParagraphStyle` and `TextStyle`.
pub mod style;
/// A font loaded for text: Compose's `Typeface`.
pub mod typeface;
/// Text size units: scaled pixels and multiples of the font size.
pub mod unit;

pub use annotated_string::{
    AnnotatedString, LinkAnnotation, LinkKey, RangeStyle, RenderString, SpanBoundaries,
    StringAnnotation,
};
pub use decoration::{Shadow, TextDecoration};
pub use font::{
    FileBackedFontFamily, FontFamily, FontFile, FontStyle, FontSynthesis, FontWeight,
    LoadedTypefacePath,
};
pub use font_family_resolver::{FontFamilyResolver, FontRequest};
pub use layout_options::{TextLayoutOptions, TextOptions, TextOverflow};
pub use line_box::{FontExtent, LineBox, line_box};
pub use paragraph::{
    Hyphens, LineBreak, ResolvedTextDirection, TextAlign, TextDirection, TextIndent,
    resolve_text_direction,
};
pub use style::{
    BaselineShift, LineHeightAlignment, LineHeightMode, LineHeightStyle, LineHeightTrim,
    LocaleList, ParagraphStyle, PlatformParagraphStyle, PlatformSpanStyle, PlatformTextStyle,
    SpanStyle, TextDrawStyle, TextGeometricTransform, TextMotion, TextShaping, TextStyle,
    text_align_fraction,
};
pub use typeface::{FontBytes, FontFamilyKey, LineMetrics, Typeface, TypefaceError};
pub use unit::TextUnit;
