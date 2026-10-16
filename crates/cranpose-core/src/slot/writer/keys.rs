use super::{
    super::{GroupKey, GroupKeySeed},
    key_state::FrameKeyState,
    state::SlotWriteSessionState,
};

impl SlotWriteSessionState {
    fn current_keys(&mut self) -> &mut FrameKeyState {
        if let Some(frame) = self.group_stack.last_mut() {
            &mut frame.keys
        } else {
            &mut self.root.keys
        }
    }

    /// The key of the next child group `seed` begins. An unkeyed group takes
    /// its ordinal here, so the group must begin next; a keyed one is
    /// recorded when it begins, by [`Self::consume_group_key`].
    pub(in crate::slot) fn reserve_group_key(&mut self, seed: GroupKeySeed) -> GroupKey {
        let static_key = if seed.exact {
            seed.static_key
        } else {
            self.mix_branch_fold(seed.static_key)
        };
        let ordinal = if seed.explicit_key.is_some() {
            0
        } else {
            self.current_keys().next_ordinal(static_key)
        };
        GroupKey::new(static_key, seed.explicit_key, ordinal)
    }

    /// Records a beginning keyed group among its siblings. An unkeyed group
    /// took its ordinal when its key was reserved.
    pub(in crate::slot) fn consume_group_key(&mut self, key: GroupKey) {
        if key.explicit_key.is_none() {
            return;
        }
        debug_assert_eq!(key.ordinal, 0, "a keyed group has no ordinal");
        assert!(
            self.current_keys().insert_seen(key),
            "duplicate sibling group key: {key:?}",
        );
    }
}
