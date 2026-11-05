// Glyph advances rounded to whole device pixels, as Android lays out a
// Compose `TextMotion.Static` text: each glyph's own advance rounded half
// up, the kerning shaping adds to it left whole. The expected pixels are
// HarfBuzz's shaping of the bench's Roboto with that rule applied.

use cranpose_text::{
    FontFamily, FontFamilyResolver, GlyphAdvances, LayoutInput, ParagraphLengths, ParagraphStyle,
    ShapingSpan, TextLayout, TextLayoutOptions, TextLayouter,
};

use crate::fonts::{ROBOTO, span, typeface};

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

// `text` in Roboto at `size`, `max_width` device pixels wide.
fn layout(text: &str, size: f32, advances: GlyphAdvances, max_width: f32) -> TextLayout {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
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

fn assert_pixels(actual: f32, expected: f32, case: &str) {
    assert!(
        (actual - expected).abs() < 2e-3,
        "{case}: {actual} device pixels, Android lays out {expected}"
    );
}

#[test]
fn a_line_android_fits_by_whole_pixel_advances_fits() {
    let rounded = layout(FITS_ON_ANDROID, SIZE, GlyphAdvances::WholePixels, 314.0);
    assert_eq!(rounded.line_count(), 1, "whole-pixel advances fit 314 px");
    assert_pixels(line_pixels(&rounded), 308.0, "whole-pixel advances");

    let fractional = layout(FITS_ON_ANDROID, SIZE, GlyphAdvances::Fractional, 314.0);
    assert_eq!(
        fractional.line_count(),
        2,
        "the font's own advances, 314.14 px, wrap"
    );
}

#[test]
fn kerning_stays_fractional_between_whole_pixel_advances() {
    let laid = layout("Te AV To.", SIZE, GlyphAdvances::WholePixels, 400.0);
    assert_pixels(line_pixels(&laid), 75.1436, "kerned pairs");
    let laid = layout(
        "Pariatur aute ut veniam cillum irure",
        SIZE,
        GlyphAdvances::WholePixels,
        400.0,
    );
    assert_pixels(line_pixels(&laid), 276.6221, "a card line");
}

#[test]
fn unkerned_glyphs_sit_on_whole_device_pixels() {
    let laid = layout("hello", SIZE, GlyphAdvances::WholePixels, 400.0);
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
