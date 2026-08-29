//! Where a line of text sits inside the height it was given.
//!
//! [`LineHeightStyle`] has been a declared-but-unread field on
//! [`ParagraphStyle`](crate::text::ParagraphStyle) since it was added: nothing
//! outside `merge` and the hash keys ever looked at it, and the rasterizer's
//! line box was a fixed rule — the box is exactly the requested line height,
//! and the leading is split evenly above and below. That rule is not what
//! Android does, and the difference is visible.
//!
//! AOSP's `StaticLayout` differs in four ways that each move a glyph row:
//!
//! - the font's ascent and descent are **whole pixels**, rounded the way
//!   `Paint.getFontMetricsInt()` rounds them, and the line is built from that
//!   pair rather than from the float metrics;
//! - the line advance is a **whole pixel**, `ceil`ed, not a float;
//! - a requested line height **shorter than the font's own ascent + descent
//!   does not shrink the line** — the font wins, which is why a 16sp/18sp
//!   style lays out in 38px rather than 36px at density 2;
//! - the leading is split with the **odd pixel below** the baseline, not above.
//!
//! [`line_box`] implements that, and it implements it **only when the caller
//! asked for it**. A style whose `line_height_style` is `None` gets exactly the
//! arithmetic it got before, bit for bit. That is deliberate: the rule changes
//! where every glyph lands, and it is not a change to make silently on behalf
//! of text that never asked. The Wear widgets ask for it through
//! [`WearTextStyle`](crate::widgets::wear::WearTextStyle), and a
//! [`DrawScope`](cranpose_ui_graphics::DrawScope) run asks for it through
//! [`TextStyle::with_line_height_style`](cranpose_ui_graphics::TextStyle::with_line_height_style)
//! — which is what lets a canvas and a `Text` on one screen agree.

use crate::text::style::{
    LineHeightAlignment, LineHeightMode, LineHeightStyle, LineHeightTrim, TextStyle,
};

/// A resolved line box: how tall the line is and where its baseline sits inside
/// it, both measured down from the top of the box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineBox {
    /// Baseline-to-baseline advance, and the height of a single-line block.
    pub height: f32,
    /// Distance from the top of the box down to the baseline.
    pub baseline: f32,
}

/// The font's own vertical extent, in the same unit as the line height.
///
/// `ascent` and `descent` are both **positive distances** from the baseline,
/// which is the sign convention AOSP states its rule in and the opposite of the
/// one `ab_glyph` reports `descent` in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontExtent {
    pub ascent: f32,
    pub descent: f32,
    /// `hhea.lineGap`. Only read when a style asks for font padding.
    pub line_gap: f32,
}

impl FontExtent {
    pub fn new(ascent: f32, descent: f32, line_gap: f32) -> Self {
        Self {
            ascent,
            descent,
            line_gap,
        }
    }

    /// Ascent plus descent — the height the font needs with no leading at all.
    pub fn natural(self) -> f32 {
        self.ascent + self.descent
    }
}

/// The line box a style asks for, given the font's extent and the line height
/// already resolved from the style's own units.
///
/// `asked` is the line height in the same unit as the extent. `grid` is how
/// many device pixels there are to one of those units, and it is what every
/// rounding in the AOSP rule is done against — pass `1.0` when the values are
/// already device pixels, or the density when they are layout points. Getting
/// it wrong does not shift a baseline by a fraction; it quantises the whole
/// line box to the wrong step.
pub fn line_box(style: &TextStyle, extent: FontExtent, asked: f32, grid: f32) -> LineBox {
    let grid = if grid.is_finite() && grid > 0.0 {
        grid
    } else {
        1.0
    };
    match style.paragraph_style.line_height_style {
        None => unstyled_line_box(extent, asked, grid),
        Some(line_height_style) => {
            let padding = font_padding(style, extent);
            aosp_line_box(line_height_style, extent, asked, padding, grid)
        }
    }
}

fn unstyled_line_box(extent: FontExtent, asked: f32, grid: f32) -> LineBox {
    let natural = (extent.natural() * grid).ceil() / grid;
    LineBox {
        height: asked,
        baseline: extent.ascent + (asked - natural) * 0.5,
    }
}

fn font_padding(style: &TextStyle, extent: FontExtent) -> f32 {
    let asked = style
        .paragraph_style
        .platform_style
        .and_then(|platform| platform.include_font_padding)
        .unwrap_or(false);
    if asked && extent.line_gap.is_finite() && extent.line_gap > 0.0 {
        extent.line_gap
    } else {
        0.0
    }
}

fn aosp_line_box(
    style: LineHeightStyle,
    extent: FontExtent,
    asked: f32,
    padding: f32,
    grid: f32,
) -> LineBox {
    let up = |value: f32| (value * grid).ceil() / grid;
    let down = |value: f32| (value * grid).floor() / grid;
    let round = |value: f32| ((value * grid) + 0.5).floor() / grid;
    let ascent = -round(-extent.ascent.max(0.0));
    let descent = round(extent.descent.max(0.0));
    let above_padding = down(padding * 0.5);
    let below_padding = padding - above_padding;
    let natural = up(ascent + descent + padding);
    let asked = if asked.is_finite() {
        up(asked)
    } else {
        natural
    };

    let height = match style.mode {
        LineHeightMode::Fixed => asked.max(1.0),
        LineHeightMode::Minimum => asked.max(natural).max(1.0),
        LineHeightMode::Tight => natural.max(1.0),
    };

    let leading = height - (ascent + descent + padding);
    let (mut above, mut below) = match style.alignment {
        LineHeightAlignment::Top => (0.0, leading),
        LineHeightAlignment::Bottom => (leading, 0.0),
        LineHeightAlignment::Center => {
            let below = up(leading * 0.5);
            (leading - below, below)
        }
        LineHeightAlignment::Proportional => {
            let total = ascent + descent;
            if total > 0.0 {
                let above = leading * (ascent / total);
                (above, leading - above)
            } else {
                (leading * 0.5, leading * 0.5)
            }
        }
    };
    above += above_padding;
    below += below_padding;

    let (trim_above, trim_below) = match style.trim {
        LineHeightTrim::None => (false, false),
        LineHeightTrim::FirstLineTop => (true, false),
        LineHeightTrim::LastLineBottom => (false, true),
        LineHeightTrim::Both => (true, true),
    };
    let mut height = height;
    if trim_above {
        height -= above;
        above = 0.0;
    }
    if trim_below {
        height -= below;
    }

    LineBox {
        height: height.max(1.0),
        baseline: above + ascent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{
        TextUnit,
        style::{ParagraphStyle, PlatformParagraphStyle},
    };

    fn roboto_16sp() -> FontExtent {
        FontExtent::new(32.0 * 1900.0 / 2048.0, 32.0 * 500.0 / 2048.0, 0.0)
    }

    fn styled(line_height_px: f32, line_height_style: Option<LineHeightStyle>) -> TextStyle {
        TextStyle {
            paragraph_style: ParagraphStyle {
                line_height: TextUnit::Sp(line_height_px),
                line_height_style,
                ..ParagraphStyle::default()
            },
            ..TextStyle::default()
        }
    }

    fn wear() -> LineHeightStyle {
        LineHeightStyle {
            alignment: LineHeightAlignment::Center,
            trim: LineHeightTrim::None,
            mode: LineHeightMode::Minimum,
        }
    }

    #[test]
    fn a_style_that_asks_for_nothing_gets_exactly_what_it_got_before() {
        let extent = roboto_16sp();
        let plain = line_box(&styled(36.0, None), extent, 36.0, 1.0);
        assert_eq!(plain.height, 36.0);
        let natural = extent.natural().ceil();
        assert_eq!(plain.baseline, extent.ascent + (36.0 - natural) * 0.5);
    }

    #[test]
    fn an_unstyled_box_is_the_same_box_measured_in_points_or_in_pixels() {
        let density = 2.0;
        for glyph_px in [24.0f32, 26.0, 28.125, 30.0, 32.0, 37.2, 38.72] {
            let ascent_px = glyph_px * 1900.0 / 2048.0;
            let descent_px = glyph_px * 500.0 / 2048.0;
            let asked_px = (glyph_px * 4.0 / 3.0).round();

            let in_pixels = line_box(
                &styled(asked_px, None),
                FontExtent::new(ascent_px, descent_px, 0.0),
                asked_px,
                1.0,
            );
            let in_points = line_box(
                &styled(asked_px / density, None),
                FontExtent::new(ascent_px / density, descent_px / density, 0.0),
                asked_px / density,
                density,
            );

            assert!(
                (in_points.height * density - in_pixels.height).abs() < 1e-4,
                "{glyph_px}px: height {} in points against {} in pixels",
                in_points.height * density,
                in_pixels.height
            );
            assert!(
                (in_points.baseline * density - in_pixels.baseline).abs() < 1e-4,
                "{glyph_px}px: baseline {} in points against {} in pixels",
                in_points.baseline * density,
                in_pixels.baseline
            );
        }
    }

    #[test]
    fn title_medium_overflows_its_own_line_height_and_the_font_wins() {
        let box_ = line_box(&styled(36.0, Some(wear())), roboto_16sp(), 36.0, 1.0);
        assert_eq!(box_.height, 38.0);
    }

    #[test]
    fn a_line_height_the_font_fits_inside_is_honoured_as_asked() {
        let extent = FontExtent::new(30.0 * 1900.0 / 2048.0, 30.0 * 500.0 / 2048.0, 0.0);
        let box_ = line_box(&styled(36.0, Some(wear())), extent, 36.0, 1.0);
        assert_eq!(box_.height, 36.0);
        assert_eq!(box_.baseline, 28.0);
    }

    #[test]
    fn the_font_metrics_are_rounded_the_way_the_platform_rounds_them() {
        for (size_px, ascent_px, descent_px) in [
            (24.0_f32, 22.0_f32, 6.0_f32),
            (26.0, 24.0, 6.0),
            (30.0, 28.0, 7.0),
            (32.0, 30.0, 8.0),
            (37.2, 35.0, 9.0),
            (38.72, 36.0, 9.0),
            (43.76, 41.0, 11.0),
            (38.0, 35.0, 9.0),
        ] {
            let extent = FontExtent::new(size_px * 1900.0 / 2048.0, size_px * 500.0 / 2048.0, 0.0);
            let tight = line_box(
                &styled(
                    0.0,
                    Some(LineHeightStyle {
                        mode: LineHeightMode::Tight,
                        ..wear()
                    }),
                ),
                extent,
                0.0,
                1.0,
            );
            assert_eq!(
                (tight.baseline, tight.height - tight.baseline),
                (ascent_px, descent_px),
                "{size_px}px",
            );
        }
    }

    #[test]
    fn the_wear_type_scale_lays_out_in_the_boxes_the_platform_gives_it() {
        for (name, size_px, line_height_px, height, baseline) in [
            ("titleMedium 1.0", 32.0_f32, 36.0_f32, 38.0_f32, 30.0_f32),
            ("labelMedium 1.0", 30.0, 36.0, 36.0, 28.0),
            ("labelSmall 1.0", 26.0, 32.0, 32.0, 25.0),
            ("titleMedium 1.24", 38.72, 41.76, 45.0, 36.0),
            ("labelMedium 1.24", 37.2, 41.76, 44.0, 35.0),
            ("labelSmall 1.24", 32.72, 38.72, 39.0, 30.0),
        ] {
            let extent = FontExtent::new(size_px * 1900.0 / 2048.0, size_px * 500.0 / 2048.0, 0.0);
            let resolved = line_box(
                &styled(line_height_px, Some(wear())),
                extent,
                line_height_px,
                1.0,
            );
            assert_eq!(
                (resolved.height, resolved.baseline),
                (height, baseline),
                "{name}",
            );
        }
    }

    #[test]
    fn the_odd_unit_of_leading_goes_below_the_baseline_not_above() {
        let extent = FontExtent::new(20.0, 10.0, 0.0);
        let box_ = line_box(&styled(33.0, Some(wear())), extent, 33.0, 1.0);
        assert_eq!(box_.height, 33.0);
        assert_eq!(box_.baseline, 21.0);
        assert_ne!(box_.baseline, 20.0 + 1.5);
    }

    #[test]
    fn a_line_height_is_a_whole_number_of_pixels() {
        let extent = FontExtent::new(20.0, 10.0, 0.0);
        let box_ = line_box(&styled(33.4, Some(wear())), extent, 33.4, 1.0);
        assert_eq!(box_.height, 34.0);
    }

    #[test]
    fn top_alignment_puts_the_glyphs_at_the_top_and_bottom_at_the_bottom() {
        let extent = FontExtent::new(20.0, 10.0, 0.0);
        let top = line_box(
            &styled(
                40.0,
                Some(LineHeightStyle {
                    alignment: LineHeightAlignment::Top,
                    ..wear()
                }),
            ),
            extent,
            40.0,
            1.0,
        );
        assert_eq!(top.baseline, 20.0);
        let bottom = line_box(
            &styled(
                40.0,
                Some(LineHeightStyle {
                    alignment: LineHeightAlignment::Bottom,
                    ..wear()
                }),
            ),
            extent,
            40.0,
            1.0,
        );
        assert_eq!(bottom.baseline, 30.0);
        assert_eq!(bottom.height - bottom.baseline, extent.descent);
    }

    #[test]
    fn proportional_alignment_splits_the_leading_the_way_the_font_is_split() {
        let extent = FontExtent::new(20.0, 10.0, 0.0);
        let style = LineHeightStyle {
            alignment: LineHeightAlignment::Proportional,
            ..wear()
        };
        let box_ = line_box(&styled(60.0, Some(style)), extent, 60.0, 1.0);
        assert_eq!(box_.baseline, 40.0);
    }

    #[test]
    fn a_fixed_line_height_lets_the_font_overflow_and_tight_ignores_the_ask() {
        let extent = roboto_16sp();
        let fixed = line_box(
            &styled(
                36.0,
                Some(LineHeightStyle {
                    mode: LineHeightMode::Fixed,
                    ..wear()
                }),
            ),
            extent,
            36.0,
            1.0,
        );
        assert_eq!(
            fixed.height, 36.0,
            "the ask wins even though the font needs 38"
        );
        let tight = line_box(
            &styled(
                80.0,
                Some(LineHeightStyle {
                    mode: LineHeightMode::Tight,
                    ..wear()
                }),
            ),
            extent,
            80.0,
            1.0,
        );
        assert_eq!(tight.height, 38.0);
        assert_eq!(tight.baseline, 30.0);
    }

    #[test]
    fn trimming_removes_the_leading_on_the_edge_it_names() {
        let extent = FontExtent::new(20.0, 10.0, 0.0);
        let both = line_box(
            &styled(
                40.0,
                Some(LineHeightStyle {
                    trim: LineHeightTrim::Both,
                    ..wear()
                }),
            ),
            extent,
            40.0,
            1.0,
        );
        assert_eq!(both.height, 30.0);
        assert_eq!(both.baseline, 20.0);

        let top_only = line_box(
            &styled(
                40.0,
                Some(LineHeightStyle {
                    trim: LineHeightTrim::FirstLineTop,
                    ..wear()
                }),
            ),
            extent,
            40.0,
            1.0,
        );
        assert_eq!(top_only.height, 35.0);
        assert_eq!(top_only.baseline, 20.0);
    }

    #[test]
    fn font_padding_is_only_spent_when_a_style_asks_for_it() {
        let extent = FontExtent::new(20.0, 10.0, 4.0);
        let without = line_box(&styled(30.0, Some(wear())), extent, 30.0, 1.0);
        assert_eq!(without.height, 30.0);
        assert_eq!(without.baseline, 20.0);

        let padded = TextStyle {
            paragraph_style: ParagraphStyle {
                line_height: TextUnit::Sp(30.0),
                line_height_style: Some(wear()),
                platform_style: Some(PlatformParagraphStyle {
                    include_font_padding: Some(true),
                    shaping: None,
                }),
                ..ParagraphStyle::default()
            },
            ..TextStyle::default()
        };
        let with = line_box(&padded, extent, 30.0, 1.0);
        assert_eq!(with.height, 34.0, "the line gap widens the font's demand");
        assert_eq!(with.baseline, 22.0, "and half of it sits above the ascent");
    }

    #[test]
    fn a_nonsense_line_height_falls_back_to_the_font_rather_than_producing_nan() {
        let extent = FontExtent::new(20.0, 10.0, 0.0);
        let box_ = line_box(&styled(30.0, Some(wear())), extent, f32::NAN, 1.0);
        assert_eq!(box_.height, 30.0);
        assert!(box_.baseline.is_finite());
    }
}
