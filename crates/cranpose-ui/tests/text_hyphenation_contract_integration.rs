use cranpose_ui::{
    AppContext, ParagraphStyle, TextLayoutOptions, TextOverflow, TextStyle, prepare_text_layout,
    text::{AnnotatedString, Hyphens, LocaleList},
};

use crate::text_contract::{CHAR_WIDTH, contract_style, layout_lines};

// The lines of `word` in English with `Hyphens.Auto`, eight characters wide.
fn hyphenated_lines(word: &str) -> Vec<String> {
    let base = contract_style();
    let style = TextStyle {
        span_style: cranpose_ui::SpanStyle {
            locale_list: Some(LocaleList::new(vec!["en-US".to_owned()])),
            ..base.span_style
        },
        paragraph_style: ParagraphStyle {
            hyphens: Hyphens::Auto,
            ..base.paragraph_style
        },
    };
    let options = TextLayoutOptions {
        overflow: TextOverflow::Clip,
        soft_wrap: true,
        max_lines: usize::MAX,
        min_lines: 1,
    };
    AppContext::new().enter(|| {
        layout_lines(&prepare_text_layout(
            &AnnotatedString::from(word),
            &style,
            options,
            Some(8.0 * CHAR_WIDTH),
        ))
    })
}

#[cfg(feature = "text-hyphenation-embedded")]
#[test]
fn a_word_too_long_for_a_line_breaks_where_its_language_hyphenates() {
    assert_eq!(
        hyphenated_lines("Transformation"),
        ["Trans", "forma", "tion"]
    );
}

#[cfg(not(feature = "text-hyphenation"))]
#[test]
fn a_word_too_long_for_a_line_breaks_between_characters_without_dictionaries() {
    assert_eq!(hyphenated_lines("Transformation"), ["Transfor", "mation"]);
}
