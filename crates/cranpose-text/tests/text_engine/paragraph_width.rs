use cranpose_text::{
    FontFamily, FontFamilyResolver, LayoutInput, ParagraphLengths, ParagraphStyle, TextLayout,
    TextLayoutOptions, TextLayouter, TextOverflow,
};

use crate::fonts::{ROBOTO, span, typeface};

const SHORT: &str = "Short text";
const LONG: &str = "A longer text that wraps onto more than one line at this width";

// Lays `text` out between `min_width` and `max_width` on a grid of `grid`
// device pixels a pixel, with `options`.
fn layout(
    resolver: &FontFamilyResolver,
    text: &str,
    (min_width, max_width): (f32, f32),
    grid: f32,
    options: TextLayoutOptions,
) -> TextLayout {
    let family = FontFamily::Default;
    let spans = [span(0..text.len(), &family, 400)];
    TextLayouter::default().layout(
        &LayoutInput {
            text,
            spans: &spans,
            paragraph: &ParagraphStyle::default(),
            lengths: ParagraphLengths {
                grid,
                ..ParagraphLengths::default()
            },
            min_width,
            max_width,
            options,
        },
        resolver,
    )
}

fn natural(laid: &TextLayout) -> f32 {
    laid.line_right(0) - laid.line_left(0)
}

#[test]
fn text_that_fits_takes_its_own_width_rounded_up_to_whole_device_pixels() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    for grid in [1.0, 2.0, 2.75] {
        let laid = layout(
            &resolver,
            SHORT,
            (0.0, 500.0),
            grid,
            TextLayoutOptions::default(),
        );

        let expected = (natural(&laid) * grid).ceil() / grid;
        assert!(
            (laid.width() - expected).abs() < 1e-4,
            "{grid}: {}",
            laid.width()
        );
        assert!(!laid.did_overflow_width());
    }
}

#[test]
fn wrapped_text_takes_its_widest_line_and_a_min_width_widens_short_text() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);

    let wrapped = layout(
        &resolver,
        LONG,
        (0.0, 150.0),
        1.0,
        TextLayoutOptions::default(),
    );
    let widened = layout(
        &resolver,
        SHORT,
        (300.0, 500.0),
        1.0,
        TextLayoutOptions::default(),
    );
    let fixed = layout(
        &resolver,
        SHORT,
        (200.0, 200.0),
        1.0,
        TextLayoutOptions::default(),
    );

    assert!(wrapped.line_count() > 1);
    let widest = (0..wrapped.line_count())
        .map(|line| wrapped.line_right(line) - wrapped.line_left(line))
        .fold(0.0, f32::max);
    assert!(widest < 150.0);
    assert_eq!(wrapped.width(), widest.ceil());
    assert_eq!(widened.width(), 300.0);
    assert_eq!(fixed.width(), 200.0);
}

#[test]
fn wrapped_text_keeps_its_width_at_the_max_widths_that_keep_its_lines() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let options = TextLayoutOptions::default();
    let mut laid = layout(&resolver, LONG, (0.0, 150.0), 1.0, options);
    let (width, lines) = (laid.width(), laid.line_count());

    let (low, high) = laid.max_widths_kept(0.0);
    assert!(low <= 150.0 && 150.0 <= high, "{low}..{high}");
    for max_width in [low, (low + high) / 2.0, high] {
        assert!(laid.holds(0.0, max_width), "at {max_width}");
        laid.align(0.0, max_width, cranpose_text::TextAlign::Start);
        let fresh = layout(&resolver, LONG, (0.0, max_width), 1.0, options);
        assert_eq!(
            (laid.width(), fresh.width(), fresh.line_count()),
            (width, width, lines),
            "at {max_width}"
        );
    }
    // Past the high end the text breaks into other lines.
    let past = layout(&resolver, LONG, (0.0, high + 1.0), 1.0, options);
    assert!(past.line_count() != lines || past.width() != width);
}

#[test]
fn unwrapped_clipped_text_keeps_its_own_width_past_the_max_and_overflows_it() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let options = TextLayoutOptions {
        soft_wrap: false,
        overflow: TextOverflow::Clip,
        ..TextLayoutOptions::default()
    };

    let laid = layout(&resolver, LONG, (0.0, 150.0), 1.0, options);

    assert_eq!(laid.line_count(), 1);
    assert_eq!(laid.width(), natural(&laid).ceil());
    assert!(laid.did_overflow_width());
}
