use super::state::SlotWriteSessionState;
use crate::{
    collections::map::{HashMap, HashSet},
    slot_storage::{GroupKey, GroupKeySeed},
    Key,
};

impl SlotWriteSessionState {
    fn next_key_ordinal(map: &mut HashMap<Key, u32>, key: Key) -> u32 {
        let ordinal = map.get(&key).copied().unwrap_or(0);
        map.insert(key, ordinal + 1);
        ordinal
    }

    fn expected_key_ordinal(map: &HashMap<Key, u32>, key: Key) -> u32 {
        map.get(&key).copied().unwrap_or(0)
    }

    fn current_key_ordinals(&mut self) -> &mut HashMap<Key, u32> {
        if let Some(frame) = self.group_stack.last_mut() {
            &mut frame.key_ordinals
        } else {
            &mut self.root.key_ordinals
        }
    }

    fn current_key_ordinals_ref(&self) -> &HashMap<Key, u32> {
        if let Some(frame) = self.group_stack.last() {
            &frame.key_ordinals
        } else {
            &self.root.key_ordinals
        }
    }

    fn current_seen_group_keys(&mut self) -> &mut HashSet<GroupKey> {
        if let Some(frame) = self.group_stack.last_mut() {
            &mut frame.seen_group_keys
        } else {
            &mut self.root.seen_group_keys
        }
    }

    pub(in crate::slot) fn preview_group_key(&self, seed: GroupKeySeed) -> GroupKey {
        let ordinal = seed.explicit_key.map_or_else(
            || Self::expected_key_ordinal(self.current_key_ordinals_ref(), seed.static_key),
            |_| 0,
        );
        GroupKey::new(seed.static_key, seed.explicit_key, ordinal)
    }

    pub(in crate::slot) fn consume_group_key(&mut self, key: GroupKey) {
        let ordinal = key.explicit_key.map_or_else(
            || Self::next_key_ordinal(self.current_key_ordinals(), key.static_key),
            |_| 0,
        );
        debug_assert_eq!(
            ordinal, key.ordinal,
            "reserved group ordinal must match the active writer state"
        );
        assert!(
            self.current_seen_group_keys().insert(key),
            "duplicate sibling group key: {:?}",
            key,
        );
    }
}
