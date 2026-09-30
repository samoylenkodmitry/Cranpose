use super::*;

#[test]
fn recycled_frames_stay_outside_the_active_stack() {
    let mut stack = GroupStack::default();
    assert_eq!(stack.pop(), None);
    stack.push().reset(AnchorId::new(1), 2, 3, 4);
    stack.push().reset(AnchorId::new(2), 5, 6, 7);
    assert_eq!(
        stack
            .iter()
            .map(|frame| frame.group_anchor)
            .collect::<Vec<_>>(),
        vec![AnchorId::new(1), AnchorId::new(2)]
    );
    let capacity = stack.capacity();
    assert_eq!(stack.pop(), Some(AnchorId::new(2)));
    assert_eq!(stack.len(), 1);
    assert_eq!(
        stack.last().expect("parent frame").group_anchor,
        AnchorId::new(1)
    );
    stack
        .last_mut()
        .expect("parent frame")
        .advance_node_cursor();
    assert_eq!(stack[0].node_cursor, 1);
    stack.push().reset(AnchorId::new(3), 8, 9, 10);
    assert_eq!(
        stack.last().expect("reused child").group_anchor,
        AnchorId::new(3)
    );
    assert_eq!(stack[0].node_cursor, 1);
    assert_eq!(stack.capacity(), capacity);
    assert_eq!(stack.pop(), Some(AnchorId::new(3)));
    assert_eq!(stack.pop(), Some(AnchorId::new(1)));
    assert!(stack.is_empty());
    assert_eq!(stack.iter().count(), 0);
    assert!(stack.last_mut().is_none());
    assert_eq!(stack.pop(), None);
    assert_eq!(stack.capacity(), capacity);
}

#[test]
fn popped_frames_clear_keys_and_release_sibling_indices() {
    let mut stack = GroupStack::default();
    let frame = stack.push();
    frame.reset(AnchorId::new(1), 2, 3, 4);
    for key in 0..32 {
        assert_eq!(frame.keys.next_ordinal(key), 0);
    }
    frame.sibling_index = Some(SiblingIndex::default());
    frame.advance_payload_cursor();
    frame.advance_node_cursor();
    frame.mark_body_finished();
    frame.was_skipped = true;
    frame.fold_watermark = 5;
    assert_eq!(stack.pop(), Some(AnchorId::new(1)));
    let frame = &stack.frames[0];
    assert!(frame.sibling_index.is_none());
    for key in 0..32 {
        assert_eq!(frame.keys.expected_ordinal(key), 0);
    }
    let frame = stack.push();
    frame.reset(AnchorId::new(2), 6, 7, 8);
    assert_eq!(frame.group_anchor, AnchorId::new(2));
    assert_eq!(frame.next_child_index, 6);
    assert_eq!(frame.old_payload_len, 7);
    assert_eq!(frame.old_node_len, 8);
    assert_eq!(frame.payload_cursor, 0);
    assert_eq!(frame.node_cursor, 0);
    assert!(!frame.body_finished);
    assert!(!frame.was_skipped);
    assert_eq!(frame.fold_watermark, 0);
    assert_eq!(frame.keys.next_ordinal(31), 0);
}
