use super::super::{GroupKey, GroupKeySeed};
use super::key_state::FrameKeyState;
use super::state::SlotWriteSessionState;

impl SlotWriteSessionState {
    fn current_keys(&mut self) -> &mut FrameKeyState {
        if let Some(frame) = self.group_stack.last_mut() {
            &mut frame.keys
        } else {
            &mut self.root.keys
        }
    }

    fn current_keys_ref(&self) -> &FrameKeyState {
        if let Some(frame) = self.group_stack.last() {
            &frame.keys
        } else {
            &self.root.keys
        }
    }

    pub(in crate::slot) fn preview_group_key(&self, seed: GroupKeySeed) -> GroupKey {
        let ordinal = seed.explicit_key.map_or_else(
            || self.current_keys_ref().expected_ordinal(seed.static_key),
            |_| 0,
        );
        GroupKey::new(seed.static_key, seed.explicit_key, ordinal)
    }

    pub(in crate::slot) fn consume_group_key(&mut self, key: GroupKey) {
        let keys = self.current_keys();
        let ordinal = key
            .explicit_key
            .map_or_else(|| keys.next_ordinal(key.static_key), |_| 0);
        debug_assert_eq!(
            ordinal, key.ordinal,
            "reserved group ordinal must match the active writer state"
        );
        assert!(
            keys.insert_seen(key),
            "duplicate sibling group key: {:?}",
            key,
        );
    }
}
