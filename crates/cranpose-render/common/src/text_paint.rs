//! What a renderer reads of a text's style to draw its glyphs, worked out
//! once per text node so that drawing a plain text never loads its style.

use std::cell::Cell;

use cranpose_ui::{
    Brush, Color, TextStyle,
    text::{RangeStyle, SpanStyle, TextDecoration, TextDrawStyle},
};

/// The paint of a text node's glyphs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextPaint {
    /// Whether the glyphs in `color` are all the text draws: no background,
    /// shadow, brush, stroke or decoration, and no span that paints its own
    /// range.
    pub plain: bool,
    /// The style's colour, or its solid brush, times its alpha; white when
    /// it names neither.
    pub color: Color,
    /// How far the style's baseline shift moves the glyphs down.
    pub baseline_shift: f32,
    /// The style's [`TextStyle::render_hash`], which keys the glyph caches
    /// a renderer draws the text from, so no frame hashes the style again.
    pub style_hash: u64,
}

impl TextPaint {
    /// The paint of a text in `style` with `spans`, at `font_size`.
    pub fn of(style: &TextStyle, spans: &[RangeStyle<SpanStyle>], font_size: f32) -> Self {
        let span_style = &style.span_style;
        let plain = span_style.background.is_none()
            && span_style.shadow.is_none()
            && span_style.brush.is_none()
            && !matches!(
                span_style.draw_style,
                Some(TextDrawStyle::Stroke { width }) if width.is_finite() && width > 0.0
            )
            && !spans_override_foreground(spans)
            && !has_visible_decoration(spans, style);
        Self {
            plain,
            color: resolve_text_color_without_gradient_fallback(style, Color::WHITE),
            baseline_shift: span_style
                .baseline_shift
                .filter(|shift| shift.is_specified())
                .map_or(0.0, |shift| -(shift.0 * font_size)),
            style_hash: style.render_hash(),
        }
    }
}

/// A text node's [`TextPaint`], set when the scene builds the node or when a
/// renderer first asks for it. It is derived data, so two caches always
/// compare equal.
#[derive(Clone, Debug, Default)]
pub struct TextPaintCache(Cell<Option<TextPaint>>);

impl TextPaintCache {
    /// A cache holding `paint`.
    pub fn holding(paint: TextPaint) -> Self {
        Self(Cell::new(Some(paint)))
    }

    /// The cached paint, or the one `compute` returns, which the cache then
    /// keeps.
    pub fn get_or(&self, compute: impl FnOnce() -> TextPaint) -> TextPaint {
        if let Some(paint) = self.0.get() {
            return paint;
        }
        let paint = compute();
        self.0.set(Some(paint));
        paint
    }
}

impl PartialEq for TextPaintCache {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

/// The text colour a style names, its solid brush's colour, or `default`,
/// times the style's alpha.
pub fn resolve_text_color_without_gradient_fallback(
    text_style: &TextStyle,
    default: Color,
) -> Color {
    let mut color = text_style
        .span_style
        .color
        .or(match text_style.span_style.brush.as_ref() {
            Some(Brush::Solid(color)) => Some(*color),
            _ => None,
        })
        .unwrap_or(default);
    if let Some(alpha) = text_style.span_style.alpha {
        color.3 *= alpha.clamp(0.0, 1.0);
    }
    color
}

fn span_has_foreground_override(span_style: &SpanStyle) -> bool {
    matches!(
        span_style.brush.as_ref(),
        Some(
            Brush::LinearGradient { .. }
                | Brush::RadialGradient { .. }
                | Brush::SweepGradient { .. }
        )
    ) || span_style.alpha.is_some()
        || span_style.draw_style.is_some()
}

/// Whether a span paints its range with a gradient, an alpha or a draw
/// style of its own.
pub fn spans_override_foreground(spans: &[RangeStyle<SpanStyle>]) -> bool {
    spans
        .iter()
        .any(|span| span_has_foreground_override(&span.item))
}

/// Whether a span gives its range a colour or a solid brush of its own.
pub fn spans_override_foreground_color(spans: &[RangeStyle<SpanStyle>]) -> bool {
    spans
        .iter()
        .any(|span| span.item.color.is_some() || matches!(span.item.brush, Some(Brush::Solid(_))))
}

/// Whether the style or a span draws an underline or a line-through.
pub fn has_visible_decoration(spans: &[RangeStyle<SpanStyle>], global_style: &TextStyle) -> bool {
    let visible = |decoration: Option<TextDecoration>| {
        decoration.is_some_and(|decoration| decoration != TextDecoration::NONE)
    };
    visible(global_style.span_style.text_decoration)
        || spans.iter().any(|span| visible(span.item.text_decoration))
}
