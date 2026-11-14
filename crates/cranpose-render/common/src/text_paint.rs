//! What a renderer reads of a text's style and string to draw its glyphs,
//! worked out once per text node so that drawing a plain text never loads
//! its style, and drawing a cached run never loads its string.

use std::{cell::OnceCell, hash::Hasher, sync::Arc};

use cranpose_text::SynthesizedStyle;
use cranpose_ui::{
    Brush, Color, TextStyle,
    text::{RangeStyle, SpanStyle, TextDecoration, TextDrawStyle, TextMotion},
};
use cranpose_ui_graphics::FxHasher;

use crate::glyph_reach::InkSpread;

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
    /// Whether the style's motion is static, so its glyphs stay on whole
    /// pixels.
    pub static_motion: bool,
    /// Whether the style or a span casts a shadow, which spreads past the
    /// glyphs.
    pub casts_shadow: bool,
    /// How far past their outlines the glyphs put ink: a stroke's outer
    /// half, and the bold or italic a face of the layout synthesizes.
    pub spread: InkSpread,
    // The font size and the bold and italic the layout synthesizes, which
    // a paint shared among texts of one style was worked out at.
    font_size: f32,
    synthesized: SynthesizedStyle,
}

impl TextPaint {
    /// The paint of a text in `style`, whose [`TextStyle::render_hash`] is
    /// `style_hash`, with `spans`, at `font_size`, laid out as `layout`.
    pub fn of(
        style: &TextStyle,
        style_hash: u64,
        spans: &[RangeStyle<SpanStyle>],
        (font_size, layout): (f32, &cranpose_text::TextLayout),
    ) -> Self {
        let span_style = &style.span_style;
        let span_paint = SpanPaint::of(spans);
        let plain = span_style.background.is_none()
            && span_style.shadow.is_none()
            && span_style.brush.is_none()
            && !matches!(
                span_style.draw_style,
                Some(TextDrawStyle::Stroke { width }) if width.is_finite() && width > 0.0
            )
            && !span_paint.foreground
            && !decorates(span_style.text_decoration)
            && !span_paint.decoration;
        Self {
            plain,
            color: resolve_text_color_without_gradient_fallback(style, Color::WHITE),
            baseline_shift: span_style
                .baseline_shift
                .filter(|shift| shift.is_specified())
                .map_or(0.0, |shift| -(shift.0 * font_size)),
            style_hash,
            static_motion: style
                .paragraph_style
                .text_motion
                .unwrap_or(TextMotion::Static)
                == TextMotion::Static,
            casts_shadow: span_style.shadow.is_some() || span_paint.shadow,
            // A plain text and its spans draw no stroke, and most cast no
            // shadow.
            spread: if plain && !span_paint.shadow {
                InkSpread::of(0.0, layout.synthesized_style())
            } else {
                InkSpread::of_text(style, spans, layout)
            },
            font_size,
            synthesized: layout.synthesized_style(),
        }
    }

    // Whether the paint is that of a text without spans in the style whose
    // render hash is `style_hash`, at `font_size`, with `synthesized`.
    fn made_at(&self, style_hash: u64, font_size: f32, synthesized: SynthesizedStyle) -> bool {
        self.style_hash == style_hash
            && self.font_size.to_bits() == font_size.to_bits()
            && self.synthesized == synthesized
    }
}

// The paint of a text without spans in `style`, whose render hash is
// `style_hash`, at `font_size`, laid out as `layout`: the one the thread
// worked out for another text of that style, at that size and with the
// same bold and italic synthesized, which most labels of a screen share.
fn shared_paint(
    style: &TextStyle,
    style_hash: u64,
    (font_size, layout): (f32, &cranpose_text::TextLayout),
) -> Arc<TextPaint> {
    let synthesized = layout.synthesized_style();
    let bold = synthesized.bold_size.map_or(0, f32::to_bits);
    let mut key = FxHasher::default();
    key.write_u64(style_hash);
    key.write_u64(u64::from(font_size.to_bits()) << 32 | u64::from(bold));
    key.write_u8(u8::from(synthesized.italic) | u8::from(synthesized.bold_size.is_some()) << 1);
    crate::thread_state::with(|state| {
        state.text_paints.borrow_mut().with(
            key.finish(),
            |kept| kept.made_at(style_hash, font_size, synthesized),
            || Arc::new(TextPaint::of(style, style_hash, &[], (font_size, layout))),
            Arc::clone,
        )
    })
}

// What the spans of a text paint of their own.
struct SpanPaint {
    // A span paints its range with a gradient, an alpha or a draw style.
    foreground: bool,
    // A span draws a visible decoration.
    decoration: bool,
    // A span casts a shadow.
    shadow: bool,
}

impl SpanPaint {
    // What `spans` paint, found in one pass.
    fn of(spans: &[RangeStyle<SpanStyle>]) -> Self {
        let none = Self {
            foreground: false,
            decoration: false,
            shadow: false,
        };
        spans.iter().fold(none, |paint, span| Self {
            foreground: paint.foreground | span_has_foreground_override(&span.item),
            decoration: paint.decoration | decorates(span.item.text_decoration),
            shadow: paint.shadow | span.item.shadow.is_some(),
        })
    }
}

// Whether `decoration` draws a line.
fn decorates(decoration: Option<TextDecoration>) -> bool {
    decoration.is_some_and(|decoration| decoration != TextDecoration::NONE)
}

/// A text node's [`TextPaint`], set when the scene builds the node or when a
/// renderer first asks for it. It is derived data, so two caches always
/// compare equal.
#[derive(Clone, Debug, Default)]
pub struct TextPaintCache(OnceCell<Arc<TextPaint>>);

impl TextPaintCache {
    /// A cache holding the paint of a text in `style`, whose
    /// [`TextStyle::render_hash`] is `style_hash`, with `spans`, at
    /// `font_size`, laid out as `layout`: shared with the other texts the
    /// thread made in that style when the text has no spans.
    pub fn of_text(
        style: &TextStyle,
        style_hash: u64,
        spans: &[RangeStyle<SpanStyle>],
        text: (f32, &cranpose_text::TextLayout),
    ) -> Self {
        let paint = if spans.is_empty() {
            shared_paint(style, style_hash, text)
        } else {
            Arc::new(TextPaint::of(style, style_hash, spans, text))
        };
        Self(OnceCell::from(paint))
    }

    /// The cached paint, or the one `compute` returns, which the cache then
    /// keeps.
    pub fn get_or(&self, compute: impl FnOnce() -> TextPaint) -> &TextPaint {
        self.0.get_or_init(|| Arc::new(compute()))
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

/// Whether a span casts a shadow of its own.
pub fn spans_cast_shadow(spans: &[RangeStyle<SpanStyle>]) -> bool {
    spans.iter().any(|span| span.item.shadow.is_some())
}

/// Whether a span gives its range a colour or a solid brush of its own.
pub fn spans_override_foreground_color(spans: &[RangeStyle<SpanStyle>]) -> bool {
    spans
        .iter()
        .any(|span| span.item.color.is_some() || matches!(span.item.brush, Some(Brush::Solid(_))))
}
