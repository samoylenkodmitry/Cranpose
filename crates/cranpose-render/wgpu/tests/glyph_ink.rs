// A label's glyphs draw their whole ink, as Compose draws it: a slant or a
// stroke past the advances of its line and past its box.

use cranpose_ui::text::{FontStyle, SpanStyle, TextDrawStyle, TextUnit};
use support::page::*;

use crate::support;

const FRAME: u32 = 120;

// `text` drawn black over white at 40 sp in `span`, its box 30 pixels in
// from the frame's corner.
fn label(text: &'static str, span: SpanStyle) {
    FramePage(FRAME, FRAME, Color::WHITE, move || {
        Text(
            text,
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
    label("H", SpanStyle::default());
}

#[composable]
fn LoneF() {
    label("f", SpanStyle::default());
}

#[composable]
fn FBeforePeriod() {
    label("f.", SpanStyle::default());
}

#[composable]
fn Slanted() {
    label(
        "H",
        SpanStyle {
            font_style: Some(FontStyle::Italic),
            ..SpanStyle::default()
        },
    );
}

#[composable]
fn Stroked() {
    label(
        "H",
        SpanStyle {
            draw_style: Some(TextDrawStyle::Stroke { width: 12.0 }),
            ..SpanStyle::default()
        },
    );
}

// The columns and rows of the dark pixels of `page`.
fn dark_pixels(page: fn()) -> Option<Vec<(u32, u32)>> {
    let (frame, _) = support::warm_app_frame(page, FRAME, FRAME)?;
    Some(
        frame
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(_, pixel)| pixel[0] < 128)
            .map(|(index, _)| (index as u32 % FRAME, index as u32 / FRAME))
            .collect(),
    )
}

// The leftmost, topmost, rightmost and bottommost dark pixels of `page`.
fn ink(page: fn()) -> Option<[u32; 4]> {
    let box_of = dark_pixels(page)?
        .into_iter()
        .fold(None, |ink: Option<[u32; 4]>, (x, y)| {
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

// The hook of an `f` reaches past its advance, and draws whole where the
// `f` ends its line as where a period follows it.
#[test]
fn an_f_that_ends_a_line_draws_its_overhang() {
    let (Some(alone), Some(followed)) = (ink(LoneF), dark_pixels(FBeforePeriod)) else {
        return;
    };
    let columns: Vec<u32> = followed.into_iter().map(|(x, _)| x).collect();
    let followed = last_before_last_gap(columns);
    assert_eq!(
        alone[2], followed,
        "the f alone ends at column {}, before a period at {followed}",
        alone[2]
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

// The last of `columns` before the last gap between them: where the glyph
// before a period ends.
fn last_before_last_gap(mut columns: Vec<u32>) -> u32 {
    columns.sort_unstable();
    columns.dedup();
    let gap = columns.windows(2).rposition(|pair| pair[1] > pair[0] + 1);
    columns[gap.unwrap_or(columns.len() - 1)]
}
