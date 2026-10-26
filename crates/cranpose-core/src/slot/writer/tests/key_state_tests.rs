use super::*;

fn group_key(key: Key) -> GroupKey {
    GroupKey::new(key, None, 0)
}

#[test]
fn inline_ordinals_track_repeated_unkeyed_calls() {
    let mut state = FrameKeyState::default();

    assert_eq!(state.next_ordinal(Key(10)), 0);
    assert_eq!(state.next_ordinal(Key(10)), 1);
    assert_eq!(state.next_ordinal(Key(10)), 2);
    assert!(!state.ordinals_are_promoted());
}

#[test]
fn ordinals_promote_after_inline_capacity() {
    let mut state = FrameKeyState::default();

    for key in 0..INLINE_KEY_STATE_CAPACITY as u64 {
        assert_eq!(state.next_ordinal(Key(key)), 0);
    }
    assert!(!state.ordinals_are_promoted());

    assert_eq!(state.next_ordinal(Key(100)), 0);
    assert!(state.ordinals_are_promoted());
    assert_eq!(state.next_ordinal(Key(3)), 1);
    assert_eq!(state.next_ordinal(Key(3)), 2);
}

#[test]
fn inline_seen_keys_reject_duplicates() {
    let mut state = FrameKeyState::default();
    let key = group_key(Key(21));

    assert!(state.insert_seen(key));
    assert!(!state.insert_seen(key));
    assert!(!state.seen_is_promoted());
}

#[test]
fn seen_keys_promote_after_inline_capacity() {
    let mut state = FrameKeyState::default();

    for key in 0..INLINE_KEY_STATE_CAPACITY as u64 {
        assert!(state.insert_seen(group_key(Key(key))));
    }
    assert!(!state.seen_is_promoted());

    assert!(state.insert_seen(group_key(Key(100))));
    assert!(state.seen_is_promoted());
    assert!(!state.insert_seen(group_key(Key(3))));
}

#[test]
fn a_cleared_frame_forgets_spilled_keys() {
    let mut state = FrameKeyState::default();
    for key in 0..=INLINE_KEY_STATE_CAPACITY as u64 {
        let _ = state.next_ordinal(Key(key));
        let _ = state.insert_seen(group_key(Key(key)));
    }
    assert!(state.ordinals_are_promoted());
    assert!(state.seen_is_promoted());

    state.clear();

    assert_eq!(state.next_ordinal(Key(1)), 0);
    assert!(state.insert_seen(group_key(Key(1))));
    assert!(!state.insert_seen(group_key(Key(1))));
    for key in 0..=INLINE_KEY_STATE_CAPACITY as u64 {
        let _ = state.next_ordinal(Key(key + 100));
    }
    assert_eq!(state.next_ordinal(Key(1)), 1);
    assert_eq!(
        state.next_ordinal(Key(INLINE_KEY_STATE_CAPACITY as u64 + 100)),
        1
    );
}
