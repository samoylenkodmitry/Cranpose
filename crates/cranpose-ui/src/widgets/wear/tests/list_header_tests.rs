use super::*;

#[test]
fn the_defaults_are_the_ones_the_tokens_declare() {
    let spec = ListHeaderSpec::default();
    assert_eq!(spec.min_height, 48.0, "ListHeaderTokens.Height");
    assert_eq!(spec.padding_start, 14.0);
    assert_eq!(spec.padding_end, 14.0);
    assert_eq!(spec.padding_top, 16.0);
    assert_eq!(spec.padding_bottom, 12.0);
    assert_eq!(
        spec.text_style,
        WearTextStyle::TITLE_MEDIUM.aligned(TextAlign::Center)
    );
}
