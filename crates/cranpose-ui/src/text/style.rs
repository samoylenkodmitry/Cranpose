use super::decoration::{Shadow, TextDecoration};
use super::font::{FontFamily, FontStyle, FontSynthesis, FontWeight};
use super::paragraph::{Hyphens, LineBreak, TextAlign, TextDirection, TextIndent};
use super::unit::TextUnit;
use crate::modifier::Color;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BaselineShift(pub f32);

impl BaselineShift {
    pub const SUPERSCRIPT: Self = Self(0.5);
    pub const SUBSCRIPT: Self = Self(-0.5);
    pub const NONE: Self = Self(0.0);
    pub const UNSPECIFIED: Self = Self(f32::NAN);

    pub fn is_specified(self) -> bool {
        !self.0.is_nan()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextGeometricTransform {
    pub scale_x: f32,
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

#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct LocaleList {
    locales: Vec<String>,
}

impl LocaleList {
    pub fn new(locales: Vec<String>) -> Self {
        Self { locales }
    }

    pub fn from_language_tags(tags: &str) -> Self {
        let locales = tags
            .split(',')
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .map(ToString::to_string)
            .collect();
        Self { locales }
    }

    pub fn locales(&self) -> &[String] {
        &self.locales
    }

    pub fn is_empty(&self) -> bool {
        self.locales.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LineHeightAlignment {
    Top,
    Center,
    #[default]
    Proportional,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LineHeightTrim {
    FirstLineTop,
    LastLineBottom,
    #[default]
    Both,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LineHeightMode {
    #[default]
    Fixed,
    Minimum,
    Tight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LineHeightStyle {
    pub alignment: LineHeightAlignment,
    pub trim: LineHeightTrim,
    pub mode: LineHeightMode,
}

impl Default for LineHeightStyle {
    fn default() -> Self {
        Self {
            alignment: LineHeightAlignment::Proportional,
            trim: LineHeightTrim::Both,
            mode: LineHeightMode::Fixed,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TextMotion {
    #[default]
    Static,
    Animated,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpanStyle {
    pub color: Option<Color>,
    pub font_size: TextUnit,
    pub font_weight: Option<FontWeight>,
    pub font_style: Option<FontStyle>,
    pub font_synthesis: Option<FontSynthesis>,
    pub font_family: Option<FontFamily>,
    pub font_feature_settings: Option<String>,
    pub letter_spacing: TextUnit,
    pub baseline_shift: Option<BaselineShift>,
    pub text_geometric_transform: Option<TextGeometricTransform>,
    pub locale_list: Option<LocaleList>,
    pub background: Option<Color>,
    pub text_decoration: Option<TextDecoration>,
    pub shadow: Option<Shadow>,
}

impl Default for SpanStyle {
    fn default() -> Self {
        Self {
            color: None,
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
        }
    }
}

impl SpanStyle {
    pub fn merge(&self, other: &SpanStyle) -> SpanStyle {
        SpanStyle {
            color: other.color.or(self.color),
            font_size: merge_text_unit(self.font_size, other.font_size),
            font_weight: other.font_weight.or(self.font_weight),
            font_style: other.font_style.or(self.font_style),
            font_synthesis: other.font_synthesis.or(self.font_synthesis),
            font_family: other.font_family.clone().or(self.font_family.clone()),
            font_feature_settings: other
                .font_feature_settings
                .clone()
                .or(self.font_feature_settings.clone()),
            letter_spacing: merge_text_unit(self.letter_spacing, other.letter_spacing),
            baseline_shift: other.baseline_shift.or(self.baseline_shift),
            text_geometric_transform: other
                .text_geometric_transform
                .or(self.text_geometric_transform),
            locale_list: other.locale_list.clone().or(self.locale_list.clone()),
            background: other.background.or(self.background),
            text_decoration: other.text_decoration.or(self.text_decoration),
            shadow: other.shadow.or(self.shadow),
        }
    }

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
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParagraphStyle {
    pub text_align: TextAlign,
    pub text_direction: TextDirection,
    pub line_height: TextUnit,
    pub text_indent: Option<TextIndent>,
    pub line_height_style: Option<LineHeightStyle>,
    pub line_break: LineBreak,
    pub hyphens: Hyphens,
    pub text_motion: Option<TextMotion>,
}

impl Default for ParagraphStyle {
    fn default() -> Self {
        Self {
            text_align: TextAlign::Unspecified,
            text_direction: TextDirection::Unspecified,
            line_height: TextUnit::Unspecified,
            text_indent: None,
            line_height_style: None,
            line_break: LineBreak::Unspecified,
            hyphens: Hyphens::Unspecified,
            text_motion: None,
        }
    }
}

impl ParagraphStyle {
    pub fn merge(&self, other: &ParagraphStyle) -> ParagraphStyle {
        ParagraphStyle {
            text_align: merge_text_align(self.text_align, other.text_align),
            text_direction: merge_text_direction(self.text_direction, other.text_direction),
            line_height: merge_text_unit(self.line_height, other.line_height),
            text_indent: other.text_indent.or(self.text_indent),
            line_height_style: other.line_height_style.or(self.line_height_style),
            line_break: merge_line_break(self.line_break, other.line_break),
            hyphens: merge_hyphens(self.hyphens, other.hyphens),
            text_motion: other.text_motion.or(self.text_motion),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct TextStyle {
    pub span_style: SpanStyle,
    pub paragraph_style: ParagraphStyle,
}

impl TextStyle {
    pub fn new(span_style: SpanStyle, paragraph_style: ParagraphStyle) -> Self {
        Self {
            span_style,
            paragraph_style,
        }
    }

    pub fn merge(&self, other: &TextStyle) -> TextStyle {
        TextStyle {
            span_style: self.span_style.merge(&other.span_style),
            paragraph_style: self.paragraph_style.merge(&other.paragraph_style),
        }
    }

    pub fn resolve_font_size(&self, default_size: f32) -> f32 {
        self.span_style.resolve_font_size(default_size)
    }

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

    pub fn resolve_letter_spacing(&self, default_size: f32) -> f32 {
        let font_size = self.resolve_font_size(default_size);
        match self.span_style.letter_spacing {
            TextUnit::Sp(value) if value.is_finite() => value,
            TextUnit::Em(value) if value.is_finite() => value * font_size,
            _ => 0.0,
        }
    }

    pub fn measurement_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        let span = &self.span_style;
        let paragraph = &self.paragraph_style;

        hash_text_unit(span.font_size, &mut hasher);
        span.font_weight.hash(&mut hasher);
        span.font_style.hash(&mut hasher);
        span.font_synthesis.hash(&mut hasher);
        span.font_family.hash(&mut hasher);
        span.font_feature_settings.hash(&mut hasher);
        hash_text_unit(span.letter_spacing, &mut hasher);
        hash_option_baseline_shift(&span.baseline_shift, &mut hasher);
        hash_option_geometric_transform(&span.text_geometric_transform, &mut hasher);
        span.locale_list.hash(&mut hasher);

        paragraph.text_align.hash(&mut hasher);
        paragraph.text_direction.hash(&mut hasher);
        hash_text_unit(paragraph.line_height, &mut hasher);
        hash_option_text_indent(&paragraph.text_indent, &mut hasher);
        paragraph.line_height_style.hash(&mut hasher);
        paragraph.line_break.hash(&mut hasher);
        paragraph.hyphens.hash(&mut hasher);
        paragraph.text_motion.hash(&mut hasher);

        hasher.finish()
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

fn hash_f32_bits<H: Hasher>(value: f32, state: &mut H) {
    value.to_bits().hash(state);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{FontFamily, TextDirection};

    #[test]
    fn baseline_shift_reports_specified() {
        assert!(BaselineShift::SUPERSCRIPT.is_specified());
        assert!(!BaselineShift::UNSPECIFIED.is_specified());
    }

    #[test]
    fn locale_list_parses_language_tags() {
        let locale_list = LocaleList::from_language_tags("en-US, ar-EG, ja-JP");
        assert_eq!(locale_list.locales(), &["en-US", "ar-EG", "ja-JP"]);
    }

    #[test]
    fn span_style_merge_prefers_incoming_specified_values() {
        let base = SpanStyle {
            font_size: TextUnit::Sp(14.0),
            font_family: Some(FontFamily::Serif),
            ..Default::default()
        };
        let incoming = SpanStyle {
            font_size: TextUnit::Unspecified,
            letter_spacing: TextUnit::Em(0.1),
            ..Default::default()
        };

        let merged = base.merge(&incoming);
        assert_eq!(merged.font_size, TextUnit::Sp(14.0));
        assert_eq!(merged.letter_spacing, TextUnit::Em(0.1));
        assert_eq!(merged.font_family, Some(FontFamily::Serif));
    }

    #[test]
    fn paragraph_style_merge_prefers_specified_values() {
        let base = ParagraphStyle {
            text_direction: TextDirection::Ltr,
            line_height: TextUnit::Sp(18.0),
            ..Default::default()
        };
        let incoming = ParagraphStyle {
            text_direction: TextDirection::Unspecified,
            line_height: TextUnit::Em(1.4),
            ..Default::default()
        };

        let merged = base.merge(&incoming);
        assert_eq!(merged.text_direction, TextDirection::Ltr);
        assert_eq!(merged.line_height, TextUnit::Em(1.4));
    }

    #[test]
    fn resolve_font_size_uses_specified_value() {
        let style = TextStyle::new(
            SpanStyle {
                font_size: TextUnit::Sp(18.0),
                ..Default::default()
            },
            ParagraphStyle::default(),
        );
        assert_eq!(style.resolve_font_size(14.0), 18.0);
    }

    #[test]
    fn resolve_font_size_handles_em_units() {
        let style = TextStyle::new(
            SpanStyle {
                font_size: TextUnit::Em(1.5),
                ..Default::default()
            },
            ParagraphStyle::default(),
        );
        assert_eq!(style.resolve_font_size(16.0), 24.0);
    }

    #[test]
    fn resolve_line_height_uses_style_value() {
        let style = TextStyle::new(
            SpanStyle {
                font_size: TextUnit::Sp(20.0),
                ..Default::default()
            },
            ParagraphStyle {
                line_height: TextUnit::Em(1.2),
                ..Default::default()
            },
        );
        assert_eq!(style.resolve_line_height(14.0, 18.0), 24.0);
    }

    #[test]
    fn text_style_merge_combines_span_and_paragraph() {
        let base = TextStyle::new(
            SpanStyle {
                font_family: Some(FontFamily::SansSerif),
                ..Default::default()
            },
            ParagraphStyle {
                text_direction: TextDirection::Ltr,
                ..Default::default()
            },
        );
        let incoming = TextStyle::new(
            SpanStyle {
                letter_spacing: TextUnit::Em(0.2),
                ..Default::default()
            },
            ParagraphStyle {
                line_height: TextUnit::Sp(22.0),
                ..Default::default()
            },
        );

        let merged = base.merge(&incoming);
        assert_eq!(merged.span_style.font_family, Some(FontFamily::SansSerif));
        assert_eq!(merged.span_style.letter_spacing, TextUnit::Em(0.2));
        assert_eq!(merged.paragraph_style.text_direction, TextDirection::Ltr);
        assert_eq!(merged.paragraph_style.line_height, TextUnit::Sp(22.0));
    }

    #[test]
    fn measurement_hash_changes_when_measurement_attributes_change() {
        let style_a = TextStyle::default();
        let style_b = TextStyle::new(
            SpanStyle {
                font_family: Some(FontFamily::SansSerif),
                ..Default::default()
            },
            ParagraphStyle {
                text_direction: TextDirection::Rtl,
                ..Default::default()
            },
        );

        assert_ne!(style_a.measurement_hash(), style_b.measurement_hash());
    }
}
