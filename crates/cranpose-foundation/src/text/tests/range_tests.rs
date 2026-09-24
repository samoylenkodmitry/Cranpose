use super::*;

#[test]
fn cursor_is_collapsed() {
    let cursor = TextRange::cursor(5);
    assert!(cursor.collapsed());
    assert_eq!(cursor.length(), 0);
    assert_eq!(cursor.start, 5);
    assert_eq!(cursor.end, 5);
}

#[test]
fn selection_is_not_collapsed() {
    let selection = TextRange::new(2, 7);
    assert!(!selection.collapsed());
    assert_eq!(selection.length(), 5);
}

#[test]
fn reverse_selection_length() {
    let reverse = TextRange::new(7, 2);
    assert_eq!(reverse.length(), 5);
    assert_eq!(reverse.min(), 2);
    assert_eq!(reverse.max(), 7);
}

#[test]
fn coerce_in_bounds() {
    let range = TextRange::new(5, 100);
    let coerced = range.coerce_in(10);
    assert_eq!(coerced.start, 5);
    assert_eq!(coerced.end, 10);
}

#[test]
fn contains_index() {
    let range = TextRange::new(2, 5);
    assert!(!range.contains(1));
    assert!(range.contains(2));
    assert!(range.contains(3));
    assert!(range.contains(4));
    assert!(!range.contains(5));
}

#[test]
fn safe_slice_basic() {
    let range = TextRange::new(0, 5);
    assert_eq!(range.safe_slice("Hello World"), "Hello");
}

#[test]
fn safe_slice_beyond_bounds() {
    let range = TextRange::new(0, 100);
    assert_eq!(range.safe_slice("Hello"), "Hello");
}

#[test]
fn safe_slice_unicode() {
    let text = "Hello 🌍";
    let range = TextRange::new(0, 7);
    let slice = range.safe_slice(text);
    assert!(slice == "Hello " || slice == "Hello 🌍");
}
