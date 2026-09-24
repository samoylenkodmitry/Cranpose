use super::*;

#[test]
fn sibling_index_threshold_parser_accepts_positive_values() {
    assert_eq!(parse_sibling_index_threshold(Some("1")), 1);
    assert_eq!(parse_sibling_index_threshold(Some("4")), 4);
    assert_eq!(parse_sibling_index_threshold(Some("64")), 64);
}

#[test]
fn sibling_index_threshold_parser_rejects_invalid_values() {
    assert_eq!(
        parse_sibling_index_threshold(None),
        DEFAULT_SIBLING_INDEX_THRESHOLD
    );
    assert_eq!(
        parse_sibling_index_threshold(Some("")),
        DEFAULT_SIBLING_INDEX_THRESHOLD
    );
    assert_eq!(
        parse_sibling_index_threshold(Some("0")),
        DEFAULT_SIBLING_INDEX_THRESHOLD
    );
    assert_eq!(
        parse_sibling_index_threshold(Some("abc")),
        DEFAULT_SIBLING_INDEX_THRESHOLD
    );
    assert_eq!(
        parse_sibling_index_threshold(Some("16x")),
        DEFAULT_SIBLING_INDEX_THRESHOLD
    );
}
