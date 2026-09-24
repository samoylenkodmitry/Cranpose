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
//! [`DrawTextStyle::with_line_height_style`](cranpose_ui_graphics::DrawTextStyle::with_line_height_style)
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
#[path = "tests/line_box_tests.rs"]
mod tests;
