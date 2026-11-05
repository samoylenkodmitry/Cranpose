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
//!   split by the style's [`LineHeightAlignment`] into **whole pixels**: the
//!   share below the baseline is `ceil`ed, so the odd pixel of a centred split
//!   sits below it;
//! - a [`LineHeightMode::Tight`] line shorter than its font adds no leading back
//!   at a trimmed edge, and takes the font's whole extent at an untrimmed one;
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

use std::cell::Cell;

use cranpose_ui_graphics::whole;

use crate::style::{
    LineHeightAlignment, LineHeightMode, LineHeightStyle, LineHeightTrim, ParagraphStyle, TextStyle,
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
    /// Distance from the baseline up to the top of the font, as a positive number.
    pub ascent: f32,
    /// Distance from the baseline down to the bottom of the font, as a positive number.
    pub descent: f32,
    /// `hhea.lineGap`. Only read when a style asks for font padding.
    pub line_gap: f32,
}

impl FontExtent {
    /// Creates an extent from `ascent`, `descent` and `line_gap`, all in the same unit.
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
    paragraph_line_box(&style.paragraph_style, extent, asked, grid)
}

/// [`line_box`] for the paragraph settings alone.
pub fn paragraph_line_box(
    style: &ParagraphStyle,
    extent: FontExtent,
    asked: f32,
    grid: f32,
) -> LineBox {
    let inputs = LineBoxInputs::of(style, extent, asked, pixel_grid(grid));
    inputs.pixels().in_units(inputs.grid)
}

// A line box in whole device pixels, as AOSP lays a line out: a paragraph
// stacked from these stays on the pixel grid however many lines it has.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LinePixels {
    pub(crate) height: f32,
    pub(crate) baseline: f32,
    pub(crate) trim_top: f32,
    pub(crate) trim_bottom: f32,
}

impl LinePixels {
    fn in_units(self, grid: f32) -> LineBox {
        LineBox {
            height: self.height / grid,
            baseline: self.baseline / grid,
            trim_top: self.trim_top / grid,
            trim_bottom: self.trim_bottom / grid,
        }
    }
}

// Device pixels per layout pixel, `1.0` for a grid that names none.
fn pixel_grid(grid: f32) -> f32 {
    if grid.is_finite() && grid > 0.0 {
        grid
    } else {
        1.0
    }
}

// What a line box follows from: the line height rules a paragraph's style
// takes, the font's extent, the height asked for, the font padding and the
// pixel grid.
#[derive(Clone, Copy)]
pub(crate) struct LineBoxInputs {
    style: LineHeightStyle,
    extent: FontExtent,
    asked: f32,
    padding: f32,
    grid: f32,
}

impl LineBoxInputs {
    // `grid` is finite and positive.
    pub(crate) fn of(style: &ParagraphStyle, extent: FontExtent, asked: f32, grid: f32) -> Self {
        let asked = if style.line_height.is_unspecified() {
            f32::NAN
        } else {
            asked
        };
        let padding = font_padding(style, extent);
        let line_height_style = match style.line_height_style {
            Some(line_height_style) => line_height_style,
            None if padding > 0.0 || font_padding_asked(style) => LineHeightStyle {
                alignment: LineHeightAlignment::Proportional,
                trim: LineHeightTrim::None,
                mode: LineHeightMode::Fixed,
            },
            None => LineHeightStyle::default(),
        };
        Self {
            style: line_height_style,
            extent,
            asked,
            padding,
            grid,
        }
    }

    // Out of line: the layouter takes most line boxes from `LastLineBox`.
    #[inline(never)]
    fn pixels(self) -> LinePixels {
        aosp_line_pixels(self.style, self.extent, self.asked, self.padding, self.grid)
    }

    // Whether these are `other` bit for bit, a NaN asked height equal to
    // itself.
    fn same(&self, other: &Self) -> bool {
        let bits = |inputs: &Self| {
            [
                inputs.extent.ascent,
                inputs.extent.descent,
                inputs.extent.line_gap,
                inputs.asked,
                inputs.padding,
                inputs.grid,
            ]
            .map(f32::to_bits)
        };
        self.style == other.style && bits(self) == bits(other)
    }
}

// The line box last made, kept for the next line of the same font and
// style: most lines of a paragraph, and the line of a text laid out again
// each frame, as a number that changes, need no rounding of their own.
#[derive(Default)]
pub(crate) struct LastLineBox(Cell<Option<(LineBoxInputs, LinePixels)>>);

impl LastLineBox {
    pub(crate) fn of(&self, inputs: LineBoxInputs) -> LinePixels {
        if let Some((kept, pixels)) = self.0.get()
            && kept.same(&inputs)
        {
            return pixels;
        }
        let pixels = inputs.pixels();
        self.0.set(Some((inputs, pixels)));
        pixels
    }
}

fn font_padding_asked(style: &ParagraphStyle) -> bool {
    style
        .platform_style
        .and_then(|platform| platform.include_font_padding)
        .unwrap_or(false)
}

fn font_padding(style: &ParagraphStyle, extent: FontExtent) -> f32 {
    if font_padding_asked(style) && extent.line_gap.is_finite() && extent.line_gap > 0.0 {
        extent.line_gap
    } else {
        0.0
    }
}

// AOSP's rule, in device pixels: `Paint.getFontMetricsInt()` rounds the
// ascent, a negative number there, and the descent; `LineHeightStyleSpan`
// `ceil`s the line height and the leading's share below the baseline.
fn aosp_line_pixels(
    style: LineHeightStyle,
    extent: FontExtent,
    asked: f32,
    padding: f32,
    grid: f32,
) -> LinePixels {
    let ascent = -whole::floor(0.5 - extent.ascent.max(0.0) * grid);
    let descent = whole::floor(extent.descent.max(0.0) * grid + 0.5);
    let padding = whole::ceil(padding * grid);
    let above_padding = whole::floor(padding * 0.5);
    let font = ascent + descent;
    let natural = font + padding;
    let asked = asked * grid;
    let asked = if asked.is_finite() {
        whole::ceil(asked)
    } else {
        natural
    };

    let height = match style.mode {
        LineHeightMode::Fixed | LineHeightMode::Tight => asked,
        LineHeightMode::Minimum => asked.max(natural),
    }
    .max(1.0);

    let leading = height - natural;
    // AOSP's `topRatio`: the share of the leading above the glyphs.
    let top_ratio = match style.alignment {
        LineHeightAlignment::Top => 0.0,
        LineHeightAlignment::Center => 0.5,
        LineHeightAlignment::Bottom => 1.0,
        LineHeightAlignment::Proportional if font > 0.0 => ascent / font,
        LineHeightAlignment::Proportional => 0.5,
    };
    // A line shorter than its font gives up below the baseline the share the
    // ratio names, not the rest: AOSP's rule, kept as it is.
    let below_share = if leading > 0.0 {
        1.0 - top_ratio
    } else {
        top_ratio
    };
    let below = whole::ceil(leading * below_share);
    let above = leading - below + above_padding;
    let below = below + padding - above_padding;

    let (trim_above, trim_below) = match style.trim {
        LineHeightTrim::None => (false, false),
        LineHeightTrim::FirstLineTop => (true, false),
        LineHeightTrim::LastLineBottom => (false, true),
        LineHeightTrim::Both => (true, true),
    };
    // A tight line shorter than its font keeps that height at a trimmed edge
    // and takes the font's whole extent at an untrimmed one.
    let tight = style.mode == LineHeightMode::Tight && leading < 0.0;
    let given_back = |trimmed: bool, share: f32| if trimmed != tight { share } else { 0.0 };

    LinePixels {
        height,
        baseline: above + ascent,
        trim_top: given_back(trim_above, above),
        trim_bottom: given_back(trim_below, below),
    }
}

#[cfg(test)]
#[path = "tests/line_box_tests.rs"]
mod tests;
