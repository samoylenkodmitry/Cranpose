use super::*;

#[test]
fn auto_hyphenation_without_dictionary_feature_returns_none() {
    let break_idx = choose_auto_hyphen_break("Transformation", &TextStyle::default(), 8, 12);
    assert_eq!(break_idx, None);
}
