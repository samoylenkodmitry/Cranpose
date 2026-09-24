use cranpose_ui::{
    AppContext, TextLayoutOptions, TextOverflow, TextStyle, prepare_text_layout, set_text_measurer,
    text::AnnotatedString,
};

use crate::text_contract_measurer::{CHAR_WIDTH, ContractMeasurer};

const TWELVE_CHARS_WIDE: f32 = 12.0 * CHAR_WIDTH;
const THREE_LINE_PARAGRAPH: &str = "aaaa bbbb cccc dddd eeee ffff";

fn prepared_lines(
    text: &str,
    overflow: TextOverflow,
    max_lines: usize,
    max_width: f32,
) -> (Vec<String>, bool) {
    let app_context = AppContext::new();
    let prepared = app_context.enter(|| {
        set_text_measurer(ContractMeasurer);
        prepare_text_layout(
            &AnnotatedString::from(text),
            &TextStyle::default(),
            TextLayoutOptions {
                overflow,
                soft_wrap: true,
                max_lines,
                min_lines: 1,
            },
            Some(max_width),
        )
    });
    (
        prepared.text.text.lines().map(str::to_string).collect(),
        prepared.did_overflow,
    )
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
