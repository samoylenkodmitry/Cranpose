use std::ops::Range;

use cranpose_text::{
    FontFamily, FontFamilyResolver, FontFeature, FontRequest, FontStyle, FontWeight,
    ParagraphLengths, ShapingSpan, font_feature_settings,
};
use smallvec::SmallVec;

use super::{AnnotatedString, SpanStyle, TextStyle, TextUnit};
use crate::density::Density;

// Compose's default text size.
pub(crate) const DEFAULT_FONT_SIZE_SP: f32 = 14.0;

// How a text's sizes become pixels: the density and its font scale, and a
// factor every text size is multiplied by, which auto-size searches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextUnits {
    pub(crate) density: Density,
    pub(crate) factor: f32,
}

impl TextUnits {
    // A size in sp, in pixels.
    fn sp(self, sp: f32) -> f32 {
        self.density.font_scale_curve().sp_to_dp(sp) * self.factor
    }
}

// The paragraph of a text in a style as the engine takes it: its spans,
// span styles merged over the style's, with sizes in pixels, and the
// paragraph's lengths. Stretches that differ only in what does not change
// shaping, such as color, are one span.
pub(crate) struct ParagraphSpans<'a> {
    // Most text is one stretch with no features, which these hold without
    // allocating or moving room for more.
    stretches: SmallVec<[Stretch<'a>; 1]>,
    features: SmallVec<[FontFeature; 1]>,
    units: TextUnits,
    // The paragraph's line height, indents and device pixel grid.
    pub(crate) lengths: ParagraphLengths,
}

// A stretch of text with the settings it shapes with and the range of its
// features among the paragraph's.
struct Stretch<'a> {
    text: Range<usize>,
    style: ShapingStyle<'a>,
    features: Range<usize>,
}

// The settings a stretch of text shapes with: its span styles over the text
// style's, the size in pixels.
#[derive(Clone, Copy, PartialEq)]
struct ShapingStyle<'a> {
    family: &'a FontFamily,
    weight: FontWeight,
    style: FontStyle,
    size: f32,
    letter_spacing: TextUnit,
    features: Option<&'a str>,
    language: Option<&'a str>,
    baseline_shift: f32,
}

impl<'a> ShapingStyle<'a> {
    // The settings of `span` over these.
    fn over(mut self, span: &'a SpanStyle, units: TextUnits) -> Self {
        if let Some(family) = &span.font_family {
            self.family = family;
        }
        if let Some(weight) = span.font_weight {
            self.weight = weight;
        }
        if let Some(style) = span.font_style {
            self.style = style;
        }
        self.size = match span.font_size {
            TextUnit::Sp(sp) if sp.is_finite() && sp > 0.0 => units.sp(sp),
            TextUnit::Em(em) if em.is_finite() && em > 0.0 => em * self.size,
            _ => self.size,
        };
        if span.letter_spacing != TextUnit::Unspecified {
            self.letter_spacing = span.letter_spacing;
        }
        if let Some(settings) = &span.font_feature_settings {
            self.features = Some(settings);
        }
        if let Some(locales) = &span.locale_list {
            self.language = locales.locales().first().map(String::as_str);
        }
        if let Some(shift) = span.baseline_shift.filter(|shift| shift.is_specified()) {
            self.baseline_shift = shift.0;
        }
        self
    }
}

impl<'a> ParagraphSpans<'a> {
    // The spans of `text` in `style` with sizes in `units`.
    pub(crate) fn new(text: &'a AnnotatedString, style: &'a TextStyle, units: TextUnits) -> Self {
        let size = units.sp(style.resolve_font_size(DEFAULT_FONT_SIZE_SP));
        let base = ShapingStyle {
            family: &FontFamily::Default,
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
            size,
            letter_spacing: TextUnit::Unspecified,
            features: None,
            language: None,
            baseline_shift: 0.0,
        }
        .over(&style.span_style, units);
        // The style's own size is its resolved size, `Em` against 14 sp.
        let base = ShapingStyle { size, ..base };
        let mut stretches = stretches(text, base, units);
        let mut features = SmallVec::new();
        // Most text asks for no features.
        for stretch in &mut stretches {
            if let Some(settings) = stretch.style.features {
                let start = features.len();
                features.extend(font_feature_settings(settings));
                stretch.features = start..features.len();
            }
        }
        let paragraph = &style.paragraph_style;
        let indent = paragraph.text_indent.unwrap_or_default();
        let length = |unit: TextUnit| match unit {
            TextUnit::Sp(sp) if sp.is_finite() => Some(units.sp(sp)),
            TextUnit::Em(em) if em.is_finite() => Some(em * size),
            _ => None,
        };
        let lengths = ParagraphLengths {
            line_height: length(paragraph.line_height),
            first_line_indent: length(indent.first_line).unwrap_or(0.0),
            rest_line_indent: length(indent.rest_line).unwrap_or(0.0),
            grid: units.density.density(),
        };
        Self {
            stretches,
            features,
            units,
            lengths,
        }
    }

    // The spans, with the typefaces of `resolver` setting their baseline
    // shifts.
    pub(crate) fn spans(&self, resolver: &FontFamilyResolver) -> SmallVec<[ShapingSpan<'_>; 1]> {
        self.stretches
            .iter()
            .map(|stretch| {
                let style = &stretch.style;
                let font = FontRequest {
                    family: style.family,
                    weight: style.weight,
                    style: style.style,
                };
                ShapingSpan {
                    range: stretch.text.clone(),
                    font,
                    size: style.size,
                    features: &self.features[stretch.features.clone()],
                    language: style.language,
                    letter_spacing: match style.letter_spacing {
                        TextUnit::Sp(sp) if sp.is_finite() => Some(self.units.sp(sp)),
                        TextUnit::Em(em) if em.is_finite() => Some(em * style.size),
                        _ => None,
                    },
                    baseline_shift: self.baseline_shift(resolver, font, style),
                }
            })
            .collect()
    }

    // How far above the line's baseline a stretch's baseline sits, in
    // pixels: Compose's `BaselineShiftSpan`, the shift times the ascent of
    // the font the stretch asks for, cut to whole device pixels.
    fn baseline_shift(
        &self,
        resolver: &FontFamilyResolver,
        font: FontRequest<'_>,
        stretch: &ShapingStyle<'_>,
    ) -> f32 {
        // No shift, or none a number states.
        if !stretch.baseline_shift.is_normal() {
            return 0.0;
        }
        let Some(typeface) = resolver.resolve(font) else {
            return 0.0;
        };
        let grid = self.lengths.grid;
        let ascent =
            typeface.line_metrics().ascent * stretch.size / f32::from(typeface.units_per_em());
        (ascent * stretch.baseline_shift * grid).trunc() / grid
    }
}

// The stretches of `text` between its span boundaries with the settings
// each shapes with, adjacent stretches with the same settings joined.
fn stretches<'a>(
    text: &'a AnnotatedString,
    base: ShapingStyle<'a>,
    units: TextUnits,
) -> SmallVec<[Stretch<'a>; 1]> {
    let stretch = |text: Range<usize>, style: ShapingStyle<'a>| Stretch {
        text,
        style,
        features: 0..0,
    };
    let mut stretches: SmallVec<[Stretch<'a>; 1]> = SmallVec::new();
    if text.span_styles.is_empty() {
        stretches.push(stretch(0..text.text.len(), base));
        return stretches;
    }
    for edges in text.span_boundaries().windows(2) {
        let range = edges[0]..edges[1];
        let style = text
            .span_styles
            .iter()
            .filter(|span| span.range.start <= range.start && range.end <= span.range.end)
            .fold(base, |style, span| style.over(&span.item, units));
        match stretches.last_mut() {
            Some(last) if last.style == style => last.text.end = range.end,
            _ => stretches.push(stretch(range, style)),
        }
    }
    if stretches.is_empty() {
        stretches.push(stretch(0..0, base));
    }
    stretches
}
