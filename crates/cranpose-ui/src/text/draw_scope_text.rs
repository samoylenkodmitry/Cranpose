//! Bridge between [`DrawScope`](cranpose_ui_graphics::DrawScope) text and the
//! framework text stack.
//!
//! `cranpose-ui-graphics` sits below fonts, so a draw scope describes text with
//! the flat [`DrawTextStyle`] value and delegates measurement back up here. This
//! module owns the single translation from that value into the full
//! [`TextStyle`] the measurer and the rasterizer both consume — so a string
//! measured through [`DrawScope::measure_text`](cranpose_ui_graphics::DrawScope::measure_text)
//! and the same string rasterized by the renderer are described identically,
//! down to the cache key.

use std::rc::Rc;

use cranpose_ui_graphics::{
    DrawTextMeasurer, DrawTextStyle, FontStyle as DrawFontStyle, Size, TextMeasurement,
    estimate_text_measurement,
};

use super::{
    font::{FontFamily, FontStyle, FontWeight},
    line_box::LineBox,
    style::{SpanStyle, TextStyle},
    unit::TextUnit,
};

/// Builds the [`TextStyle`] that describes a draw-scope text run.
///
/// Everything a `DrawTextStyle` can say is a span attribute except the line
/// height and its policy, so the rest of the paragraph style stays at its
/// defaults — in particular `text_align` is left unspecified, because a draw
/// scope has already resolved alignment into the primitive's rect.
///
/// The policy has to come across. Without it every run drawn through a canvas
/// takes [`line_box`](fn@super::line_box)'s plain branch while a `Text` composable
/// of the same style takes the AOSP one, and a screen that does both puts its
/// two sets of rows a device pixel apart.
pub fn text_style_for_draw_style(style: &DrawTextStyle) -> TextStyle {
    let mut span_style = SpanStyle {
        font_size: TextUnit::Sp(style.resolved_font_size()),
        font_weight: Some(FontWeight::new(style.font_weight.value())),
        font_style: Some(match style.font_style {
            DrawFontStyle::Normal => FontStyle::Normal,
            DrawFontStyle::Italic | DrawFontStyle::Oblique => FontStyle::Italic,
        }),
        ..SpanStyle::default()
    };
    if let Some(family) = &style.font_family {
        span_style.font_family = Some(FontFamily::from_name(family));
    }
    let letter_spacing = style.resolved_letter_spacing();
    if letter_spacing != 0.0 {
        span_style.letter_spacing = TextUnit::Sp(letter_spacing);
    }

    let mut text_style = TextStyle::from_span_style(span_style);
    if let Some(line_height) = style.line_height
        && line_height.is_finite()
        && line_height > 0.0
    {
        text_style.paragraph_style.line_height = TextUnit::Sp(line_height);
    }
    text_style.paragraph_style.line_height_style = style.line_height_style;
    text_style
}

/// The line box a draw-scope style resolves to against the app's fonts: how
/// tall one line is and where its baseline sits inside it.
///
/// This is the vertical half of [`DrawScope::measure_text`](cranpose_ui_graphics::DrawScope::measure_text),
/// answerable without a string to measure or a scope to measure in — a layout
/// that stacks rows of a known style needs the row pitch before it has any text
/// for them. `None` when no app context owns the fonts.
///
/// It resolves the style exactly as the measurer does, which means the sizes are
/// taken as stated: a `DrawTextStyle` is already resolved, so the system font
/// scale must not be folded in a second time here.
pub fn draw_style_line_box(style: &DrawTextStyle) -> Option<LineBox> {
    super::measure::resolved_line_box(&text_style_for_draw_style(style))
}

/// Measures draw-scope text against the app's fonts.
///
/// Every call lands in `super::measure::measure_resolved_text`, backed by the
/// app context's metrics cache — so measuring an unchanged string every frame
/// is a hash lookup, not a shaping pass.
///
/// "Resolved" is the whole point: a [`DrawTextStyle`] states final sizes, and a
/// scene lowers a text primitive with `style.resolved_font_size()` untouched,
/// so the system font scale must not be folded in here. It is applied where an
/// unresolved size lives instead — the `Text` composable's `Sp` values — and
/// that path carries the scaled style through to the renderer with it.
#[derive(Clone, Copy, Debug, Default)]
pub struct AppContextTextMeasurer;

impl AppContextTextMeasurer {
    /// A shared measurer to hand to
    /// [`DrawScopeDefault::with_text_measurer`](cranpose_ui_graphics::DrawScopeDefault::with_text_measurer).
    pub fn shared() -> Rc<dyn DrawTextMeasurer> {
        thread_local! {
            static SHARED: Rc<dyn DrawTextMeasurer> = Rc::new(AppContextTextMeasurer);
        }
        SHARED.with(Rc::clone)
    }
}

impl DrawTextMeasurer for AppContextTextMeasurer {
    fn measure_text(&self, text: &str, style: &DrawTextStyle) -> TextMeasurement {
        if crate::render_state::current_app_context().is_none() {
            return estimate_text_measurement(text, style);
        }

        let text_style = text_style_for_draw_style(style);
        let annotated = super::shared_plain_annotated_string(text);
        let metrics = super::measure::measure_resolved_text(&annotated, &text_style);
        let line_height = if metrics.line_height.is_finite() && metrics.line_height > 0.0 {
            metrics.line_height
        } else {
            estimate_text_measurement(text, style).line_height
        };
        let first_baseline = super::measure::resolved_first_baseline(&text_style)
            .unwrap_or_else(|| estimate_text_measurement(text, style).first_baseline);

        if text.is_empty() {
            return TextMeasurement::empty(line_height, first_baseline);
        }

        let line_count = metrics.line_count.max(1);
        TextMeasurement {
            size: Size::new(metrics.width.max(0.0), line_count as f32 * line_height),
            line_height,
            first_baseline,
            line_count,
        }
    }
}

#[cfg(test)]
#[path = "tests/draw_scope_text_tests.rs"]
mod tests;
