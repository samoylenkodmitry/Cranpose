pub mod annotated_string;
pub mod decoration;
pub mod draw_scope_text;
pub mod font;
pub mod layout_options;
pub mod line_box;
pub mod measure;
pub mod paragraph;
pub mod style;
pub mod unit;

pub use annotated_string::{
    AnnotatedString, LinkAnnotation, LinkKey, RangeStyle, RenderString, StringAnnotation,
    shared_plain_annotated_string,
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
    set_text_measurer, wrapped_line_ranges,
};
pub use paragraph::{
    Hyphens, LineBreak, ResolvedTextDirection, TextAlign, TextDirection, TextIndent,
    resolve_text_direction,
};
pub use style::{
    BaselineShift, LineHeightAlignment, LineHeightMode, LineHeightStyle, LineHeightTrim,
    LocaleList, ParagraphStyle, PlatformParagraphStyle, PlatformSpanStyle, PlatformTextStyle,
    SpanStyle, TextDrawStyle, TextGeometricTransform, TextMotion, TextShaping, TextStyle,
};
pub use unit::TextUnit;

pub use crate::text_layout_result::TextLayoutResult;
