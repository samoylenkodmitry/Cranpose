use std::hash::{Hash, Hasher};

use cranpose_ui_graphics::{Brush, Color, FxHasher, RenderHash};
/// The line-height policy lives with the drawing-side text types, because a
/// [`DrawScope`](cranpose_ui_graphics::DrawScope) has to be able to name it too:
/// a run drawn through a canvas and a `Text` composable on the same screen must
/// resolve their line boxes by the same rule, and the only style a draw scope
/// can carry is the flat one from `cranpose-ui-graphics`.
pub use cranpose_ui_graphics::{
    LineHeightAlignment, LineHeightMode, LineHeightStyle, LineHeightTrim,
};

use super::{
    decoration::{Shadow, TextDecoration},
    font::{FontFamily, FontStyle, FontSynthesis, FontWeight},
    paragraph::{Hyphens, LineBreak, TextAlign, TextDirection, TextIndent},
    unit::TextUnit,
};

/// Vertical shift of text from its baseline as a fraction of the font size. Positive values
/// move the text up. Compose's `BaselineShift`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BaselineShift(pub f32);

impl BaselineShift {
    /// Moves the text up by half the font size.
    pub const SUPERSCRIPT: Self = Self(0.5);
    /// Moves the text down by half the font size.
    pub const SUBSCRIPT: Self = Self(-0.5);
    /// No shift.
    pub const NONE: Self = Self(0.0);
    /// No value is set and the shift is inherited. Holds `NaN`.
    pub const UNSPECIFIED: Self = Self(f32::NAN);

    /// Returns false for [`BaselineShift::UNSPECIFIED`].
    pub fn is_specified(self) -> bool {
        !self.0.is_nan()
    }
}

/// Horizontal scaling and skew of glyphs. Compose's `TextGeometricTransform`. Text rendering
/// does not apply it yet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextGeometricTransform {
    /// Horizontal scale factor of the glyphs. 1.0 leaves them unscaled.
    pub scale_x: f32,
    /// Horizontal skew of the glyphs. 0.0 leaves them upright.
    pub skew_x: f32,
}

impl Default for TextGeometricTransform {
    fn default() -> Self {
        Self {
            scale_x: 1.0,
            skew_x: 0.0,
        }
    }
}

/// Ordered list of language tags, most preferred first. They select language-specific glyph
/// forms and hyphenation rules. Compose's `LocaleList`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct LocaleList {
    locales: std::sync::Arc<[String]>,
}

impl LocaleList {
    /// Creates a list from `locales`, which are language tags such as `en-US`.
    pub fn new(locales: Vec<String>) -> Self {
        Self {
            locales: locales.into(),
        }
    }

    /// Parses a comma-separated list of language tags such as `en-US, fr`. Blank entries are
    /// skipped.
    pub fn from_language_tags(tags: &str) -> Self {
        let locales: Vec<_> = tags
            .split(',')
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .map(ToString::to_string)
            .collect();
        Self::new(locales)
    }

    /// The language tags, most preferred first.
    pub fn locales(&self) -> &[String] {
        &self.locales
    }

    /// Returns true when the list holds no tags.
    pub fn is_empty(&self) -> bool {
        self.locales.is_empty()
    }
}

/// Whether text is expected to move, scale or rotate. Compose's `TextMotion`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TextMotion {
    /// The text stays in place. Glyphs are placed on whole pixels for sharp edges.
    #[default]
    Static,
    /// The text may move, scale or rotate. Glyphs keep sub-pixel positions so motion stays
    /// smooth.
    Animated,
}

/// Platform-specific settings of a span. It holds no options yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct PlatformSpanStyle;

/// How much work the text shaper does for a paragraph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextShaping {
    /// Text is shaped only when it has non-ASCII characters. ASCII text uses plain glyph advances.
    Basic,
    /// The full OpenType shaper always runs, including on ASCII text.
    Advanced,
}

/// Platform-specific settings of a paragraph. Compose's `PlatformParagraphStyle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct PlatformParagraphStyle {
    /// When `Some(true)`, the font's line gap is added to the line height, as with Android's
    /// `includeFontPadding`. `None` and `Some(false)` add nothing.
    pub include_font_padding: Option<bool>,
    /// Shaping mode. `None` acts like [`TextShaping::Basic`].
    pub shaping: Option<TextShaping>,
}

/// Platform-specific settings of a span and of a paragraph. Compose's `PlatformTextStyle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct PlatformTextStyle {
    /// Settings for spans.
    pub span_style: Option<PlatformSpanStyle>,
    /// Settings for paragraphs.
    pub paragraph_style: Option<PlatformParagraphStyle>,
}

/// Whether glyphs are filled or only outlined.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum TextDrawStyle {
    /// Glyphs are filled with the text color or brush.
    #[default]
    Fill,
    /// Only the outlines of the glyphs are drawn.
    Stroke {
        /// Outline width in logical pixels. Zero, negative and non-finite widths draw a fill.
        width: f32,
    },
}

/// Style of a span of characters: color, font, spacing and decoration. Compose's `SpanStyle`.
/// A field left unset (`None` or `Unspecified`) keeps the inherited value when styles merge.
#[derive(Clone, Debug, PartialEq)]
pub struct SpanStyle {
    /// Text color.
    pub color: Option<Color>,
    /// Brush that paints the glyphs, for example a gradient. Merging a style that sets `color`
    /// clears an inherited `brush`, and the reverse.
    pub brush: Option<Brush>,
    /// Opacity from 0.0 to 1.0, multiplied into the color or brush. `None` leaves it unchanged.
    pub alpha: Option<f32>,
    /// Font size in `Sp` or as a multiple of the default size with `Em`.
    pub font_size: TextUnit,
    /// Weight of the font.
    pub font_weight: Option<FontWeight>,
    /// Upright or italic glyphs.
    pub font_style: Option<FontStyle>,
    /// Which missing bold or italic faces may be faked. `None` allows both.
    pub font_synthesis: Option<FontSynthesis>,
    /// Font family.
    pub font_family: Option<FontFamily>,
    /// OpenType feature settings in CSS `font-feature-settings` syntax, such as `smcp, liga 0`.
    pub font_feature_settings: Option<std::sync::Arc<str>>,
    /// Extra space added after each character, in `Sp` or as a multiple of the font size.
    pub letter_spacing: TextUnit,
    /// Vertical shift of the glyphs from the baseline.
    pub baseline_shift: Option<BaselineShift>,
    /// Horizontal scale and skew of the glyphs. Text rendering does not apply it yet.
    pub text_geometric_transform: Option<TextGeometricTransform>,
    /// Languages of the text, which select language-specific glyph forms and hyphenation rules.
    pub locale_list: Option<LocaleList>,
    /// Color drawn behind the glyphs.
    pub background: Option<Color>,
    /// Underline and line through.
    pub text_decoration: Option<TextDecoration>,
    /// Shadow drawn behind the glyphs.
    pub shadow: Option<Shadow>,
    /// Platform-specific settings.
    pub platform_style: Option<PlatformSpanStyle>,
    /// Fill or stroke. `None` fills the glyphs.
    pub draw_style: Option<TextDrawStyle>,
}

impl Default for SpanStyle {
    fn default() -> Self {
        Self {
            color: None,
            brush: None,
            alpha: None,
            font_size: TextUnit::Unspecified,
            font_weight: None,
            font_style: None,
            font_synthesis: None,
            font_family: None,
            font_feature_settings: None,
            letter_spacing: TextUnit::Unspecified,
            baseline_shift: None,
            text_geometric_transform: None,
            locale_list: None,
            background: None,
            text_decoration: None,
            shadow: None,
            platform_style: None,
            draw_style: None,
        }
    }
}

impl SpanStyle {
    /// Returns a style where each field set in `other` replaces the one in `self`. Fields unset
    /// in `other` keep the value of `self`.
    pub fn merge(&self, other: &Self) -> Self {
        let (merged_color, merged_brush) = merge_foreground_style(self, other);
        Self {
            color: merged_color,
            brush: merged_brush,
            alpha: other.alpha.or(self.alpha),
            font_size: merge_text_unit(self.font_size, other.font_size),
            font_weight: other.font_weight.or(self.font_weight),
            font_style: other.font_style.or(self.font_style),
            font_synthesis: other.font_synthesis.or(self.font_synthesis),
            font_family: other
                .font_family
                .clone()
                .or_else(|| self.font_family.clone()),
            font_feature_settings: other
                .font_feature_settings
                .clone()
                .or_else(|| self.font_feature_settings.clone()),
            letter_spacing: merge_text_unit(self.letter_spacing, other.letter_spacing),
            baseline_shift: other.baseline_shift.or(self.baseline_shift),
            text_geometric_transform: other
                .text_geometric_transform
                .or(self.text_geometric_transform),
            locale_list: other
                .locale_list
                .clone()
                .or_else(|| self.locale_list.clone()),
            background: other.background.or(self.background),
            text_decoration: other.text_decoration.or(self.text_decoration),
            shadow: other.shadow.or(self.shadow),
            platform_style: other.platform_style.or(self.platform_style),
            draw_style: other.draw_style.or(self.draw_style),
        }
    }

    /// Same as [`SpanStyle::merge`]. Compose's `SpanStyle.plus`.
    pub fn plus(&self, other: &Self) -> Self {
        self.merge(other)
    }

    /// Returns the font size in sp. An `Sp` size is used as is, an `Em` size multiplies
    /// `default_size`, and an unspecified, non-positive or non-finite size gives `default_size`.
    /// A `default_size` that is not positive and finite counts as 14.0.
    pub fn resolve_font_size(&self, default_size: f32) -> f32 {
        let fallback = if default_size.is_finite() && default_size > 0.0 {
            default_size
        } else {
            14.0
        };
        match self.font_size {
            TextUnit::Sp(value) if value.is_finite() && value > 0.0 => value,
            TextUnit::Em(value) if value.is_finite() && value > 0.0 => value * fallback,
            _ => fallback,
        }
    }

    /// Returns `color`, else the color of a solid `brush`, else `default_color`, with `alpha`
    /// multiplied into the result's alpha channel.
    pub fn resolve_foreground_color(&self, default_color: Color) -> Color {
        let mut color = self
            .color
            .or_else(|| solid_brush_color(self.brush.as_ref()))
            .unwrap_or(default_color);
        if let Some(alpha) = self.alpha {
            color.3 *= alpha.clamp(0.0, 1.0);
        }
        color
    }

    /// Hash of every field that changes how the text looks, as renderers use it to key caches.
    pub fn render_hash(&self) -> u64 {
        let mut hasher = FxHasher::default();
        hash_span_style(self, &mut hasher);
        hasher.finish()
    }
}

/// Style of a whole paragraph: alignment, direction, line height and line breaking.
/// Compose's `ParagraphStyle`.
#[derive(Clone, Debug, PartialEq)]
pub struct ParagraphStyle {
    /// Horizontal alignment of the lines.
    pub text_align: TextAlign,
    /// Base direction of the paragraph.
    pub text_direction: TextDirection,
    /// Distance between baselines, in `Sp` or as a multiple of the font size. `Unspecified` uses
    /// the natural line height of the font.
    pub line_height: TextUnit,
    /// Indent of the first line and of the other lines. Text layout does not apply it yet.
    pub text_indent: Option<TextIndent>,
    /// Platform-specific settings.
    pub platform_style: Option<PlatformParagraphStyle>,
    /// How the line height is placed in each line and trimmed at the edges of the paragraph.
    pub line_height_style: Option<LineHeightStyle>,
    /// Strategy that breaks the paragraph into lines.
    pub line_break: LineBreak,
    /// Whether words may be split with a hyphen.
    pub hyphens: Hyphens,
    /// Whether the text moves. `None` counts as [`TextMotion::Static`].
    pub text_motion: Option<TextMotion>,
}

impl Default for ParagraphStyle {
    fn default() -> Self {
        Self {
            text_align: TextAlign::Unspecified,
            text_direction: TextDirection::Unspecified,
            line_height: TextUnit::Unspecified,
            text_indent: None,
            platform_style: None,
            line_height_style: None,
            line_break: LineBreak::Unspecified,
            hyphens: Hyphens::Unspecified,
            text_motion: None,
        }
    }
}

impl ParagraphStyle {
    /// Returns a style where each field set in `other` replaces the one in `self`. Fields unset
    /// in `other` keep the value of `self`.
    pub fn merge(&self, other: &Self) -> Self {
        Self {
            text_align: merge_text_align(self.text_align, other.text_align),
            text_direction: merge_text_direction(self.text_direction, other.text_direction),
            line_height: merge_text_unit(self.line_height, other.line_height),
            text_indent: other.text_indent.or(self.text_indent),
            platform_style: other.platform_style.or(self.platform_style),
            line_height_style: other.line_height_style.or(self.line_height_style),
            line_break: merge_line_break(self.line_break, other.line_break),
            hyphens: merge_hyphens(self.hyphens, other.hyphens),
            text_motion: other.text_motion.or(self.text_motion),
        }
    }

    /// Same as [`ParagraphStyle::merge`]. Compose's `ParagraphStyle.plus`.
    pub fn plus(&self, other: &Self) -> Self {
        self.merge(other)
    }

    /// Hash of every field that changes how the paragraph looks, as renderers use it to key
    /// caches.
    pub fn render_hash(&self) -> u64 {
        let mut hasher = FxHasher::default();
        hash_paragraph_style(self, &mut hasher);
        hasher.finish()
    }
}

/// Complete style of a text: a [`SpanStyle`] and a [`ParagraphStyle`]. Compose's `TextStyle`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TextStyle {
    /// Settings that apply to characters.
    pub span_style: SpanStyle,
    /// Settings that apply to the whole paragraph.
    pub paragraph_style: ParagraphStyle,
}

impl TextStyle {
    /// Creates a style from `span_style` and `paragraph_style`.
    pub fn new(span_style: SpanStyle, paragraph_style: ParagraphStyle) -> Self {
        Self {
            span_style,
            paragraph_style,
        }
    }

    /// Creates a style with `span_style` and default paragraph settings.
    pub fn from_span_style(span_style: SpanStyle) -> Self {
        Self::new(span_style, ParagraphStyle::default())
    }

    /// Creates a style with `paragraph_style` and default span settings.
    pub fn from_paragraph_style(paragraph_style: ParagraphStyle) -> Self {
        Self::new(SpanStyle::default(), paragraph_style)
    }

    /// Returns a style that merges the span and paragraph styles of `other` over those of
    /// `self`.
    pub fn merge(&self, other: &Self) -> Self {
        Self {
            span_style: self.span_style.merge(&other.span_style),
            paragraph_style: self.paragraph_style.merge(&other.paragraph_style),
        }
    }

    /// Same as [`TextStyle::merge`]. Compose's `TextStyle.plus`.
    pub fn plus(&self, other: &Self) -> Self {
        self.merge(other)
    }

    /// Returns a copy of the span style.
    pub fn to_span_style(&self) -> SpanStyle {
        self.span_style.clone()
    }

    /// Returns a copy of the paragraph style.
    pub fn to_paragraph_style(&self) -> ParagraphStyle {
        self.paragraph_style.clone()
    }

    /// Returns the platform-specific settings of the span and the paragraph, or `None` when
    /// neither has any.
    pub fn platform_style(&self) -> Option<PlatformTextStyle> {
        create_platform_text_style(
            None,
            self.span_style.platform_style,
            self.paragraph_style.platform_style,
        )
    }

    /// Returns this style with the platform-specific settings of the span and the paragraph
    /// replaced by those in `platform_style`.
    pub fn with_platform_style(mut self, platform_style: Option<PlatformTextStyle>) -> Self {
        self.span_style.platform_style = platform_style.and_then(|style| style.span_style);
        self.paragraph_style.platform_style =
            platform_style.and_then(|style| style.paragraph_style);
        self
    }

    /// Returns the font size in sp. See [`SpanStyle::resolve_font_size`].
    pub fn resolve_font_size(&self, default_size: f32) -> f32 {
        self.span_style.resolve_font_size(default_size)
    }

    /// Returns the line height in the unit of `default_size`. A positive `Sp` line height is used
    /// as is and a positive `Em` one multiplies the font size. Otherwise the result is
    /// `natural_line_height`, or the font size when that is not positive and finite.
    pub fn resolve_line_height(&self, default_size: f32, natural_line_height: f32) -> f32 {
        let fallback = if natural_line_height.is_finite() && natural_line_height > 0.0 {
            natural_line_height
        } else {
            self.resolve_font_size(default_size)
        };
        match self.paragraph_style.line_height {
            TextUnit::Sp(value) if value.is_finite() && value > 0.0 => value,
            TextUnit::Em(value) if value.is_finite() && value > 0.0 => {
                value * self.resolve_font_size(default_size)
            }
            _ => fallback,
        }
    }

    /// Returns the letter spacing in the unit of `default_size`. An `Em` value multiplies the
    /// font size. An unspecified or non-finite spacing gives 0.0.
    pub fn resolve_letter_spacing(&self, default_size: f32) -> f32 {
        let font_size = self.resolve_font_size(default_size);
        match self.span_style.letter_spacing {
            TextUnit::Sp(value) if value.is_finite() => value,
            TextUnit::Em(value) if value.is_finite() => value * font_size,
            _ => 0.0,
        }
    }

    /// Returns the text color. See [`SpanStyle::resolve_foreground_color`].
    pub fn resolve_text_color(&self, default_color: Color) -> Color {
        self.span_style.resolve_foreground_color(default_color)
    }

    /// Hash of the settings that change how the text measures. Colors, decorations and shadows
    /// are left out.
    pub fn measurement_hash(&self) -> u64 {
        let mut hasher = FxHasher::default();
        hash_span_measurement(&self.span_style, &mut hasher);
        hash_paragraph_style(&self.paragraph_style, &mut hasher);
        hasher.finish()
    }

    /// Hash of every setting that changes how the text looks, as renderers use it to key caches.
    pub fn render_hash(&self) -> u64 {
        let mut hasher = FxHasher::default();
        hash_span_style(&self.span_style, &mut hasher);
        hash_paragraph_style(&self.paragraph_style, &mut hasher);
        hasher.finish()
    }
}

fn merge_foreground_style(
    current: &SpanStyle,
    incoming: &SpanStyle,
) -> (Option<Color>, Option<Brush>) {
    if let Some(brush) = incoming.brush.clone() {
        return (None, Some(brush));
    }
    if let Some(color) = incoming.color {
        return (Some(color), None);
    }
    (current.color, current.brush.clone())
}

fn solid_brush_color(brush: Option<&Brush>) -> Option<Color> {
    match brush {
        Some(Brush::Solid(color)) => Some(*color),
        _ => None,
    }
}

fn create_platform_text_style(
    explicit: Option<PlatformTextStyle>,
    span_style: Option<PlatformSpanStyle>,
    paragraph_style: Option<PlatformParagraphStyle>,
) -> Option<PlatformTextStyle> {
    let explicit_span = explicit.and_then(|style| style.span_style);
    let explicit_paragraph = explicit.and_then(|style| style.paragraph_style);
    let span = span_style.or(explicit_span);
    let paragraph = paragraph_style.or(explicit_paragraph);
    if span.is_none() && paragraph.is_none() {
        None
    } else {
        Some(PlatformTextStyle {
            span_style: span,
            paragraph_style: paragraph,
        })
    }
}

fn merge_text_unit(current: TextUnit, incoming: TextUnit) -> TextUnit {
    if matches!(incoming, TextUnit::Unspecified) {
        current
    } else {
        incoming
    }
}

fn merge_text_align(current: TextAlign, incoming: TextAlign) -> TextAlign {
    if matches!(incoming, TextAlign::Unspecified) {
        current
    } else {
        incoming
    }
}

fn merge_text_direction(current: TextDirection, incoming: TextDirection) -> TextDirection {
    if matches!(incoming, TextDirection::Unspecified) {
        current
    } else {
        incoming
    }
}

fn merge_line_break(current: LineBreak, incoming: LineBreak) -> LineBreak {
    if matches!(incoming, LineBreak::Unspecified) {
        current
    } else {
        incoming
    }
}

fn merge_hyphens(current: Hyphens, incoming: Hyphens) -> Hyphens {
    if matches!(incoming, Hyphens::Unspecified) {
        current
    } else {
        incoming
    }
}

/// The span attributes that change how text measures, without its colors,
/// brush, background, decorations or shadow.
pub(crate) fn hash_span_measurement<H: Hasher>(span: &SpanStyle, state: &mut H) {
    hash_text_unit(span.font_size, state);
    span.font_weight.hash(state);
    span.font_style.hash(state);
    span.font_synthesis.hash(state);
    span.font_family.hash(state);
    span.font_feature_settings.hash(state);
    hash_text_unit(span.letter_spacing, state);
    hash_option_baseline_shift(&span.baseline_shift, state);
    hash_option_geometric_transform(&span.text_geometric_transform, state);
    span.locale_list.hash(state);
    span.platform_style.hash(state);
}

fn hash_f32_bits<H: Hasher>(value: f32, state: &mut H) {
    value.to_bits().hash(state);
}

fn hash_option_color<H: Hasher>(color: &Option<Color>, state: &mut H) {
    match color {
        Some(color) => {
            1u8.hash(state);
            color.render_hash().hash(state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_option_brush<H: Hasher>(brush: &Option<Brush>, state: &mut H) {
    match brush {
        Some(brush) => {
            1u8.hash(state);
            brush.render_hash().hash(state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_option_alpha<H: Hasher>(alpha: &Option<f32>, state: &mut H) {
    match alpha {
        Some(alpha) => {
            1u8.hash(state);
            hash_f32_bits(*alpha, state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_text_unit<H: Hasher>(unit: TextUnit, state: &mut H) {
    match unit {
        TextUnit::Unspecified => 0u8.hash(state),
        TextUnit::Sp(value) => {
            1u8.hash(state);
            hash_f32_bits(value, state);
        }
        TextUnit::Em(value) => {
            2u8.hash(state);
            hash_f32_bits(value, state);
        }
    }
}

fn hash_option_baseline_shift<H: Hasher>(shift: &Option<BaselineShift>, state: &mut H) {
    match shift {
        Some(shift) => {
            1u8.hash(state);
            hash_f32_bits(shift.0, state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_option_geometric_transform<H: Hasher>(
    transform: &Option<TextGeometricTransform>,
    state: &mut H,
) {
    match transform {
        Some(transform) => {
            1u8.hash(state);
            hash_f32_bits(transform.scale_x, state);
            hash_f32_bits(transform.skew_x, state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_option_text_indent<H: Hasher>(indent: &Option<TextIndent>, state: &mut H) {
    match indent {
        Some(indent) => {
            1u8.hash(state);
            hash_text_unit(indent.first_line, state);
            hash_text_unit(indent.rest_line, state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_option_shadow<H: Hasher>(shadow: &Option<Shadow>, state: &mut H) {
    match shadow {
        Some(shadow) => {
            1u8.hash(state);
            shadow.color.render_hash().hash(state);
            hash_f32_bits(shadow.offset.x, state);
            hash_f32_bits(shadow.offset.y, state);
            hash_f32_bits(shadow.blur_radius, state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_option_text_draw_style<H: Hasher>(draw_style: &Option<TextDrawStyle>, state: &mut H) {
    match draw_style {
        Some(TextDrawStyle::Fill) => {
            1u8.hash(state);
            0u8.hash(state);
        }
        Some(TextDrawStyle::Stroke { width }) => {
            1u8.hash(state);
            1u8.hash(state);
            hash_f32_bits(*width, state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_span_style<H: Hasher>(span: &SpanStyle, state: &mut H) {
    hash_span_measurement(span, state);
    hash_option_color(&span.color, state);
    hash_option_brush(&span.brush, state);
    hash_option_alpha(&span.alpha, state);
    hash_option_color(&span.background, state);
    span.text_decoration.hash(state);
    hash_option_shadow(&span.shadow, state);
    hash_option_text_draw_style(&span.draw_style, state);
}

fn hash_paragraph_style<H: Hasher>(paragraph: &ParagraphStyle, state: &mut H) {
    paragraph.text_align.hash(state);
    paragraph.text_direction.hash(state);
    hash_text_unit(paragraph.line_height, state);
    hash_option_text_indent(&paragraph.text_indent, state);
    paragraph.platform_style.hash(state);
    paragraph.line_height_style.hash(state);
    paragraph.line_break.hash(state);
    paragraph.hyphens.hash(state);
    paragraph.text_motion.hash(state);
}

#[cfg(test)]
#[path = "tests/style_tests.rs"]
mod tests;
