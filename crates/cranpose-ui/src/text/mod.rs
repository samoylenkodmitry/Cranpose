pub use cranpose_text::{
    annotated_string, decoration, font, layout_options, line_box, paragraph, style, unit,
};
pub mod draw_scope_text;
pub(crate) mod layout_cache;
mod layout_input;
/// Text measurement, line wrapping and cursor hit testing.
pub mod measure;
pub(crate) mod shared_plain_text;
mod shared_text;
#[cfg(any(test, feature = "test-helpers"))]
mod test_fonts;
mod text_key;

pub use annotated_string::{
    AnnotatedString, LinkAnnotation, LinkKey, RangeStyle, RenderString, SpanBoundaries,
    StringAnnotation,
};
pub use decoration::{Shadow, TextDecoration};
pub use draw_scope_text::{AppContextTextMeasurer, draw_style_line_box, text_style_for_draw_style};
pub use font::{
    FileBackedFontFamily, FontFamily, FontFile, FontStyle, FontSynthesis, FontWeight,
    LoadedTypefacePath,
};
pub use layout_options::{TextLayoutOptions, TextOptions, TextOverflow};
pub use line_box::{FontExtent, LineBox};
pub use measure::{
    PreparedTextLayout, TextMetrics, first_baseline, get_cursor_x_for_offset,
    get_offset_for_position, glyph_line_box, layout_text, measure_text, measure_text_with_options,
    offset_for_position_wrapped, prepare_text_layout, set_text_fonts, text_line_box,
    wrapped_line_ranges,
};
pub use paragraph::{
    Hyphens, LineBreak, ResolvedTextDirection, TextAlign, TextDirection, TextIndent,
    resolve_text_direction,
};
pub use shared_plain_text::{shared_plain_annotated_string, shared_plain_render_string};
pub use shared_text::SharedText;
pub use style::{
    BaselineShift, LineHeightAlignment, LineHeightMode, LineHeightStyle, LineHeightTrim,
    LocaleList, ParagraphStyle, PlatformParagraphStyle, PlatformSpanStyle, PlatformTextStyle,
    SpanStyle, TextDrawStyle, TextGeometricTransform, TextMotion, TextShaping, TextStyle,
};
#[cfg(any(test, feature = "test-helpers"))]
pub use test_fonts::test_fonts;
pub use unit::TextUnit;

pub use crate::text_layout_result::TextLayoutResult;
