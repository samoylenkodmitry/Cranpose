// A label's glyphs draw their whole ink, as Compose draws it: a slant or a
// stroke past the advances of its line and past its box.

use cranpose_ui::text::{FontStyle, SpanStyle, TextDrawStyle, TextUnit};
use support::page::*;

use crate::support;

const FRAME: u32 = 120;

// An H drawn black over white at 40 sp in `span`, its box 30 pixels in from
// the frame's corner.
fn label(span: SpanStyle) {
    FramePage(FRAME, FRAME, Color::WHITE, move || {
        Text(
            "H",
            Modifier::empty().offset(30.0, 30.0),
            TextStyle::from_span_style(SpanStyle {
                color: Some(Color::BLACK),
                font_size: TextUnit::Sp(40.0),
                ..span.clone()
            }),
        );
    });
}

#[composable]
fn Upright() {
    label(SpanStyle::default());
}

#[composable]
fn Slanted() {
    label(SpanStyle {
        font_style: Some(FontStyle::Italic),
        ..SpanStyle::default()
    });
}

#[composable]
fn Stroked() {
    label(SpanStyle {
        draw_style: Some(TextDrawStyle::Stroke { width: 12.0 }),
        ..SpanStyle::default()
    });
}

// The leftmost, topmost, rightmost and bottommost dark pixels of `page`.
fn ink(page: fn()) -> Option<[u32; 4]> {
    let (frame, _) = support::warm_app_frame(page, FRAME, FRAME)?;
    let dark = frame
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(_, pixel)| pixel[0] < 128)
        .map(|(index, _)| (index as u32 % FRAME, index as u32 / FRAME));
    let box_of = dark.fold(None, |ink: Option<[u32; 4]>, (x, y)| {
        Some(ink.map_or([x, y, x, y], |[left, top, right, bottom]| {
            [left.min(x), top.min(y), right.max(x), bottom.max(y)]
        }))
    });
    Some(box_of.expect("the label draws"))
}

// A synthesized italic slants the top of an H a quarter of its height to
// the right, past the H's advance, and the slanted part draws.
#[test]
fn a_slanted_glyph_draws_past_its_advance() {
    let (Some(upright), Some(slanted)) = (ink(Upright), ink(Slanted)) else {
        return;
    };
    let slant = (upright[3] - upright[1]) / 4;
    assert!(
        slanted[2] >= upright[2] + slant,
        "the slanted H ends at column {}, the upright one at {} and its slant reaches {slant} \
         further",
        slanted[2],
        upright[2]
    );
}

// A stroke's outer half lies outside the outline, past the line's ends, and
// draws on both sides.
#[test]
fn a_stroke_draws_its_outer_half_past_the_line_ends() {
    let (Some(filled), Some(stroked)) = (ink(Upright), ink(Stroked)) else {
        return;
    };
    assert!(
        stroked[0] + 6 <= filled[0] && stroked[2] >= filled[2] + 6,
        "the stroked H spans columns {}..={}, the filled one {}..={}",
        stroked[0],
        stroked[2],
        filled[0],
        filled[2]
    );
}
