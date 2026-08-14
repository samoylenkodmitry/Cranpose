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
    estimate_text_measurement, DrawTextMeasurer, FontStyle as DrawFontStyle, Size, TextMeasurement,
    TextStyle as DrawTextStyle,
};

use super::font::{FontFamily, FontStyle, FontWeight};
use super::line_box::LineBox;
use super::style::{SpanStyle, TextStyle};
use super::unit::TextUnit;

/// Builds the [`TextStyle`] that describes a draw-scope text run.
///
/// Everything a `DrawTextStyle` can say is a span attribute except the line
/// height and its policy, so the rest of the paragraph style stays at its
/// defaults — in particular `text_align` is left unspecified, because a draw
/// scope has already resolved alignment into the primitive's rect.
///
/// The policy has to come across. Without it every run drawn through a canvas
/// takes [`line_box`](super::line_box)'s plain branch while a `Text` composable
/// of the same style takes the AOSP one, and a screen that does both puts its
/// two sets of rows a device pixel apart.
pub fn text_style_for_draw_style(style: &DrawTextStyle) -> TextStyle {
    let mut span_style = SpanStyle {
        font_size: TextUnit::Sp(style.resolved_font_size()),
        font_weight: Some(FontWeight::new(style.font_weight.value())),
        font_style: Some(match style.font_style {
            DrawFontStyle::Normal => FontStyle::Normal,
            // No font in the stack ships an oblique face; the renderer
            // synthesizes both the same way.
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
    if let Some(line_height) = style.line_height {
        if line_height.is_finite() && line_height > 0.0 {
            text_style.paragraph_style.line_height = TextUnit::Sp(line_height);
        }
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
/// Every call lands in [`super::measure::measure_resolved_text`], backed by the
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
        // Draw closures normally run inside the app context that owns the
        // fonts. Tooling that runs one standalone gets the font-free estimate
        // rather than a panic.
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
            // Height comes from the line box, not from `metrics.height`: a
            // measurer is free to report a taller box for `min_lines`, and the
            // rasterizer only ever advances by `line_height` per line.
            size: Size::new(metrics.width.max(0.0), line_count as f32 * line_height),
            line_height,
            first_baseline,
            line_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_ui_graphics::{FontWeight as DrawFontWeight, TextAlign, TextVerticalAlign};

    #[test]
    fn draw_style_maps_onto_span_attributes() {
        let style = DrawTextStyle::new(23.0)
            .with_font_family("Fira Sans")
            .with_weight(DrawFontWeight::BOLD)
            .with_style(DrawFontStyle::Italic)
            .with_letter_spacing(1.5)
            .with_line_height(30.0);
        let mapped = text_style_for_draw_style(&style);

        assert_eq!(mapped.span_style.font_size, TextUnit::Sp(23.0));
        assert_eq!(mapped.span_style.font_weight, Some(FontWeight::BOLD));
        assert_eq!(mapped.span_style.font_style, Some(FontStyle::Italic));
        assert_eq!(
            mapped.span_style.font_family,
            Some(FontFamily::Named("Fira Sans".to_string()))
        );
        assert_eq!(mapped.span_style.letter_spacing, TextUnit::Sp(1.5));
        assert_eq!(mapped.paragraph_style.line_height, TextUnit::Sp(30.0));
    }

    #[test]
    fn draw_style_leaves_unset_attributes_unspecified() {
        let mapped = text_style_for_draw_style(&DrawTextStyle::new(14.0));
        assert_eq!(mapped.span_style.font_family, None);
        assert!(mapped.span_style.letter_spacing.is_unspecified());
        assert!(mapped.paragraph_style.line_height.is_unspecified());
        assert_eq!(mapped.paragraph_style.line_height_style, None);
        assert_eq!(mapped.span_style.color, None);
    }

    #[test]
    fn a_drawn_run_and_a_composed_text_resolve_the_same_line_box() {
        // The defect this field exists for: a canvas and a `Text` on one screen
        // took different line-box rules, so every drawn row landed a device
        // pixel off the composed rows beside it. Roboto at 16sp on a density-2
        // watch, in device pixels.
        use crate::text::line_box::{line_box, FontExtent};
        use crate::widgets::wear::wear_line_height_style;

        let extent = FontExtent::new(32.0 * 1900.0 / 2048.0, 32.0 * 500.0 / 2048.0, 0.0);
        let drawn = DrawTextStyle::new(32.0)
            .with_line_height(36.0)
            .with_line_height_style(wear_line_height_style());
        let resolved = line_box(&text_style_for_draw_style(&drawn), extent, 36.0, 1.0);
        let composed = line_box(
            &crate::widgets::wear::WearTextStyle::TITLE_MEDIUM
                .resolve(cranpose_ui_graphics::Color::WHITE),
            extent,
            36.0,
            1.0,
        );
        assert_eq!(resolved, composed);
        // And it is the platform's answer, not the plain split: the font's own
        // extent is 38px, which a 36px line height does not shrink.
        assert_eq!(resolved.height, 38.0);
        assert_eq!(resolved.baseline, 30.0);

        // Without the policy the same run takes the plain branch and sits half
        // a device pixel higher, which is what put the two paths out of step.
        let unstyled = line_box(
            &text_style_for_draw_style(&DrawTextStyle::new(32.0).with_line_height(36.0)),
            extent,
            36.0,
            1.0,
        );
        assert_ne!(unstyled, resolved);
    }

    #[test]
    fn alignment_never_reaches_the_paragraph_style() {
        // A draw scope resolves alignment into the primitive's rect; letting it
        // through here would align the text twice.
        let style = DrawTextStyle::new(14.0)
            .with_align(TextAlign::Center)
            .with_vertical_align(TextVerticalAlign::Bottom);
        let mapped = text_style_for_draw_style(&style);
        assert_eq!(
            mapped.paragraph_style.text_align,
            super::super::paragraph::TextAlign::Unspecified
        );
    }

    #[test]
    fn oblique_and_italic_request_the_same_face() {
        let italic =
            text_style_for_draw_style(&DrawTextStyle::new(14.0).with_style(DrawFontStyle::Italic));
        let oblique =
            text_style_for_draw_style(&DrawTextStyle::new(14.0).with_style(DrawFontStyle::Oblique));
        assert_eq!(italic.span_style.font_style, oblique.span_style.font_style);
    }

    #[test]
    fn degenerate_font_sizes_are_resolved_before_they_reach_the_measurer() {
        for size in [0.0, -3.0, f32::NAN] {
            let mapped = text_style_for_draw_style(&DrawTextStyle::new(size));
            assert_eq!(
                mapped.span_style.font_size,
                TextUnit::Sp(DrawTextStyle::DEFAULT_FONT_SIZE)
            );
        }
    }

    #[test]
    fn measuring_without_an_app_context_falls_back_to_the_estimate() {
        let style = DrawTextStyle::new(16.0);
        assert_eq!(
            AppContextTextMeasurer.measure_text("HELLO", &style),
            estimate_text_measurement("HELLO", &style)
        );
    }
}
