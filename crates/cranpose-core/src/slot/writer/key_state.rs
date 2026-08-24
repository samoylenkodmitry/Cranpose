use smallvec::SmallVec;

use super::super::GroupKey;
use crate::{
    collections::map::{HashMap, HashSet},
    Key,
};

const INLINE_KEY_STATE_CAPACITY: usize = 8;

#[derive(Default)]
pub(in crate::slot) struct FrameKeyState {
    ordinal_entries: SmallVec<[(Key, u32); INLINE_KEY_STATE_CAPACITY]>,
    ordinal_map: Option<HashMap<Key, u32>>,
    seen_entries: SmallVec<[GroupKey; INLINE_KEY_STATE_CAPACITY]>,
    seen_set: Option<HashSet<GroupKey>>,
}

impl FrameKeyState {
    pub(in crate::slot) fn clear(&mut self) {
        self.ordinal_entries.clear();
        if let Some(map) = &mut self.ordinal_map {
            map.clear();
        }
        self.seen_entries.clear();
        if let Some(set) = &mut self.seen_set {
            set.clear();
        }
    }

    pub(in crate::slot) fn expected_ordinal(&self, key: Key) -> u32 {
        if let Some(map) = &self.ordinal_map {
            return map.get(&key).copied().unwrap_or(0);
        }
        self.ordinal_entries
            .iter()
            .find_map(|(entry_key, ordinal)| (*entry_key == key).then_some(*ordinal))
            .unwrap_or(0)
    }

    pub(in crate::slot) fn next_ordinal(&mut self, key: Key) -> u32 {
        if let Some(map) = &mut self.ordinal_map {
            let ordinal = map.get(&key).copied().unwrap_or(0);
            map.insert(key, ordinal + 1);
            return ordinal;
        }

        if let Some((_, ordinal)) = self
            .ordinal_entries
            .iter_mut()
            .find(|(entry_key, _)| *entry_key == key)
        {
            let current = *ordinal;
            *ordinal += 1;
            return current;
        }

        if self.ordinal_entries.len() < INLINE_KEY_STATE_CAPACITY {
            self.ordinal_entries.push((key, 1));
            return 0;
        }

        let map = self.promote_ordinals();
        map.insert(key, 1);
        0
    }

    pub(in crate::slot) fn insert_seen(&mut self, key: GroupKey) -> bool {
        if let Some(set) = &mut self.seen_set {
            return set.insert(key);
        }

        if self.seen_entries.contains(&key) {
            return false;
        }

        if self.seen_entries.len() < INLINE_KEY_STATE_CAPACITY {
            self.seen_entries.push(key);
            return true;
        }

        self.promote_seen().insert(key)
    }

    fn promote_ordinals(&mut self) -> &mut HashMap<Key, u32> {
        let mut map = HashMap::default();
        map.reserve(self.ordinal_entries.len() * 2);
        for (key, ordinal) in self.ordinal_entries.drain(..) {
            map.insert(key, ordinal);
        }
        self.ordinal_map.insert(map)
    }

    fn promote_seen(&mut self) -> &mut HashSet<GroupKey> {
        let mut set = HashSet::default();
        set.reserve(self.seen_entries.len() * 2);
        for key in self.seen_entries.drain(..) {
            set.insert(key);
        }
        self.seen_set.insert(set)
    }

    #[cfg(test)]
    fn ordinals_are_promoted(&self) -> bool {
        self.ordinal_map.is_some()
    }

    #[cfg(test)]
    fn seen_is_promoted(&self) -> bool {
        self.seen_set.is_some()
    }
}

#[cfg(test)]
mod tests {
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
}
