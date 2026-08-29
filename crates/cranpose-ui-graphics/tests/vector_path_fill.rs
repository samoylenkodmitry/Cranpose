use cranpose_ui_graphics::{PathFillRule, VectorPath};

const NESTED_SQUARES: &str = "M0 0 L100 0 L100 100 L0 100 Z M25 25 L75 25 L75 75 L25 75 Z";

#[test]
fn a_parsed_path_reports_the_rule_it_was_parsed_with() {
    let path = VectorPath::parse(NESTED_SQUARES).expect("the path parses");
    assert_eq!(
        path.fill_rule(),
        PathFillRule::NonZero,
        "SVG's default fill rule is non-zero, and a parse must not invent another"
    );
}

#[test]
fn parsing_with_a_fill_rule_is_what_the_path_then_reports() {
    let path = VectorPath::parse_with_fill_rule(NESTED_SQUARES, PathFillRule::EvenOdd)
        .expect("the path parses");
    assert_eq!(path.fill_rule(), PathFillRule::EvenOdd);
}

#[test]
fn scaling_a_path_keeps_the_fill_rule_it_was_given() {
    let path = VectorPath::parse_with_fill_rule(NESTED_SQUARES, PathFillRule::EvenOdd)
        .expect("the path parses");
    assert_eq!(
        path.scaled(2.0).fill_rule(),
        PathFillRule::EvenOdd,
        "scaling reset the fill rule"
    );
}
