use cranpose_text::{
    FontFamilyResolver, ParagraphStyle, TextLayout, TextLayoutOptions, TextOverflow,
};

use crate::fonts::{ARABIC_VF, ROBOTO, lay_out, typeface};

const TEXT: &str = "The quick brown fox jumps over the lazy dog";

fn layout(
    resolver: &FontFamilyResolver,
    text: &str,
    width: f32,
    options: TextLayoutOptions,
) -> TextLayout {
    lay_out(resolver, text, &ParagraphStyle::default(), width, options)
}

fn options(overflow: TextOverflow, soft_wrap: bool, max_lines: usize) -> TextLayoutOptions {
    TextLayoutOptions {
        overflow,
        soft_wrap,
        max_lines,
        min_lines: 1,
    }
}

// The glyph ids of `line`, left to right.
fn glyphs(laid: &TextLayout, line: usize) -> Vec<u32> {
    laid.glyph_runs(line)
        .flat_map(|run| run.positions().map(|glyph| glyph.id.0).collect::<Vec<_>>())
        .collect()
}

fn ellipsis(path: &str) -> u32 {
    typeface(path)
        .glyph('\u{2026}')
        .expect("the font has an ellipsis")
        .0
}

#[test]
fn the_last_line_the_limit_keeps_ends_in_an_ellipsis_that_fits() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);

    let laid = layout(
        &resolver,
        TEXT,
        120.0,
        options(TextOverflow::Ellipsis, true, 2),
    );

    assert_eq!(laid.line_count(), 2);
    assert!(laid.did_overflow_height());
    assert!(!laid.is_line_ellipsized(0) && laid.is_line_ellipsized(1));
    assert_eq!(glyphs(&laid, 1).last().copied(), Some(ellipsis(ROBOTO)));
    assert!(laid.line_right(1) <= 120.0, "{}", laid.line_right(1));
}

#[test]
fn the_last_line_the_limit_keeps_takes_the_rest_of_its_paragraph() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let text = format!("{TEXT}\n{TEXT}");

    let laid = layout(
        &resolver,
        &text,
        120.0,
        options(TextOverflow::Ellipsis, true, 2),
    );

    assert_eq!(
        laid.line_end(1),
        TEXT.len() + 1,
        "the line runs to its paragraph's end, not the next paragraph"
    );
    assert!(laid.is_line_ellipsized(1));
    assert!(laid.line_right(1) <= 120.0, "{}", laid.line_right(1));
}

#[test]
fn start_and_middle_ellipses_clip_text_of_more_lines() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let wrapped = layout(&resolver, TEXT, 120.0, TextLayoutOptions::default());

    for overflow in [TextOverflow::StartEllipsis, TextOverflow::MiddleEllipsis] {
        let laid = layout(&resolver, TEXT, 120.0, options(overflow, true, 2));

        assert_eq!(laid.line_count(), 2, "{overflow:?}");
        assert!(laid.did_overflow_height(), "{overflow:?}");
        assert!(!laid.is_line_ellipsized(1), "{overflow:?}");
        assert_eq!(laid.line_end(1), wrapped.line_end(1), "{overflow:?}");
    }
}

#[test]
fn clipped_text_keeps_the_lines_that_fit_the_limit_whole() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let wrapped = layout(&resolver, TEXT, 120.0, TextLayoutOptions::default());

    let clipped = layout(&resolver, TEXT, 120.0, options(TextOverflow::Clip, true, 2));

    assert_eq!(clipped.line_count(), 2);
    assert!(clipped.did_overflow_height() && !wrapped.did_overflow_height());
    assert!(!clipped.is_line_ellipsized(1));
    assert_eq!(clipped.line_end(1), wrapped.line_end(1));
}

#[test]
fn unwrapped_text_overflows_its_width_or_ends_in_an_ellipsis() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);

    let clipped = layout(
        &resolver,
        TEXT,
        120.0,
        options(TextOverflow::Clip, false, usize::MAX),
    );
    let ellipsized = layout(
        &resolver,
        TEXT,
        120.0,
        options(TextOverflow::Ellipsis, false, usize::MAX),
    );

    assert_eq!(clipped.line_count(), 1);
    assert!(clipped.did_overflow_width());
    assert_eq!(ellipsized.line_count(), 1);
    assert!(ellipsized.is_line_ellipsized(0));
    assert!(ellipsized.line_right(0) <= 120.0);
}

#[test]
fn a_single_line_shows_a_start_or_middle_ellipsis_where_asked() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let mark = ellipsis(ROBOTO);

    let start = layout(
        &resolver,
        TEXT,
        120.0,
        options(TextOverflow::StartEllipsis, true, 1),
    );
    let middle = layout(
        &resolver,
        TEXT,
        120.0,
        options(TextOverflow::MiddleEllipsis, true, 1),
    );

    assert_eq!(glyphs(&start, 0).first().copied(), Some(mark));
    let middle_glyphs = glyphs(&middle, 0);
    let at = middle_glyphs
        .iter()
        .position(|&id| id == mark)
        .expect("an ellipsis");
    assert!(at > 0 && at + 1 < middle_glyphs.len(), "{middle_glyphs:?}");
    assert!(start.line_right(0) <= 120.0 && middle.line_right(0) <= 120.0);
}

#[test]
fn a_right_to_left_line_ends_in_an_ellipsis_on_its_left() {
    let resolver = FontFamilyResolver::new([typeface(ARABIC_VF)]);
    let text = "مرحبا بالعالم، هذه جملة اختبار طويلة جدا";

    let laid = layout(
        &resolver,
        text,
        120.0,
        options(TextOverflow::Ellipsis, true, 1),
    );

    assert_eq!(glyphs(&laid, 0).first().copied(), Some(ellipsis(ARABIC_VF)));
    assert!((laid.line_right(0) - 120.0).abs() < 0.01);
}

#[test]
fn min_lines_add_room_for_lines_the_text_lacks() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let one = layout(&resolver, "a", f32::INFINITY, TextLayoutOptions::default());

    let three = layout(
        &resolver,
        "a",
        f32::INFINITY,
        TextLayoutOptions {
            min_lines: 3,
            ..TextLayoutOptions::default()
        },
    );

    assert_eq!(three.line_count(), 1);
    assert!((three.height() - 3.0 * one.height()).abs() < 0.01);
}
