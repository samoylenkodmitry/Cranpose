use super::super::{
    GroupKeySeed, PayloadAnchor, PayloadInit, PayloadKind, SlotWriteSession, ValueSlotId,
};
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
        source: crate::Key,
        init: impl FnOnce() -> T,
    ) -> ValueSlotId {
        self.located_value_slot(kind, source, init).0
    }

    /// The value slot at the cursor and its value, found by one lookup.
    pub(crate) fn value_slot_and_value<T: 'static>(
        &mut self,
        kind: PayloadKind,
        source: crate::Key,
        init: impl FnOnce() -> T,
    ) -> (ValueSlotId, &mut T) {
        let (slot, record_index) = self.located_value_slot(kind, source, init);
        (slot, self.table.value_at_mut(slot, record_index))
    }

    fn located_value_slot<T: 'static>(
        &mut self,
        kind: PayloadKind,
        source: crate::Key,
        init: impl FnOnce() -> T,
    ) -> (ValueSlotId, Option<usize>) {
        let mut init = Some(init);
        let mut make = move || -> Box<dyn std::any::Any> {
            Box::new(init.take().expect("payload init must run at most once")())
        };
        let mut init = PayloadInit::new::<T>(source, &mut make);
        self.value_slot_with_kind_dyn(kind, &mut init)
    }

    /// The value slot at the cursor, and the index of its payload record
    /// when it has one.
    #[inline(never)]
    fn value_slot_with_kind_dyn(
        &mut self,
        kind: PayloadKind,
        init: &mut PayloadInit<'_>,
    ) -> (ValueSlotId, Option<usize>) {
        init.mix_source(self.state.branch_fold());
        let Some((group_anchor, group_index, payload_cursor)) = self.live_value_slot_frame() else {
            return (self.recover_value_slot_with_kind(kind, init), None);
        };
        self.value_slot_in_group(group_anchor, group_index, payload_cursor, kind, init)
    }

    /// The top frame's group, its index and payload cursor, after dropping
    /// frames whose group no longer resolves.
    fn live_value_slot_frame(&mut self) -> Option<(crate::AnchorId, usize, usize)> {
        loop {
            let frame = self.state.group_stack.last()?;
            let (group_anchor, payload_cursor) = (frame.group_anchor, frame.payload_cursor);
            if let Some(group_index) = self.table.open_group_index(group_anchor, frame.group_index)
            {
                return Some((group_anchor, group_index, payload_cursor));
            }
            log::error!(
                "slot writer discarded stale value-slot group frame for anchor {group_anchor:?}"
            );
            self.state.pop_group_frame()?;
        }
    }

    fn value_slot_in_group(
        &mut self,
        group_anchor: crate::AnchorId,
        group_index: usize,
        payload_cursor: usize,
        kind: PayloadKind,
        init: &mut PayloadInit<'_>,
    ) -> (ValueSlotId, Option<usize>) {
        let (slot, record_index, location_refresh) = self.table.use_value_payload_at_cursor(
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
        if slot.anchor() == PayloadAnchor::INVALID {
            log::error!(
                "slot writer returned an invalid value slot after payload allocation failed"
            );
            return (slot, None);
        }
        if let Some(frame) = self.state.group_stack.last_mut() {
            frame.advance_payload_cursor();
        } else {
            log::error!("slot writer value-slot group frame disappeared before cursor advance");
        }
        (slot, record_index)
    }

    fn recover_value_slot_with_kind(
        &mut self,
        kind: PayloadKind,
        init: &mut PayloadInit<'_>,
    ) -> ValueSlotId {
        log::error!(
            "slot writer value slot requested with an empty group stack; recording recovery group"
        );
        // A frame whose anchor no longer resolves may come from a wrong index:
        // rewrite them all from the groups before inserting more.
        self.table.rewrite_group_indexes();
        let key = self.preview_group_key(GroupKeySeed::unkeyed(RECOVERY_VALUE_SLOT_STATIC_KEY));
        self.begin_group(key, None, None);
        let slot = match self.live_value_slot_frame() {
            Some((group_anchor, group_index, payload_cursor)) => {
                self.value_slot_in_group(group_anchor, group_index, payload_cursor, kind, init)
                    .0
            }
            None => {
                log::error!("slot writer could not open a recovery group for a value slot");
                ValueSlotId::new_for_table(PayloadAnchor::INVALID, self.table.storage_id())
            }
        };
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

    pub(crate) fn remember<T: 'static>(
        &mut self,
        source: crate::Key,
        init: impl FnOnce() -> T,
    ) -> Owned<T> {
        self.remember_with_kind(PayloadKind::Remember, source, init)
    }

    pub(crate) fn remember_effect<T: Default + 'static>(&mut self, source: crate::Key) -> Owned<T> {
        let mut made = false;
        let mut make = move || -> Box<dyn std::any::Any> {
            assert!(!made, "payload init must run at most once");
            made = true;
            Box::new(Owned::new(T::default()))
        };
        let mut init = PayloadInit::new_effect::<T>(source, &mut make);
        let (slot, record_index) = self.value_slot_with_kind_dyn(PayloadKind::Effect, &mut init);
        self.table
            .value_at_mut::<Owned<T>>(slot, record_index)
            .clone()
    }

    pub(crate) fn remember_with_kind<T: 'static>(
        &mut self,
        kind: PayloadKind,
        source: crate::Key,
        init: impl FnOnce() -> T,
    ) -> Owned<T> {
        let (slot, record_index) = self.located_value_slot(kind, source, || {
            let (value, states) = crate::runtime::collecting_states(init);
            Owned::with_states(value, states)
        });
        #[cfg(any(test, debug_assertions))]
        debug_assert!(
            self.table
                .payload_anchor_active_location(slot.anchor())
                .is_some(),
            "remember must only read a value slot with an active payload anchor"
        );
        self.table
            .value_at_mut::<Owned<T>>(slot, record_index)
            .clone()
    }
}
