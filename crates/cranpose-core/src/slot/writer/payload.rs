use super::super::{GroupKeySeed, PayloadAnchor, PayloadKind, SlotWriteSession, ValueSlotId};
use crate::{Key, Owned};

const RECOVERY_VALUE_SLOT_STATIC_KEY: Key = Key::MAX;

impl SlotWriteSession<'_> {
    pub(in crate::slot) fn flush_payload_location_refreshes(&mut self) {
        self.table.flush_payload_location_refreshes(self.state);
        #[cfg(any(test, debug_assertions))]
        self.state
            .debug_assert_no_pending_payload_location_refreshes("payload location flush");
    }

    pub(crate) fn value_slot_with_kind<T: 'static>(
        &mut self,
        kind: PayloadKind,
        init: impl FnOnce() -> T,
    ) -> ValueSlotId {
        self.discard_stale_value_slot_frames();
        let Some(frame) = self.state.group_stack.last() else {
            return self.recover_value_slot_with_kind(kind, init);
        };
        let (group_anchor, payload_cursor) = { (frame.group_anchor, frame.payload_cursor) };
        self.value_slot_in_active_group(group_anchor, payload_cursor, kind, init)
    }

    fn discard_stale_value_slot_frames(&mut self) {
        while let Some(group_anchor) = self
            .state
            .group_stack
            .last()
            .map(|frame| frame.group_anchor)
        {
            if self.table.active_group_index(group_anchor).is_some() {
                return;
            }
            log::error!(
                "slot writer discarded stale value-slot group frame for anchor {group_anchor:?}"
            );
            let Some(frame) = self.state.group_stack.pop() else {
                return;
            };
            self.state.recycle_group_frame(frame);
        }
    }

    fn value_slot_in_active_group<T: 'static>(
        &mut self,
        group_anchor: crate::AnchorId,
        payload_cursor: usize,
        kind: PayloadKind,
        init: impl FnOnce() -> T,
    ) -> ValueSlotId {
        let Some(group_index) = self.table.active_group_index(group_anchor) else {
            log::error!(
                "slot writer recovered value-slot request for stale owner anchor {group_anchor:?}"
            );
            self.discard_stale_value_slot_frames();
            return self.recover_value_slot_with_kind(kind, init);
        };
        let (slot, deferred_drop, location_refresh) = self.table.use_value_payload_at_cursor(
            group_anchor,
            group_index,
            payload_cursor,
            kind,
            init,
        );
        if let Some(location_refresh) = location_refresh {
            self.state
                .note_payload_location_refresh(location_refresh.owner, location_refresh.start);
        }
        if let Some(deferred_drop) = deferred_drop {
            self.lifecycle.queue_drop(deferred_drop);
        }
        if slot.anchor() == PayloadAnchor::INVALID {
            log::error!(
                "slot writer returned an invalid value slot after payload allocation failed"
            );
            return slot;
        }
        if let Some(frame) = self.state.group_stack.last_mut() {
            frame.advance_payload_cursor();
        } else {
            log::error!("slot writer value-slot group frame disappeared before cursor advance");
        }
        slot
    }

    fn recover_value_slot_with_kind<T: 'static>(
        &mut self,
        kind: PayloadKind,
        init: impl FnOnce() -> T,
    ) -> ValueSlotId {
        log::error!(
            "slot writer value slot requested with an empty group stack; recording recovery group"
        );
        let key = self.preview_group_key(GroupKeySeed::unkeyed(RECOVERY_VALUE_SLOT_STATIC_KEY));
        let started = self.begin_group(key, None);
        let slot = self.value_slot_in_active_group(started.anchor, 0, kind, init);
        let result = self.finish_group_body();
        if !result.detached_children.is_empty()
            || !result.direct_nodes.is_empty()
            || !result.root_nodes.is_empty()
        {
            log::error!("slot writer recovery value group produced unexpected detached content");
        }
        self.end_group();
        slot
    }

    pub(crate) fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T> {
        self.remember_with_kind(PayloadKind::Remember, init)
    }

    pub(crate) fn remember_with_kind<T: 'static>(
        &mut self,
        kind: PayloadKind,
        init: impl FnOnce() -> T,
    ) -> Owned<T> {
        let slot = self.value_slot_with_kind(kind, || Owned::new(init()));
        // The current payload anchor is activated before the coalesced range
        // refresh is flushed, so remember can read the value it just requested.
        #[cfg(any(test, debug_assertions))]
        debug_assert!(
            self.table
                .payload_anchor_active_location(slot.anchor())
                .is_some(),
            "remember must only read a value slot with an active payload anchor"
        );
        self.table.read_value::<Owned<T>>(slot).clone()
    }
}
