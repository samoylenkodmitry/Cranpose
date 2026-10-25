use super::*;

#[test]
fn utf16_offset_converts_and_clamps() {
    let text = "a\u{00e9}\u{6f22}x";
    assert_eq!(utf16_offset(text, 0), 0);
    assert_eq!(utf16_offset(text, 1), 1);
    assert_eq!(utf16_offset(text, 3), 2);
    assert_eq!(utf16_offset(text, 6), 3);
    assert_eq!(utf16_offset(text, 7), 4);
    assert_eq!(utf16_offset(text, 100), 4);
    assert_eq!(utf16_offset(text, 2), 1);
    assert_eq!(utf16_offset(text, 4), 2);
    let emoji = "\u{1f600}b";
    assert_eq!(utf16_offset(emoji, 4), 2);
    assert_eq!(utf16_offset(emoji, 5), 3);
}

#[test]
fn queue_push_and_drain() {
    let queue = AndroidImeEventQueue::new();
    queue.push(AndroidImeEvent::FinishComposing);
    queue.push(AndroidImeEvent::CommitText {
        text: "hi".to_string(),
        new_cursor_position: 1,
    });
    let events = queue.drain();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0], AndroidImeEvent::FinishComposing);
    assert!(queue.drain().is_empty());
}
