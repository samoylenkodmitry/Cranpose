use std::{cell::Cell, rc::Rc};

use cranpose::{
    framebuffer::{Framebuffer, PixelRect},
    prelude::*,
};

use crate::framebuffer::KeptFrame;

const WIDTH: u32 = 160;
const HEIGHT: u32 = 120;

// A label's ink box: the leftmost, topmost, rightmost and bottommost pixels
// it darkens.
#[derive(Clone, Copy, Debug)]
struct Ink {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

// The ink of `label` drawn black over white at 40 sp in `span` and
// `paragraph`, its box 30 pixels in from the screen's corner.
fn ink(label: &'static str, span: SpanStyle, paragraph: ParagraphStyle) -> Ink {
    let dark = dark_pixels(label, span, paragraph);
    dark.into_iter()
        .fold(None, |ink: Option<Ink>, (x, y)| {
            Some(match ink {
                None => Ink {
                    left: x,
                    top: y,
                    right: x,
                    bottom: y,
                },
                Some(ink) => Ink {
                    left: ink.left.min(x),
                    top: ink.top.min(y),
                    right: ink.right.max(x),
                    bottom: ink.bottom.max(y),
                },
            })
        })
        .expect("the label draws")
}

// The pixels `label` drawn as `ink` draws it darkens, as columns and rows.
fn dark_pixels(label: &'static str, span: SpanStyle, paragraph: ParagraphStyle) -> Vec<(u32, u32)> {
    dark_pixels_after(&[], label, (span, paragraph))
}

// `dark_pixels` of `label`, which the screen shows after each of `before`
// in a frame of its own.
fn dark_pixels_after(
    before: &[&'static str],
    label: &'static str,
    (span, paragraph): (SpanStyle, ParagraphStyle),
) -> Vec<(u32, u32)> {
    let style = TextStyle {
        span_style: SpanStyle {
            color: Some(Color::BLACK),
            font_size: text::TextUnit::Sp(40.0),
            ..span
        },
        paragraph_style: paragraph,
        ..TextStyle::default()
    };
    let first = before.first().copied().unwrap_or(label);
    let (mut framebuffer, state) = label_screen(first, style);
    for (frame, next) in (2..).zip(before.iter().skip(1).chain([&label])) {
        state.set(*next);
        framebuffer.tick(frame * 16_666_667);
    }
    framebuffer.changed_areas(&mut Vec::new());
    let mut picture = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    framebuffer.draw_area(
        PixelRect {
            x: 0,
            y: 0,
            width: WIDTH,
            height: HEIGHT,
        },
        &mut picture,
    );
    picture
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(_, pixel)| pixel[0] < 128)
        .map(|(index, _)| (index as u32 % WIDTH, index as u32 / WIDTH))
        .collect()
}

fn italic() -> SpanStyle {
    SpanStyle {
        font_style: Some(text::FontStyle::Italic),
        ..SpanStyle::default()
    }
}

fn stroked(width: f32) -> SpanStyle {
    SpanStyle {
        draw_style: Some(TextDrawStyle::Stroke { width }),
        ..SpanStyle::default()
    }
}

// A synthesized italic slants the top of an H a quarter of its height to
// the right, past the H's advance, and the slanted part draws.
#[test]
fn a_slanted_glyph_draws_past_its_advance() {
    let plain = ink("H", SpanStyle::default(), ParagraphStyle::default());
    let slanted = ink("H", italic(), ParagraphStyle::default());
    let slant = (plain.bottom - plain.top) / 4;
    assert!(
        slanted.right >= plain.right + slant,
        "the slanted H ends at column {}, the upright one at {} and its slant reaches {slant} \
         further",
        slanted.right,
        plain.right
    );
}

// The hook of an `f` reaches past its advance, and draws whole where the
// `f` ends its line as where a period follows it.
#[test]
fn an_f_that_ends_a_line_draws_its_overhang() {
    let alone = ink("f", SpanStyle::default(), ParagraphStyle::default());
    let columns: Vec<u32> = dark_pixels("f.", SpanStyle::default(), ParagraphStyle::default())
        .into_iter()
        .map(|(x, _)| x)
        .collect();
    let followed = last_before_last_gap(columns);
    assert_eq!(
        alone.right, followed,
        "the f alone ends at column {}, before a period at {followed}",
        alone.right
    );
}

// An `f` after a ligature, which the shaper places, ends its line with its
// hook whole too.
#[test]
fn a_shaped_f_that_ends_a_line_draws_its_overhang() {
    let alone = ink("fif", SpanStyle::default(), ParagraphStyle::default());
    let columns: Vec<u32> = dark_pixels("fif.", SpanStyle::default(), ParagraphStyle::default())
        .into_iter()
        .map(|(x, _)| x)
        .collect();
    let followed = last_before_last_gap(columns);
    assert_eq!(
        alone.right, followed,
        "the last f alone ends at column {}, before a period at {followed}",
        alone.right
    );
}

// An `f` that the text met before, in another label, ends its line with
// its hook whole too.
#[test]
fn an_f_met_before_that_ends_a_line_draws_its_overhang() {
    let plain = (SpanStyle::default(), ParagraphStyle::default());
    let alone = ink("Hf", SpanStyle::default(), ParagraphStyle::default());
    let again: Vec<u32> = dark_pixels_after(&["f"], "Hf", plain.clone())
        .into_iter()
        .map(|(x, _)| x)
        .collect();
    let right = again.into_iter().max().expect("the label draws");
    assert_eq!(
        alone.right, right,
        "Hf alone ends at column {}, after an f at {right}",
        alone.right
    );
}

// A stroke's outer half lies outside the outline, past the line's first and
// last advance edge, and draws on both sides.
#[test]
fn a_stroke_draws_its_outer_half_past_the_line_ends() {
    let filled = ink("H", SpanStyle::default(), ParagraphStyle::default());
    let stroked = ink("H", stroked(12.0), ParagraphStyle::default());
    assert!(
        stroked.left + 6 <= filled.left && stroked.right >= filled.right + 6,
        "the stroked H spans columns {}..={}, the filled one {}..={}",
        stroked.left,
        stroked.right,
        filled.left,
        filled.right
    );
}

// Marks stacked over a capital rise past the ascent of the font, above
// the label's box, and draw there.
#[test]
fn stacked_marks_draw_above_the_box() {
    let marked = ink(
        "A\u{302}\u{302}\u{302}",
        SpanStyle::default(),
        ParagraphStyle::default(),
    );
    assert!(
        marked.top < 30,
        "the marks start at row {}, the box at row 30",
        marked.top
    );
}

// A line half as tall as its font, untrimmed, leaves the top of an H above
// the label's box, and the top draws there.
#[test]
fn a_line_shorter_than_its_font_draws_above_the_box() {
    let short = ParagraphStyle {
        line_height: text::TextUnit::Sp(20.0),
        line_height_style: Some(text::LineHeightStyle {
            trim: text::LineHeightTrim::None,
            ..text::LineHeightStyle::default()
        }),
        ..ParagraphStyle::default()
    };
    let tall = ink("H", SpanStyle::default(), ParagraphStyle::default());
    let squeezed = ink("H", SpanStyle::default(), short);
    assert!(
        squeezed.top < 30 && squeezed.bottom - squeezed.top == tall.bottom - tall.top,
        "the H spans rows {}..={} on a short line from row 30, {}..={} on its own",
        squeezed.top,
        squeezed.bottom,
        tall.top,
        tall.bottom
    );
}

// A line a little shorter than its font holds the ink of an E, and leaves
// the accent of an É above the label's box, where it draws.
#[test]
fn an_accent_above_a_short_line_draws_above_the_box() {
    let short = ParagraphStyle {
        line_height: text::TextUnit::Sp(45.0),
        line_height_style: Some(text::LineHeightStyle {
            trim: text::LineHeightTrim::None,
            ..text::LineHeightStyle::default()
        }),
        ..ParagraphStyle::default()
    };
    let plain = ink("E", SpanStyle::default(), short.clone());
    let accented = ink("\u{c9}", SpanStyle::default(), short);
    assert!(
        plain.top >= 30 && accented.top < 30,
        "the E starts at row {}, the accent at row {}, the box at row 30",
        plain.top,
        accented.top
    );
}

// A slanted, stroked label that changes changes the pixels of its ink, and
// a host that keeps its frame shows what a whole redraw shows.
#[test]
fn a_changing_label_changes_the_pixels_of_its_ink() {
    let style = TextStyle {
        span_style: SpanStyle {
            color: Some(Color::BLACK),
            font_size: text::TextUnit::Sp(40.0),
            font_style: Some(text::FontStyle::Italic),
            draw_style: Some(TextDrawStyle::Stroke { width: 6.0 }),
            ..SpanStyle::default()
        },
        ..TextStyle::default()
    };
    let (mut framebuffer, label) = label_screen("H", style);
    let mut kept = KeptFrame::new(WIDTH, HEIGHT);
    kept.draw_changed_areas(&mut framebuffer, 1);
    for (frame, value) in (2..).zip(["W", "j", "Hf", "M", "H"]) {
        label.set(value);
        framebuffer.tick(frame * 16_666_667);
        kept.draw_changed_areas(&mut framebuffer, frame);
        assert!(!kept.areas.is_empty(), "frame {frame}: the label changes");
    }
}

// A white screen that shows `first` in `style`, its box 30 pixels in from
// the corner, after its first frame, and the state that holds the label.
fn label_screen(
    first: &'static str,
    style: TextStyle,
) -> (Framebuffer, MutableState<&'static str>) {
    let shown = Rc::new(Cell::new(None));
    let sink = Rc::clone(&shown);
    let mut framebuffer =
        Framebuffer::new(WIDTH, HEIGHT, 1.0, SystemTheme::Light, &[], move || {
            let style = style.clone();
            let label = remember(|| mutableStateOf(first)).with(|label| *label);
            sink.set(Some(label));
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .background(Color::WHITE)
                    .padding(30.0),
                BoxSpec::default(),
                move || {
                    Text(label.get(), Modifier::empty(), style.clone());
                },
            );
        })
        .expect("valid surface");
    framebuffer.set_active(true);
    framebuffer.tick(16_666_667);
    let label = shown.get().expect("the screen holds its label");
    (framebuffer, label)
}

// The last of `columns` before the last gap between them: where the glyph
// before a period ends.
fn last_before_last_gap(mut columns: Vec<u32>) -> u32 {
    columns.sort_unstable();
    columns.dedup();
    let gap = columns.windows(2).rposition(|pair| pair[1] > pair[0] + 1);
    columns[gap.unwrap_or(columns.len() - 1)]
}
