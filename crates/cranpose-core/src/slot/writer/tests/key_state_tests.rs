use super::*;

fn group_key(key: Key) -> GroupKey {
    GroupKey::new(key, None, 0)
}

#[test]
fn inline_ordinals_track_repeated_unkeyed_calls() {
    let mut state = FrameKeyState::default();

    assert_eq!(state.expected_ordinal(10), 0);
    assert_eq!(state.next_ordinal(10), 0);
    assert_eq!(state.expected_ordinal(10), 1);
    assert_eq!(state.next_ordinal(10), 1);
    assert_eq!(state.expected_ordinal(10), 2);
    assert!(!state.ordinals_are_promoted());
}

#[test]
fn ordinals_promote_after_inline_capacity() {
    let mut state = FrameKeyState::default();

    for key in 0..INLINE_KEY_STATE_CAPACITY as Key {
        assert_eq!(state.next_ordinal(key), 0);
    }
    assert!(!state.ordinals_are_promoted());

    assert_eq!(state.next_ordinal(100), 0);
    assert!(state.ordinals_are_promoted());
    assert_eq!(state.next_ordinal(3), 1);
    assert_eq!(state.expected_ordinal(3), 2);
}

#[test]
fn inline_seen_keys_reject_duplicates() {
    let mut state = FrameKeyState::default();
    let key = group_key(21);

    assert!(state.insert_seen(key));
    assert!(!state.insert_seen(key));
    assert!(!state.seen_is_promoted());
}

#[test]
fn seen_keys_promote_after_inline_capacity() {
    let mut state = FrameKeyState::default();

    for key in 0..INLINE_KEY_STATE_CAPACITY as Key {
        assert!(state.insert_seen(group_key(key)));
    }
    assert!(!state.seen_is_promoted());

    assert!(state.insert_seen(group_key(100)));
    assert!(state.seen_is_promoted());
    assert!(!state.insert_seen(group_key(3)));
}

#[test]
fn clear_keeps_promoted_storage_reusable() {
    let mut state = FrameKeyState::default();
    for key in 0..=INLINE_KEY_STATE_CAPACITY as Key {
        let _ = state.next_ordinal(key);
        let _ = state.insert_seen(group_key(key));
    }
    assert!(state.ordinals_are_promoted());
    assert!(state.seen_is_promoted());

    state.clear();

    assert!(state.ordinals_are_promoted());
    assert!(state.seen_is_promoted());
    assert_eq!(state.expected_ordinal(1), 0);
    assert_eq!(state.next_ordinal(1), 0);
    assert!(state.insert_seen(group_key(1)));
}
