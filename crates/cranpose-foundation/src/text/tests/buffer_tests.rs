use super::*;

#[test]
fn new_buffer_has_cursor_at_end() {
    let buffer = TextFieldBuffer::new("Hello");
    assert_eq!(buffer.text(), "Hello");
    assert_eq!(buffer.selection(), TextRange::cursor(5));
}

#[test]
fn insert_at_cursor() {
    let mut buffer = TextFieldBuffer::new("Hello");
    buffer.place_cursor_at_end();
    buffer.insert(", World!");
    assert_eq!(buffer.text(), "Hello, World!");
    assert_eq!(buffer.selection(), TextRange::cursor(13));
}

#[test]
fn insert_in_middle() {
    let mut buffer = TextFieldBuffer::new("Helo");
    buffer.place_cursor_before_char(2);
    buffer.insert("l");
    assert_eq!(buffer.text(), "Hello");
}

#[test]
fn delete_selection() {
    let mut buffer = TextFieldBuffer::new("Hello World");
    buffer.select(TextRange::new(5, 11));
    buffer.delete(buffer.selection());
    assert_eq!(buffer.text(), "Hello");
}

#[test]
fn delete_before_cursor() {
    let mut buffer = TextFieldBuffer::new("Hello");
    buffer.place_cursor_at_end();
    buffer.delete_before_cursor();
    assert_eq!(buffer.text(), "Hell");
}

#[test]
fn select_all() {
    let mut buffer = TextFieldBuffer::new("Hello");
    buffer.select_all();
    assert_eq!(buffer.selection(), TextRange::new(0, 5));
}

#[test]
fn replace_selection() {
    let mut buffer = TextFieldBuffer::new("Hello World");
    buffer.select(TextRange::new(6, 11));
    buffer.insert("Rust");
    assert_eq!(buffer.text(), "Hello Rust");
}

#[test]
fn clear_buffer() {
    let mut buffer = TextFieldBuffer::new("Hello");
    buffer.clear();
    assert!(buffer.is_empty());
    assert_eq!(buffer.selection(), TextRange::zero());
}

#[test]
fn unicode_handling() {
    let mut buffer = TextFieldBuffer::new("Hello 🌍");
    buffer.place_cursor_at_end();
    buffer.delete_before_cursor();
    assert_eq!(buffer.text(), "Hello ");
}

#[test]
fn delete_surrounding_collapsed_cursor() {
    let mut buffer = TextFieldBuffer::new("abcdef");
    buffer.place_cursor_before_char(3);
    buffer.delete_surrounding(2, 2);
    assert_eq!(buffer.text(), "af");
    assert_eq!(buffer.selection(), TextRange::cursor(1));
}

#[test]
fn delete_surrounding_preserves_composition() {
    let mut buffer = TextFieldBuffer::new("abcdef");
    buffer.place_cursor_before_char(3);
    buffer.set_composition(Some(TextRange::new(2, 4)));
    buffer.delete_surrounding(3, 3);
    assert_eq!(buffer.text(), "cd");
    assert_eq!(buffer.selection(), TextRange::cursor(1));
    assert_eq!(buffer.composition(), Some(TextRange::new(0, 2)));
}
