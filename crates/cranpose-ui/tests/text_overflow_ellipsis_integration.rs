use std::rc::Rc;

use cranpose_ui::{
    AppContext, PreparedTextLayout, SpanStyle, TextLayoutOptions, TextOverflow,
    prepare_text_layout,
    text::{AnnotatedString, TextUnit},
};

use crate::text_contract::{CHAR_WIDTH, FONT_SIZE_SP, contract_style, drawn_at_size, drawn_lines};

const TWELVE_CHARS_WIDE: f32 = 12.0 * CHAR_WIDTH;
const THREE_LINE_PARAGRAPH: &str = "aaaa bbbb cccc dddd eeee ffff";
const THREE_SHORT_PARAGRAPHS: &str = "aaa\nbbb\nccc";
const ELLIPSES: [TextOverflow; 3] = [
    TextOverflow::Ellipsis,
    TextOverflow::StartEllipsis,
    TextOverflow::MiddleEllipsis,
];
const DOUBLE_WIDTH_SP: f32 = 2.0 * FONT_SIZE_SP;
const HALF_WIDTH_SP: f32 = 0.5 * FONT_SIZE_SP;

fn prepare(
    text: impl Into<AnnotatedString>,
    options: TextLayoutOptions,
    max_width: f32,
) -> Rc<PreparedTextLayout> {
    let text = text.into();
    AppContext::new()
        .enter(|| prepare_text_layout(&text, &contract_style(), options, Some(max_width)))
}

fn wrapped(overflow: TextOverflow, max_lines: usize) -> TextLayoutOptions {
    TextLayoutOptions {
        overflow,
        soft_wrap: true,
        max_lines,
        min_lines: 1,
    }
}

fn unwrapped(overflow: TextOverflow) -> TextLayoutOptions {
    TextLayoutOptions {
        overflow,
        soft_wrap: false,
        ..TextLayoutOptions::default()
    }
}

fn prepared_lines(
    text: &str,
    overflow: TextOverflow,
    max_lines: usize,
    max_width: f32,
) -> (Vec<String>, bool) {
    let prepared = prepare(text, wrapped(overflow, max_lines), max_width);
    (drawn_lines(&prepared), prepared.did_overflow())
}

fn sized_span_text(before: &str, font_size_sp: f32, sized: &str, after: &str) -> AnnotatedString {
    AnnotatedString::builder()
        .append(before)
        .push_style(SpanStyle {
            font_size: TextUnit::Sp(font_size_sp),
            ..Default::default()
        })
        .append(sized)
        .pop()
        .append(after)
        .to_annotated_string()
}

// The right edge of the widest line of `prepared`.
fn painted_width(prepared: &PreparedTextLayout) -> f32 {
    (0..prepared.layout.line_count())
        .map(|line| prepared.layout.line_right(line))
        .fold(0.0, f32::max)
}

#[test]
fn clip_wraps_the_contract_paragraph_into_three_lines() {
    let (lines, did_overflow) = prepared_lines(
        THREE_LINE_PARAGRAPH,
        TextOverflow::Clip,
        usize::MAX,
        TWELVE_CHARS_WIDE,
    );

    assert_eq!(lines, ["aaaa bbbb", "cccc dddd", "eeee ffff"]);
    assert!(!did_overflow);
}

#[test]
fn end_ellipsis_fills_the_last_line_from_the_lines_max_lines_dropped() {
    let (lines, did_overflow) = prepared_lines(
        THREE_LINE_PARAGRAPH,
        TextOverflow::Ellipsis,
        2,
        TWELVE_CHARS_WIDE,
    );

    assert_eq!(lines, ["aaaa bbbb", "cccc dddd e\u{2026}"]);
    assert!(did_overflow);
}

#[test]
fn end_ellipsis_marks_paragraphs_max_lines_dropped() {
    let (lines, did_overflow) = prepared_lines(
        "aaaa bbbb\ncccc\ndddd",
        TextOverflow::Ellipsis,
        2,
        TWELVE_CHARS_WIDE,
    );

    assert_eq!(lines, ["aaaa bbbb", "cccc\u{2026}"]);
    assert!(did_overflow);
}

#[test]
fn end_ellipsis_trims_a_full_last_line_until_the_ellipsis_fits() {
    let (lines, did_overflow) = prepared_lines(
        "aaaa bbbb cc\ndddd",
        TextOverflow::Ellipsis,
        1,
        TWELVE_CHARS_WIDE,
    );

    assert_eq!(lines, ["aaaa bbbb c\u{2026}"]);
    assert!(did_overflow);
}

#[test]
fn start_ellipsis_on_one_line_keeps_the_end_of_the_wrapped_paragraph() {
    let (lines, did_overflow) = prepared_lines(
        THREE_LINE_PARAGRAPH,
        TextOverflow::StartEllipsis,
        1,
        TWELVE_CHARS_WIDE,
    );

    assert_eq!(lines, ["\u{2026}d eeee ffff"]);
    assert!(did_overflow);
}

#[test]
fn middle_ellipsis_on_one_line_keeps_both_ends_of_the_wrapped_paragraph() {
    let (lines, did_overflow) = prepared_lines(
        THREE_LINE_PARAGRAPH,
        TextOverflow::MiddleEllipsis,
        1,
        TWELVE_CHARS_WIDE,
    );

    assert_eq!(lines, ["aaaa b\u{2026} ffff"]);
    assert!(did_overflow);
}

#[test]
fn start_and_middle_ellipsis_clip_text_limited_to_several_lines() {
    for overflow in [TextOverflow::StartEllipsis, TextOverflow::MiddleEllipsis] {
        let (lines, did_overflow) =
            prepared_lines(THREE_LINE_PARAGRAPH, overflow, 2, TWELVE_CHARS_WIDE);

        assert_eq!(lines, ["aaaa bbbb", "cccc dddd"], "{overflow:?}");
        assert!(did_overflow, "{overflow:?}");
    }
}

#[test]
fn start_and_middle_ellipsis_mark_the_paragraphs_a_single_line_drops() {
    // The text is as wide as its widest paragraph, "aaaa bbbb", so the
    // ellipsis takes a character's room, as Android cuts a line with more
    // text after it.
    for (overflow, expected) in [
        (TextOverflow::StartEllipsis, "\u{2026}aaa bbbb"),
        (TextOverflow::MiddleEllipsis, "aaaa\u{2026}bbbb"),
    ] {
        let (lines, did_overflow) =
            prepared_lines("aaaa bbbb\ncccc", overflow, 1, TWELVE_CHARS_WIDE);

        assert_eq!(lines, [expected], "{overflow:?}");
        assert!(did_overflow, "{overflow:?}");
    }
}

#[test]
fn end_ellipsis_fits_a_larger_span_on_the_last_line_within_the_width() {
    let text = sized_span_text("aa ", DOUBLE_WIDTH_SP, "BBBB", " cc dd");

    let prepared = prepare(text, wrapped(TextOverflow::Ellipsis, 1), TWELVE_CHARS_WIDE);

    assert_eq!(drawn_lines(&prepared), ["aa BBBB\u{2026}"]);
    assert_eq!(drawn_at_size(&prepared, DOUBLE_WIDTH_SP), ["BBBB"]);
    assert!(painted_width(&prepared) <= TWELVE_CHARS_WIDE);
    assert!(prepared.did_overflow());
}

#[test]
fn end_ellipsis_keeps_as_much_of_a_smaller_span_as_fits() {
    let text = sized_span_text("aa ", HALF_WIDTH_SP, "bbbbbbbbbbbbbbbb", " cc dd");

    let prepared = prepare(text, wrapped(TextOverflow::Ellipsis, 1), TWELVE_CHARS_WIDE);

    assert_eq!(drawn_lines(&prepared), ["aa bbbbbbbbbbbbbbbb\u{2026}"]);
    assert_eq!(
        drawn_at_size(&prepared, HALF_WIDTH_SP),
        ["bbbbbbbbbbbbbbbb"]
    );
    assert_eq!(painted_width(&prepared), TWELVE_CHARS_WIDE);
    assert!(prepared.did_overflow());
}

#[test]
fn start_ellipsis_keeps_the_span_styles_after_the_ellipsis() {
    let text = sized_span_text("aaaa bbbb cccc ", DOUBLE_WIDTH_SP, "DD", "");

    let prepared = prepare(
        text,
        wrapped(TextOverflow::StartEllipsis, 1),
        TWELVE_CHARS_WIDE,
    );

    assert_eq!(drawn_lines(&prepared), ["\u{2026}b cccc DD"]);
    assert_eq!(drawn_at_size(&prepared, DOUBLE_WIDTH_SP), ["DD"]);
    assert!(painted_width(&prepared) <= TWELVE_CHARS_WIDE);
    assert!(prepared.did_overflow());
}

#[test]
fn middle_ellipsis_keeps_the_span_styles_on_both_sides() {
    let text = sized_span_text("", HALF_WIDTH_SP, "aaaaaaaaaaaa bbbbbbbbbbbb", "");

    let prepared = prepare(
        text,
        wrapped(TextOverflow::MiddleEllipsis, 1),
        TWELVE_CHARS_WIDE,
    );

    // The ellipsis takes the half width of the text its line starts with.
    assert_eq!(drawn_lines(&prepared), ["aaaaaaaaaaaa\u{2026}bbbbbbbbbbb"]);
    assert_eq!(
        drawn_at_size(&prepared, HALF_WIDTH_SP),
        ["aaaaaaaaaaaa", "bbbbbbbbbbb"]
    );
    assert_eq!(painted_width(&prepared), TWELVE_CHARS_WIDE);
    assert!(prepared.did_overflow());
}

#[test]
fn ellipsis_without_soft_wrap_lays_out_only_the_first_line() {
    // The text is as wide as its widest paragraph, so the ellipsis that marks
    // the dropped lines takes a character's room.
    for (overflow, expected) in ELLIPSES
        .into_iter()
        .zip(["aa\u{2026}", "\u{2026}aa", "a\u{2026}a"])
    {
        let prepared = prepare(
            THREE_SHORT_PARAGRAPHS,
            unwrapped(overflow),
            TWELVE_CHARS_WIDE,
        );

        assert_eq!(drawn_lines(&prepared), [expected], "{overflow:?}");
        assert_eq!(prepared.metrics().line_count, 1, "{overflow:?}");
        assert!(prepared.did_overflow(), "{overflow:?}");
    }
}

#[test]
fn start_and_middle_ellipsis_without_soft_wrap_elide_the_first_line() {
    let text = "aaaa bbbb cccc dddd\neeee";
    for (overflow, expected) in [
        (TextOverflow::StartEllipsis, "\u{2026}b cccc dddd"),
        (TextOverflow::MiddleEllipsis, "aaaa b\u{2026} dddd"),
    ] {
        let prepared = prepare(text, unwrapped(overflow), TWELVE_CHARS_WIDE);

        assert_eq!(drawn_lines(&prepared), [expected], "{overflow:?}");
        assert!(prepared.did_overflow(), "{overflow:?}");
    }
}

#[test]
fn ellipsis_without_soft_wrap_keeps_the_min_lines_height() {
    let prepared = prepare(
        THREE_SHORT_PARAGRAPHS,
        TextLayoutOptions {
            min_lines: 3,
            ..unwrapped(TextOverflow::Ellipsis)
        },
        TWELVE_CHARS_WIDE,
    );

    assert_eq!(drawn_lines(&prepared), ["aa\u{2026}"]);
    assert_eq!(prepared.metrics().line_count, 3);
}

#[test]
fn clip_without_soft_wrap_keeps_every_line() {
    let prepared = prepare(
        THREE_SHORT_PARAGRAPHS,
        unwrapped(TextOverflow::Clip),
        TWELVE_CHARS_WIDE,
    );

    assert_eq!(drawn_lines(&prepared), ["aaa", "bbb", "ccc"]);
    assert!(!prepared.did_overflow());
}
