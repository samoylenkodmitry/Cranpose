use super::*;

#[test]
fn test_monospaced_layout() {
    let layout = TextLayoutResult::monospaced("Hello", 10.0, 20.0);

    assert_eq!(layout.get_cursor_x(0), 0.0);
    assert_eq!(layout.get_cursor_x(5), 50.0);
}

#[test]
fn test_get_offset_for_x() {
    let layout = TextLayoutResult::monospaced("Hello", 10.0, 20.0);

    let offset = layout.get_offset_for_x(25.0);
    assert!(offset == 2 || offset == 3);
}

#[test]
fn test_multiline() {
    let layout = TextLayoutResult::monospaced("Hi\nWorld", 10.0, 20.0);

    assert_eq!(layout.lines.len(), 2);
    assert_eq!(layout.lines[0].start_offset, 0);
    assert_eq!(layout.lines[1].start_offset, 3);
}

#[test]
fn test_validity() {
    let layout = TextLayoutResult::monospaced("Hello", 10.0, 20.0);

    assert!(layout.is_valid_for("Hello"));
    assert!(!layout.is_valid_for("World"));
}
