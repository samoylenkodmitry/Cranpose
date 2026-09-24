use super::*;

#[test]
fn single_line_properties() {
    let limits = TextFieldLineLimits::SingleLine;
    assert!(limits.is_single_line());
    assert!(!limits.is_multi_line());
    assert_eq!(limits.min_lines(), 1);
    assert_eq!(limits.max_lines(), 1);
}

#[test]
fn multi_line_default_properties() {
    let limits = TextFieldLineLimits::default();
    assert!(!limits.is_single_line());
    assert!(limits.is_multi_line());
    assert_eq!(limits.min_lines(), 1);
    assert_eq!(limits.max_lines(), usize::MAX);
}

#[test]
fn multi_line_constrained_properties() {
    let limits = TextFieldLineLimits::MultiLine {
        min_lines: 3,
        max_lines: 10,
    };
    assert!(!limits.is_single_line());
    assert!(limits.is_multi_line());
    assert_eq!(limits.min_lines(), 3);
    assert_eq!(limits.max_lines(), 10);
}

#[test]
fn filter_replaces_newlines() {
    assert_eq!(filter_for_single_line("hello\nworld"), "hello world");
    assert_eq!(filter_for_single_line("a\n\nb"), "a  b");
    assert_eq!(filter_for_single_line("no newlines"), "no newlines");
    assert_eq!(filter_for_single_line("\n\n\n"), "   ");
}
