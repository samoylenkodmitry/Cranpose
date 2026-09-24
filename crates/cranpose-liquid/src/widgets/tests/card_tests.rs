use super::*;

#[test]
fn a_row_draws_no_separator_until_one_is_asked_for() {
    assert!(
        !LiquidListRowSpec::default().separator,
        "the last row of a section must not draw a trailing hairline"
    );
    assert!(LiquidListRowSpec::default().with_separator(true).separator);
    assert!(
        !LiquidListRowSpec::default()
            .with_separator(true)
            .with_separator(false)
            .separator,
        "the latest answer is the one that holds"
    );
}
