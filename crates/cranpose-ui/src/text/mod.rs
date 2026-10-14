/// Styled text: spans, links and string annotations.
pub mod annotated_string;
/// Text decorations (underline, line through) and text shadows.
pub mod decoration;
pub mod draw_scope_text;
/// Font weight, style, family and synthesis settings.
pub mod font;
/// Options that limit lines, control wrapping and choose the overflow behavior.
pub mod layout_options;
pub mod line_box;
/// Text measurement, line wrapping and cursor hit testing.
pub mod measure;
/// Paragraph settings: alignment, direction, line breaking, hyphenation and indent.
pub mod paragraph;
mod shared_text;
/// Text styles: `SpanStyle`, `ParagraphStyle` and `TextStyle`.
pub mod style;
/// Text size units: scaled pixels and multiples of the font size.
pub mod unit;
mod word_breaks;

pub use annotated_string::{
    AnnotatedString, LinkAnnotation, LinkKey, RangeStyle, RenderString, SpanBoundaries,
    StringAnnotation, shared_plain_annotated_string, shared_plain_render_string,
};
pub use decoration::{Shadow, TextDecoration};
pub use draw_scope_text::{AppContextTextMeasurer, draw_style_line_box, text_style_for_draw_style};
pub use font::{
    FileBackedFontFamily, FontFamily, FontFile, FontStyle, FontSynthesis, FontWeight,
    LoadedTypefacePath,
};
pub use layout_options::{TextLayoutOptions, TextOptions, TextOverflow};
pub use line_box::{FontExtent, LineBox, line_box};
pub use measure::{
    PreparedTextLayout, TextLinePrefixWidths, TextMeasurer, TextMetrics, first_baseline,
    get_cursor_x_for_offset, get_offset_for_position, glyph_line_box, layout_text, measure_text,
    measure_text_for_node, measure_text_with_options, measure_text_with_options_for_node,
    offset_for_position_wrapped, prepare_text_layout, prepare_text_layout_for_node,
    set_text_measurer, text_line_box, wrapped_line_ranges,
};
pub use paragraph::{
    Hyphens, LineBreak, ResolvedTextDirection, TextAlign, TextDirection, TextIndent,
    resolve_text_direction,
};
pub use shared_text::SharedText;
pub use style::{
    BaselineShift, LineHeightAlignment, LineHeightMode, LineHeightStyle, LineHeightTrim,
    LocaleList, ParagraphStyle, PlatformParagraphStyle, PlatformSpanStyle, PlatformTextStyle,
    SpanStyle, TextDrawStyle, TextGeometricTransform, TextMotion, TextShaping, TextStyle,
    text_align_fraction,
};
pub use unit::TextUnit;

pub use crate::text_layout_result::TextLayoutResult;
