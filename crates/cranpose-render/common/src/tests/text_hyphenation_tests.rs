use cranpose_ui::text::{LocaleList, SpanStyle, TextStyle};

use super::*;

fn style_with_locale(tags: &str) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            locale_list: Some(LocaleList::from_language_tags(tags)),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn dictionary_breaks_transformation_like_compose_contract() {
    let break_idx = choose_auto_hyphen_break("Transformation", &TextStyle::default(), 8, 12);
    assert_eq!(break_idx, Some(10));
}

#[test]
fn locale_gate_uses_french_dictionary() {
    let break_idx = choose_auto_hyphen_break("éléphant", &style_with_locale("fr-FR"), 0, 7);
    assert_eq!(break_idx, Some(3));
}

#[test]
fn locale_gate_uses_german_dictionary() {
    let break_idx = choose_auto_hyphen_break(
        "Geschwindigkeitsbegrenzung",
        &style_with_locale("de-DE"),
        10,
        20,
    );
    assert!(break_idx.is_some());
}

#[test]
fn unknown_locale_disables_hyphenation() {
    let break_idx = choose_auto_hyphen_break("Transformation", &style_with_locale("ja-JP"), 8, 12);
    assert_eq!(break_idx, None);
}

#[test]
fn dictionary_uses_english_locale_alias() {
    let break_idx = choose_auto_hyphen_break("Transformation", &style_with_locale("en_GB"), 8, 12);
    assert_eq!(break_idx, Some(10));
}

#[test]
fn ignores_breaks_outside_words() {
    let break_idx = choose_auto_hyphen_break("ab cd", &TextStyle::default(), 0, 2);
    assert_eq!(break_idx, None);
}
