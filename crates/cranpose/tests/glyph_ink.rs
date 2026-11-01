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
    let style = TextStyle {
        span_style: SpanStyle {
            color: Some(Color::BLACK),
            font_size: text::TextUnit::Sp(40.0),
            ..span
        },
        paragraph_style: paragraph,
        ..TextStyle::default()
    };
    let mut framebuffer =
        Framebuffer::new(WIDTH, HEIGHT, 1.0, SystemTheme::Light, &[], move || {
            let style = style.clone();
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .background(Color::WHITE)
                    .padding(30.0),
                BoxSpec::default(),
                move || {
                    Text(label, Modifier::empty(), style.clone());
                },
            );
        })
        .expect("valid surface");
    framebuffer.set_active(true);
    framebuffer.tick(16_666_667);
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
        draw_style: Some(text::TextDrawStyle::Stroke { width }),
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

// A slanted, stroked label that changes changes the pixels of its ink, and
// a host that keeps its frame shows what a whole redraw shows.
#[test]
fn a_changing_label_changes_the_pixels_of_its_ink() {
    let held = Rc::new(Cell::new(None));
    let sink = Rc::clone(&held);
    let mut framebuffer =
        Framebuffer::new(WIDTH, HEIGHT, 1.0, SystemTheme::Light, &[], move || {
            let label = remember(|| mutableStateOf("H")).with(|label| *label);
            sink.set(Some(label));
            let style = TextStyle {
                span_style: SpanStyle {
                    color: Some(Color::BLACK),
                    font_size: text::TextUnit::Sp(40.0),
                    font_style: Some(text::FontStyle::Italic),
                    draw_style: Some(text::TextDrawStyle::Stroke { width: 6.0 }),
                    ..SpanStyle::default()
                },
                ..TextStyle::default()
            };
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
    let mut kept = KeptFrame::new(WIDTH, HEIGHT);
    kept.draw_changed_areas(&mut framebuffer, 1);
    let label = held.get().expect("the screen holds its label");
    for (frame, value) in (2..).zip(["W", "j", "Hf", "M", "H"]) {
        label.set(value);
        framebuffer.tick(frame * 16_666_667);
        kept.draw_changed_areas(&mut framebuffer, frame);
        assert!(!kept.areas.is_empty(), "frame {frame}: the label changes");
    }
}

// The last of `columns` before the last gap between them: where the glyph
// before a period ends.
fn last_before_last_gap(mut columns: Vec<u32>) -> u32 {
    columns.sort_unstable();
    columns.dedup();
    let gap = columns.windows(2).rposition(|pair| pair[1] > pair[0] + 1);
    columns[gap.unwrap_or(columns.len() - 1)]
}
