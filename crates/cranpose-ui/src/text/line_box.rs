//! Where a line of text sits inside the height it was given.
//!
//! [`LineHeightStyle`] has been a declared-but-unread field on
//! [`ParagraphStyle`](crate::text::ParagraphStyle) since it was added: nothing
//! outside `merge` and the hash keys ever looked at it, and the rasterizer's
//! line box was a fixed rule — the box is exactly the requested line height,
//! and the leading is split evenly above and below. That rule is not what
//! Android does, and the difference is visible.
//!
//! AOSP's `StaticLayout` differs in three ways that each move a glyph row:
//!
//! - the line advance is a **whole pixel**, `ceil`ed, not a float;
//! - a requested line height **shorter than the font's own ascent + descent
//!   does not shrink the line** — the font wins, which is why a 16sp/18sp
//!   style lays out in 38px rather than 36px at density 2;
//! - the leading is split with the **odd pixel below** the baseline, not above.
//!
//! [`line_box`] implements that, and it implements it **only when the caller
//! asked for it**. A style whose `line_height_style` is `None` — which is every
//! style in the framework today — gets exactly the arithmetic it got before,
//! bit for bit. That is deliberate: the rule changes where every glyph in the
//! framework lands, and it is not a change to make silently on behalf of text
//! that never asked.

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
    match style.paragraph_style.line_height_style {
        None => unstyled_line_box(extent, asked),
        Some(line_height_style) => {
            let padding = font_padding(style, extent);
            let grid = if grid.is_finite() && grid > 0.0 {
                grid
            } else {
                1.0
            };
            aosp_line_box(line_height_style, extent, asked, padding, grid)
        }
    }
}

/// The rule for a style that names no line-height policy: the box is the
/// requested height and the leading is split evenly.
///
/// This is what the rasterizer has always done, kept bit for bit, because every
/// style in the framework still lands here and none of them asked to move.
fn unstyled_line_box(extent: FontExtent, asked: f32) -> LineBox {
    let natural = extent.natural().ceil();
    LineBox {
        height: asked,
        baseline: extent.ascent + (asked - natural) * 0.5,
    }
}

/// `includeFontPadding`'s share of the leading.
///
/// Android's font padding is the gap between the `hhea` metrics and the tighter
/// typographic ones. `ab_glyph` reports one pair of metrics and the line gap
/// separately, so the closest honest reading is the line gap: `Some(true)`
/// spends it, `Some(false)` and `None` do not. Wear's `DefaultTextStyle` sets
/// it to `false`, which is the case this module exists to serve.
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
    // Android hands its layout `Paint.FontMetricsInt`, whose ascent and descent
    // are whole pixels — both rounded AWAY from the baseline, so neither can
    // clip a glyph. Doing the leading split on unrounded metrics leaves a
    // fractional remainder that the `ceil` below then spends in the wrong
    // direction, and the baseline comes out under the ascent.
    //
    // `round` per metric, which is what the app this was measured against
    // uses, agrees with `ceil` on all four text styles these screens draw.
    // They diverge only where a metric's fraction is under a half, and none of
    // Roboto's are at these sizes.
    let ascent = up(extent.ascent.max(0.0));
    let descent = up(extent.descent.max(0.0));
    // Font padding widens the font's own demand, so it survives `Tight` and it
    // is what a shorter requested line height has to beat.
    let above_padding = down(padding * 0.5);
    let below_padding = padding - above_padding;
    let natural = up(ascent + descent + padding);
    let asked = if asked.is_finite() {
        up(asked)
    } else {
        natural
    };

    let height = match style.mode {
        // The requested height, whatever the font wants.
        LineHeightMode::Fixed => asked.max(1.0),
        // The font's demand is a floor. This is what Android does and what the
        // Wear text styles measure as.
        LineHeightMode::Minimum => asked.max(natural).max(1.0),
        // The font's demand, whatever was requested.
        LineHeightMode::Tight => natural.max(1.0),
    };

    // Leading is whatever the box has over the font's own extent. It can be
    // negative under `Fixed`, and then the glyphs simply overflow their box —
    // which is also what Android does.
    let leading = height - (ascent + descent + padding);
    let (mut above, mut below) = match style.alignment {
        // All of it below: the text sits at the top of its box.
        LineHeightAlignment::Top => (0.0, leading),
        // All of it above.
        LineHeightAlignment::Bottom => (leading, 0.0),
        // Split evenly, with the odd whole unit going BELOW the baseline.
        // Splitting the other way is a one-pixel error on every line whose
        // leading is odd, which at density 2 is every other line height.
        LineHeightAlignment::Center => {
            let below = up(leading * 0.5);
            (leading - below, below)
        }
        // Split in the font's own ascent-to-descent ratio.
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

    // Trimming removes the leading the alignment just handed out, on whichever
    // edge of the block it lands. A Wear row is one line, so both edges belong
    // to the same box; a multi-line paragraph needs per-line boxes, which
    // `TextMetrics` does not carry (its height is a flat `lines * advance`),
    // so trimming there is not yet expressible and this stays a single-line
    // rule.
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
    use crate::text::style::{ParagraphStyle, PlatformParagraphStyle};
    use crate::text::TextUnit;

    /// Roboto at 16sp on a density-2 watch: 32px of glyph, `hhea` ascender
    /// 1900/2048 and descender 500/2048, so 29.69px above the baseline and
    /// 7.81px below — 37.5px of natural extent against a 36px line height.
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
    fn title_medium_overflows_its_own_line_height_and_the_font_wins() {
        // The measured case: 16sp/18sp titleMedium lays out in 38px, not 36px.
        let box_ = line_box(&styled(36.0, Some(wear())), roboto_16sp(), 36.0, 1.0);
        assert_eq!(box_.height, 38.0);
    }

    #[test]
    fn a_line_height_the_font_fits_inside_is_honoured_as_asked() {
        // 15sp labelMedium: 30px glyph, 35.16px natural, 36px asked. The ask
        // wins, and this is the case that is NOT visible on these screens.
        let extent = FontExtent::new(30.0 * 1900.0 / 2048.0, 30.0 * 500.0 / 2048.0, 0.0);
        let box_ = line_box(&styled(36.0, Some(wear())), extent, 36.0, 1.0);
        assert_eq!(box_.height, 36.0);
        // Whole-pixel metrics are 28 above and 8 below, so the ask is spent
        // exactly and the baseline sits on the ascent.
        assert_eq!(box_.baseline, 28.0);
    }

    #[test]
    fn the_odd_unit_of_leading_goes_below_the_baseline_not_above() {
        // 3 units of leading over a whole-numbered font: 1 above, 2 below.
        let extent = FontExtent::new(20.0, 10.0, 0.0);
        let box_ = line_box(&styled(33.0, Some(wear())), extent, 33.0, 1.0);
        assert_eq!(box_.height, 33.0);
        assert_eq!(box_.baseline, 21.0);
        // Splitting the other way would put the baseline at 21.5 and every
        // glyph row half a pixel out.
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
        // 30 of leading split 2:1, so 20 above.
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
        // Whole-pixel metrics: 30 above the baseline and 8 below.
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
        // 10 of leading, 5 above and 5 below; only the top is removed.
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
