use cranpose_ui::{
    AppContext, PreparedTextLayout, SpanStyle, TextLayoutOptions, TextMeasurer, TextOverflow,
    TextStyle, prepare_text_layout, set_text_measurer,
    text::{AnnotatedString, TextUnit},
};

use crate::text_contract_measurer::{BASE_FONT_SIZE_SP, CHAR_WIDTH, ContractMeasurer};

const TWELVE_CHARS_WIDE: f32 = 12.0 * CHAR_WIDTH;
const THREE_LINE_PARAGRAPH: &str = "aaaa bbbb cccc dddd eeee ffff";
const DOUBLE_WIDTH_SP: f32 = 2.0 * BASE_FONT_SIZE_SP;
const HALF_WIDTH_SP: f32 = 0.5 * BASE_FONT_SIZE_SP;

fn prepare(
    text: &AnnotatedString,
    overflow: TextOverflow,
    max_lines: usize,
    max_width: f32,
) -> PreparedTextLayout {
    let app_context = AppContext::new();
    app_context.enter(|| {
        set_text_measurer(ContractMeasurer);
        prepare_text_layout(
            text,
            &TextStyle::default(),
            TextLayoutOptions {
                overflow,
                soft_wrap: true,
                max_lines,
                min_lines: 1,
            },
            Some(max_width),
        )
    })
}

fn prepared_lines(
    text: &str,
    overflow: TextOverflow,
    max_lines: usize,
    max_width: f32,
) -> (Vec<String>, bool) {
    let prepared = prepare(&AnnotatedString::from(text), overflow, max_lines, max_width);
    (
        prepared.text.text.lines().map(str::to_string).collect(),
        prepared.did_overflow,
    )
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

fn sized_span_text_of(prepared: &PreparedTextLayout, font_size_sp: f32) -> Vec<&str> {
    prepared
        .text
        .span_styles
        .iter()
        .filter(|span| span.item.font_size == TextUnit::Sp(font_size_sp))
        .map(|span| &prepared.text.text[span.range.clone()])
        .collect()
}

fn painted_width(prepared: &PreparedTextLayout) -> f32 {
    ContractMeasurer
        .measure(&prepared.text, &TextStyle::default())
        .width
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
fn start_and_middle_ellipsis_leave_a_fitting_single_line_intact() {
    for overflow in [TextOverflow::StartEllipsis, TextOverflow::MiddleEllipsis] {
        let (lines, did_overflow) =
            prepared_lines("aaaa bbbb\ncccc", overflow, 1, TWELVE_CHARS_WIDE);

        assert_eq!(lines, ["aaaa bbbb"], "{overflow:?}");
        assert!(did_overflow, "{overflow:?}");
    }
}

#[test]
fn end_ellipsis_fits_a_larger_span_on_the_last_line_within_the_width() {
    let text = sized_span_text("aa ", DOUBLE_WIDTH_SP, "BBBB", " cc dd");

    let prepared = prepare(&text, TextOverflow::Ellipsis, 1, TWELVE_CHARS_WIDE);

    assert_eq!(prepared.text.text, "aa BBBB\u{2026}");
    assert_eq!(sized_span_text_of(&prepared, DOUBLE_WIDTH_SP), ["BBBB"]);
    assert!(painted_width(&prepared) <= TWELVE_CHARS_WIDE);
    assert!(prepared.did_overflow);
}

#[test]
fn end_ellipsis_keeps_as_much_of_a_smaller_span_as_fits() {
    let text = sized_span_text("aa ", HALF_WIDTH_SP, "bbbbbbbbbbbbbbbb", " cc dd");

    let prepared = prepare(&text, TextOverflow::Ellipsis, 1, TWELVE_CHARS_WIDE);

    assert_eq!(prepared.text.text, "aa bbbbbbbbbbbbbbbb\u{2026}");
    assert_eq!(
        sized_span_text_of(&prepared, HALF_WIDTH_SP),
        ["bbbbbbbbbbbbbbbb"]
    );
    assert_eq!(painted_width(&prepared), TWELVE_CHARS_WIDE);
    assert!(prepared.did_overflow);
}

#[test]
fn start_ellipsis_keeps_the_span_styles_after_the_ellipsis() {
    let text = sized_span_text("aaaa bbbb cccc ", DOUBLE_WIDTH_SP, "DD", "");

    let prepared = prepare(&text, TextOverflow::StartEllipsis, 1, TWELVE_CHARS_WIDE);

    assert_eq!(prepared.text.text, "\u{2026}b cccc DD");
    assert_eq!(sized_span_text_of(&prepared, DOUBLE_WIDTH_SP), ["DD"]);
    assert!(painted_width(&prepared) <= TWELVE_CHARS_WIDE);
    assert!(prepared.did_overflow);
}

#[test]
fn middle_ellipsis_keeps_the_span_styles_on_both_sides() {
    let text = sized_span_text("", HALF_WIDTH_SP, "aaaaaaaaaaaa bbbbbbbbbbbb", "");

    let prepared = prepare(&text, TextOverflow::MiddleEllipsis, 1, TWELVE_CHARS_WIDE);

    assert_eq!(prepared.text.text, "aaaaaaaaaaa\u{2026}bbbbbbbbbbb");
    assert_eq!(
        sized_span_text_of(&prepared, HALF_WIDTH_SP),
        ["aaaaaaaaaaa", "bbbbbbbbbbb"]
    );
    assert_eq!(painted_width(&prepared), TWELVE_CHARS_WIDE);
    assert!(prepared.did_overflow);
}
