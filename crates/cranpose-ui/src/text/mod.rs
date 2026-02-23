pub mod annotated_string;
pub mod decoration;
pub mod font;
pub mod layout_options;
pub mod measure;
pub mod paragraph;
pub mod style;
pub mod unit;

pub use annotated_string::{AnnotatedString, RangeStyle};
pub use decoration::{Shadow, TextDecoration};
pub use font::{
    FileBackedFontFamily, FontFamily, FontFile, FontStyle, FontSynthesis, FontWeight,
    LoadedTypefacePath,
};
pub use layout_options::{TextLayoutOptions, TextOverflow};
pub use measure::{
    get_cursor_x_for_offset, get_offset_for_position, layout_text, measure_text,
    measure_text_with_options, prepare_text_layout, set_text_measurer, PreparedTextLayout,
    TextMeasurer, TextMetrics,
};
pub use paragraph::{
    resolve_text_direction, Hyphens, LineBreak, ResolvedTextDirection, TextAlign, TextDirection,
    TextIndent,
};
pub use style::{
    BaselineShift, LineHeightAlignment, LineHeightMode, LineHeightStyle, LineHeightTrim,
    LocaleList, ParagraphStyle, PlatformParagraphStyle, PlatformSpanStyle, PlatformTextStyle,
    SpanStyle, TextDrawStyle, TextGeometricTransform, TextMotion, TextStyle,
};
pub use unit::TextUnit;
