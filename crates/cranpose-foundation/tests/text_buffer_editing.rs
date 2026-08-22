//! The editing buffer behind every text field.
//!
//! Everything a keypress does to a field ends here: what the cursor deletes,
//! how a selection grows, and whether the buffer admits it has been touched.
//! These are the operations a user notices immediately when they are one byte
//! out, so each one is stated exactly rather than exercised through a widget.

use cranpose_foundation::text::{TextFieldBuffer, TextRange};

#[test]
fn a_fresh_buffer_has_no_selection_and_no_changes() {
    let buffer = TextFieldBuffer::new("hello");
    assert!(!buffer.has_selection(), "a new buffer selected something");
    assert!(
        !buffer.has_changes(),
        "a buffer nobody edited claimed to be dirty"
    );
    assert_eq!(buffer.selection(), TextRange::cursor(5));
}

#[test]
fn a_buffer_reports_changes_only_after_it_is_edited() {
    let mut buffer = TextFieldBuffer::new("hello");
    buffer.select_all();
    assert!(
        !buffer.has_changes(),
        "selecting is not editing and must not mark the buffer dirty"
    );

    buffer.insert("!");
    assert!(buffer.has_changes(), "an insert left the buffer clean");
}

#[test]
fn deleting_after_the_cursor_removes_the_following_character() {
    let mut buffer = TextFieldBuffer::with_selection("hello", TextRange::cursor(0));
    buffer.delete_after_cursor();
    assert_eq!(buffer.text(), "ello");
    assert_eq!(
        buffer.selection(),
        TextRange::cursor(0),
        "the cursor moved while deleting forward"
    );
}

#[test]
fn deleting_after_the_cursor_at_the_end_does_nothing() {
    let mut buffer = TextFieldBuffer::new("hello");
    buffer.delete_after_cursor();
    assert_eq!(buffer.text(), "hello");
    assert!(
        !buffer.has_changes(),
        "a delete that removed nothing marked the buffer dirty"
    );
}

#[test]
fn deleting_after_the_cursor_removes_a_whole_character_not_a_byte() {
    // "é" is two bytes in UTF-8. Removing one of them would leave the buffer
    // holding text that is not valid UTF-8 at all.
    let mut buffer = TextFieldBuffer::with_selection("éa", TextRange::cursor(0));
    buffer.delete_after_cursor();
    assert_eq!(buffer.text(), "a");
}

#[test]
fn deleting_after_the_cursor_removes_the_selection_when_there_is_one() {
    let mut buffer = TextFieldBuffer::with_selection("hello", TextRange::new(1, 4));
    buffer.delete_after_cursor();
    assert_eq!(buffer.text(), "ho");
    assert!(!buffer.has_selection());
}

#[test]
fn extending_the_selection_left_grows_it_one_character_at_a_time() {
    let mut buffer = TextFieldBuffer::new("hello");
    buffer.extend_selection_left();
    assert_eq!(buffer.selection(), TextRange::new(5, 4));
    buffer.extend_selection_left();
    assert_eq!(buffer.selection(), TextRange::new(5, 3));
    assert_eq!(
        buffer.copy_selection().as_deref(),
        Some("lo"),
        "the extended selection did not cover the characters it grew over"
    );
}

#[test]
fn extending_the_selection_left_stops_at_the_start() {
    let mut buffer = TextFieldBuffer::with_selection("ab", TextRange::cursor(0));
    buffer.extend_selection_left();
    assert_eq!(buffer.selection(), TextRange::cursor(0));
}

#[test]
fn extending_left_and_then_right_returns_to_where_it_began() {
    // Shift-left then shift-right is the commonest correction a user makes in
    // a text field. It only collapses back if both operations move the same
    // end of the range and leave the anchor alone.
    let mut buffer = TextFieldBuffer::with_selection("hello", TextRange::cursor(2));
    buffer.extend_selection_left();
    assert!(buffer.has_selection());
    buffer.extend_selection_right();
    assert_eq!(
        buffer.selection(),
        TextRange::cursor(2),
        "shift-left then shift-right left a selection behind"
    );

    buffer.extend_selection_right();
    assert!(buffer.has_selection());
    buffer.extend_selection_left();
    assert_eq!(
        buffer.selection(),
        TextRange::cursor(2),
        "shift-right then shift-left left a selection behind"
    );
}

#[test]
fn extending_the_selection_right_grows_it_one_character_at_a_time() {
    let mut buffer = TextFieldBuffer::with_selection("hello", TextRange::cursor(0));
    buffer.extend_selection_right();
    assert_eq!(buffer.selection(), TextRange::new(0, 1));
    buffer.extend_selection_right();
    assert_eq!(buffer.selection(), TextRange::new(0, 2));
    assert_eq!(buffer.copy_selection().as_deref(), Some("he"));
}

#[test]
fn extending_the_selection_right_stops_at_the_end() {
    let mut buffer = TextFieldBuffer::new("ab");
    buffer.extend_selection_right();
    assert_eq!(buffer.selection(), TextRange::cursor(2));
}

#[test]
fn extending_the_selection_walks_whole_characters() {
    let mut buffer = TextFieldBuffer::with_selection("éa", TextRange::cursor(0));
    buffer.extend_selection_right();
    assert_eq!(
        buffer.copy_selection().as_deref(),
        Some("é"),
        "the selection stopped inside a multi-byte character"
    );
}
