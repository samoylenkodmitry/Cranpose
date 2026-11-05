// Glyph advances rounded to whole device pixels, as Android lays out a
// Compose `TextMotion.Static` text: each glyph's own advance rounded half
// up, the kerning shaping adds to it left whole. The expected pixels are
// HarfBuzz's shaping of the bench's Roboto with that rule applied.

use cranpose_text::{
    FontFamily, FontFamilyResolver, GlyphAdvances, LayoutInput, ParagraphLengths, ParagraphStyle,
    ShapingSpan, TextLayout, TextLayoutOptions, TextLayouter, TextOverflow,
};

use crate::fonts::{DEJAVU_MONO, ROBOTO, span, typeface};

// The Mate 20 X's density: 6 px of layout is 18 device pixels.
const GRID: f32 = 3.0;
const SIZE: f32 = 6.0;

// A line of a Gauntlet card body that Compose fits in its 314 px.
const FITS_ON_ANDROID: &str = "eiusmod aliquip cillum nostrud ullamco";

fn input<'a>(
    text: &'a str,
    spans: &'a [ShapingSpan<'a>],
    paragraph: &'a ParagraphStyle,
    advances: GlyphAdvances,
    max_width: f32,
) -> LayoutInput<'a> {
    LayoutInput {
        text,
        spans,
        paragraph,
        lengths: ParagraphLengths {
            grid: GRID,
            advances,
            ..ParagraphLengths::default()
        },
        min_width: 0.0,
        max_width,
        options: TextLayoutOptions::default(),
    }
}

// `text` in the font at `path` at `size`, `max_width` device pixels wide.
fn layout(
    path: &str,
    text: &str,
    size: f32,
    advances: GlyphAdvances,
    max_width: f32,
) -> TextLayout {
    let resolver = FontFamilyResolver::new([typeface(path)]);
    let family = FontFamily::Default;
    let spans = [ShapingSpan {
        size,
        ..span(0..text.len(), &family, 400)
    }];
    let paragraph = ParagraphStyle::default();
    let input = input(text, &spans, &paragraph, advances, max_width / GRID);
    TextLayouter::default().layout(&input, &resolver)
}

// The width of the first line's text, in device pixels.
fn line_pixels(laid: &TextLayout) -> f32 {
    (laid.line_right(0) - laid.line_left(0)) * GRID
}

// A width sits a hundredth of a device pixel at most inside the pixels
// Android lays out, so a text fits a box of the width Android gives it.
fn assert_pixels(actual: f32, expected: f32, case: &str) {
    assert!(
        (actual - expected).abs() < 1e-2,
        "{case}: {actual} device pixels, Android lays out {expected}"
    );
}

#[test]
fn a_line_fits_by_whole_pixel_advances_as_on_android() {
    let rounded = layout(
        ROBOTO,
        FITS_ON_ANDROID,
        SIZE,
        GlyphAdvances::WholePixels,
        314.0,
    );
    assert_eq!(rounded.line_count(), 1, "whole-pixel advances fit 314 px");
    assert_pixels(line_pixels(&rounded), 308.0, "whole-pixel advances");

    let fractional = layout(
        ROBOTO,
        FITS_ON_ANDROID,
        SIZE,
        GlyphAdvances::Fractional,
        314.0,
    );
    assert_eq!(
        fractional.line_count(),
        2,
        "the font's own advances, 314.14 px, wrap"
    );
}

#[test]
fn kerning_stays_fractional_between_whole_pixel_advances() {
    let laid = layout(ROBOTO, "Te AV To.", SIZE, GlyphAdvances::WholePixels, 400.0);
    assert_pixels(line_pixels(&laid), 75.1436, "kerned pairs");
    let laid = layout(
        ROBOTO,
        "Pariatur aute ut veniam cillum irure",
        SIZE,
        GlyphAdvances::WholePixels,
        400.0,
    );
    assert_pixels(line_pixels(&laid), 276.6221, "a card line");
}

#[test]
fn unkerned_glyphs_sit_on_whole_device_pixels() {
    let laid = layout(ROBOTO, "hello", SIZE, GlyphAdvances::WholePixels, 400.0);
    let lefts: Vec<f32> = laid
        .glyph_runs(0)
        .flat_map(|run| {
            run.positions()
                .map(|glyph| glyph.x * GRID)
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(lefts.len(), 5);
    assert!(
        lefts.iter().all(|x| (x - x.round()).abs() < 1e-3),
        "glyphs at {lefts:?} device px"
    );
    let caret = laid.horizontal_position(5, cranpose_text::CaretDirection::Primary) * GRID;
    assert_pixels(caret, 38.0, "the caret after the text");
}

// HarfBuzz sets a combining mark's advance to zero after it reads the
// mark's rounded advance from the font, so the mark adds no pixel. DejaVu
// Sans Mono gives its marks the advance of a whole cell.
#[test]
fn a_zeroed_mark_adds_no_pixels() {
    let rounded = GlyphAdvances::WholePixels;
    let marked = layout(
        DEJAVU_MONO,
        "q\u{301}q\u{301}q\u{301}",
        SIZE,
        rounded,
        400.0,
    );
    let plain = layout(DEJAVU_MONO, "qqq", SIZE, rounded, 400.0);
    assert_pixels(
        line_pixels(&marked),
        line_pixels(&plain),
        "three marked letters",
    );
    assert_pixels(line_pixels(&plain), 33.0, "three letters");
}

// Auto-sized text lays the same glyphs out again at smaller sizes: the
// advances round at the size each layout draws.
#[test]
fn a_relayout_at_another_size_rounds_at_that_size() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let family = FontFamily::Default;
    let paragraph = ParagraphStyle::default();
    let at = |size: f32| {
        [ShapingSpan {
            size,
            ..span(0..FITS_ON_ANDROID.len(), &family, 400)
        }]
    };
    let (large, small) = (at(SIZE), at(5.0));
    let rounded = GlyphAdvances::WholePixels;
    let mut layouter = TextLayouter::default();
    let first = layouter.layout(
        &input(FITS_ON_ANDROID, &large, &paragraph, rounded, 400.0),
        &resolver,
    );
    let again = layouter.relayout(
        &input(FITS_ON_ANDROID, &small, &paragraph, rounded, 400.0),
        &resolver,
        &first,
    );
    let fresh = TextLayouter::default().layout(
        &input(FITS_ON_ANDROID, &small, &paragraph, rounded, 400.0),
        &resolver,
    );
    assert_pixels(
        line_pixels(&again),
        line_pixels(&fresh),
        "relayout at 15 px",
    );
    let glyphs = |laid: &TextLayout| -> Vec<f32> {
        laid.glyph_runs(0)
            .flat_map(|run| run.positions().map(|glyph| glyph.x).collect::<Vec<_>>())
            .collect()
    };
    assert_eq!(glyphs(&again), glyphs(&fresh));
}

// A text sized to its own width, as a chip sizes a label, keeps one line
// with no ellipsis and no overflow: widths of whole pixel advances land on
// the device pixel grid, where a sum a hair off a pixel must not tip the
// line over. The chip adds its padding to the label's width and takes it
// off again for the label, which moves the width by a hair.
#[test]
fn a_text_keeps_one_line_at_its_own_width_at_any_density() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let family = FontFamily::Default;
    let paragraph = ParagraphStyle::default();
    let labels = [
        "-1.24%", "QNWZ", "674.57", "+0.27%", "+17.61%", "AVATAR", "Toyota",
    ];
    let mut wrapped = Vec::new();
    for advances in [GlyphAdvances::WholePixels, GlyphAdvances::Fractional] {
        for grid in [1.75_f32, 2.0, 2.625, 3.0] {
            for size in [4.0_f32, 4.5, 5.0, 5.5, 6.0, 7.0, 10.0, 14.0] {
                for (text, padding) in labels
                    .iter()
                    .flat_map(|text| [0.0_f32, 8.0, 12.5, 24.0].map(|padding| (*text, padding)))
                {
                    let spans = [ShapingSpan {
                        size,
                        ..span(0..text.len(), &family, 400)
                    }];
                    let mut input = input(text, &spans, &paragraph, advances, f32::INFINITY);
                    input.lengths.grid = grid;
                    let mut layouter = TextLayouter::default();
                    let own = layouter.layout(&input, &resolver);
                    input.max_width = (own.width() + padding) - padding;
                    input.options = TextLayoutOptions {
                        overflow: TextOverflow::Ellipsis,
                        max_lines: 1,
                        ..TextLayoutOptions::default()
                    };
                    let again = layouter.layout(&input, &resolver);
                    if own.line_count() != 1
                        || again.line_count() != 1
                        || again.is_line_ellipsized(0)
                        || again.did_overflow_width()
                    {
                        wrapped.push((advances, grid, size, text, padding));
                    }
                }
            }
        }
    }
    assert!(
        wrapped.is_empty(),
        "wrapped, ellipsized or overflowed at their own width: {wrapped:?}"
    );
}

// A wrapped text laid out again at its own width breaks into the same lines,
// so its layout stands for that width, as a width that animates asks of it
// each frame. Widths of whole pixel advances land on the device pixel grid,
// where the own width of a layout is its widest line.
#[test]
fn a_wrapped_text_of_whole_pixels_holds_at_its_own_width() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let family = FontFamily::Default;
    let paragraph = ParagraphStyle::default();
    let text = "Ea elit veniam et laboris enim ut ipsum";
    let mut missed = Vec::new();
    for hundredth in (200..=400).step_by(5) {
        let size = hundredth as f32 / 100.0;
        let spans = [ShapingSpan {
            size,
            ..span(0..text.len(), &family, 400)
        }];
        for third in 45..=90 {
            let width = third as f32 / 3.0;
            let input = input(text, &spans, &paragraph, GlyphAdvances::WholePixels, width);
            let laid = TextLayouter::default().layout(&input, &resolver);
            if laid.line_count() > 1 && !laid.holds(0.0, laid.width()) {
                missed.push((size, width, laid.width()));
            }
        }
    }
    assert!(missed.is_empty(), "no hold at their own width: {missed:?}");
}
