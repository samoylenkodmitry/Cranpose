//! Where a line of text sits inside the height it was given, and what a
//! paragraph gives back at its edges.
//!
//! Jetpack Compose lays text out with AOSP's `StaticLayout` and its
//! `LineHeightStyle` span, and so does [`line_box`]:
//!
//! - the font's ascent and descent are **whole pixels**, rounded the way
//!   `Paint.getFontMetricsInt()` rounds them, and the line is built from that
//!   pair rather than from the float metrics;
//! - the line advance is a **whole pixel**, `ceil`ed, not a float;
//! - the leading, the line height past the font's own ascent + descent, is
//!   placed by the style's [`LineHeightAlignment`], with the odd pixel of a
//!   centred split below the baseline;
//! - [`LineHeightTrim`] gives the leading back **only at the paragraph's edges**:
//!   above its first line and below its last. Every line keeps the full advance
//!   between baselines, so a paragraph of `n` lines is
//!   [`LineBox::block_height`] tall and its first baseline sits at
//!   [`LineBox::first_baseline`].
//!
//! A style that names no [`LineHeightStyle`] gets Compose's default, which is
//! [`LineHeightStyle::default`]: proportional leading, both edges trimmed, the
//! requested height fixed. A style that asks for font padding instead gets the
//! rule Compose keeps for padded text: proportional leading, nothing trimmed. A
//! style that asks for no line height gets the font's own ascent + descent, as
//! Compose's does, so a single line of it is exactly as tall as its font.

use crate::text::style::{
    LineHeightAlignment, LineHeightMode, LineHeightStyle, LineHeightTrim, TextStyle,
};

/// A resolved line box: how far apart a paragraph's baselines are, where the
/// baseline sits in each line, and what the paragraph's first and last lines
/// give back at its edges. All measured down from the top of a line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineBox {
    /// Baseline-to-baseline advance: the height of every line before the
    /// paragraph's edges are trimmed.
    pub height: f32,
    /// Distance from the top of a line down to its baseline.
    pub baseline: f32,
    /// What the first line gives back above its glyphs.
    pub trim_top: f32,
    /// What the last line gives back below its glyphs.
    pub trim_bottom: f32,
}

impl LineBox {
    /// A box with nothing trimmed: `height` apart, baseline `baseline` down.
    pub fn untrimmed(height: f32, baseline: f32) -> Self {
        Self {
            height,
            baseline,
            trim_top: 0.0,
            trim_bottom: 0.0,
        }
    }

    /// The height of a paragraph of `lines` lines: every line's advance, less
    /// what the first line's top and the last line's bottom give back.
    pub fn block_height(self, lines: usize) -> f32 {
        (self.height * lines.max(1) as f32 - self.trim_top - self.trim_bottom).max(1.0)
    }

    /// The first line's baseline, measured down from the paragraph's top.
    pub fn first_baseline(self) -> f32 {
        self.baseline - self.trim_top
    }

    /// Where line `index` starts, measured down from the paragraph's top.
    pub fn line_top(self, index: usize) -> f32 {
        index as f32 * self.height - self.trim_top
    }
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
/// `asked` is the line height in the same unit as the extent; a style that
/// asks for no line height is laid out at the font's own extent whatever
/// `asked` says. `grid` is how many device pixels there are to one of those
/// units, and it is what every rounding in the AOSP rule is done against —
/// pass `1.0` when the values are already device pixels, or the density when
/// they are layout points. Getting it wrong does not shift a baseline by a
/// fraction; it quantises the whole line box to the wrong step.
pub fn line_box(style: &TextStyle, extent: FontExtent, asked: f32, grid: f32) -> LineBox {
    let grid = if grid.is_finite() && grid > 0.0 {
        grid
    } else {
        1.0
    };
    let asked = if style.paragraph_style.line_height.is_unspecified() {
        f32::NAN
    } else {
        asked
    };
    let padding = font_padding(style, extent);
    let line_height_style = match style.paragraph_style.line_height_style {
        Some(line_height_style) => line_height_style,
        None if padding > 0.0 || font_padding_asked(style) => LineHeightStyle {
            alignment: LineHeightAlignment::Proportional,
            trim: LineHeightTrim::None,
            mode: LineHeightMode::Fixed,
        },
        None => LineHeightStyle::default(),
    };
    aosp_line_box(line_height_style, extent, asked, padding, grid)
}

fn font_padding_asked(style: &TextStyle) -> bool {
    style
        .paragraph_style
        .platform_style
        .and_then(|platform| platform.include_font_padding)
        .unwrap_or(false)
}

fn font_padding(style: &TextStyle, extent: FontExtent) -> f32 {
    if font_padding_asked(style) && extent.line_gap.is_finite() && extent.line_gap > 0.0 {
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

    LineBox {
        height: height.max(1.0),
        baseline: above + ascent,
        trim_top: if trim_above { above } else { 0.0 },
        trim_bottom: if trim_below { below } else { 0.0 },
    }
}

#[cfg(test)]
#[path = "tests/line_box_tests.rs"]
mod tests;
