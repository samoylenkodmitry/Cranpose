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

    /// The branch fold currently in effect, for identity captured at a
    /// creation site rather than at slot-write time — a provider entry built
    /// in one arm must not share identity with one built in another.
    pub(crate) fn active_branch_fold(&mut self) -> crate::Key {
        self.state.branch_fold()
    }

    /// Thin monomorphic shim: the payload machinery below works on a
    /// [`PayloadInit`] so it is compiled once instead of once per remembered
    /// value type.
    pub(crate) fn value_slot_with_kind<T: 'static>(
        &mut self,
        kind: PayloadKind,
        source: crate::Key,
        init: impl FnOnce() -> T,
    ) -> ValueSlotId {
        let mut init = Some(init);
        let mut make = move || -> Box<dyn std::any::Any> {
            Box::new(init.take().expect("payload init must run at most once")())
        };
        let mut init = PayloadInit::new::<T>(source, &mut make);
        self.value_slot_with_kind_dyn(kind, &mut init)
    }

    /// `inline(never)`: keep one compiled copy of the payload machinery
    /// instead of re-inlining it into every monomorphic shim under fat LTO.
    #[inline(never)]
    fn value_slot_with_kind_dyn(
        &mut self,
        kind: PayloadKind,
        init: &mut PayloadInit<'_>,
    ) -> ValueSlotId {
        init.mix_source(self.state.branch_fold());
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

    fn value_slot_in_active_group(
        &mut self,
        group_anchor: crate::AnchorId,
        payload_cursor: usize,
        kind: PayloadKind,
        init: &mut PayloadInit<'_>,
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

    fn recover_value_slot_with_kind(
        &mut self,
        kind: PayloadKind,
        init: &mut PayloadInit<'_>,
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
        let mut init = PayloadInit::new_startable::<Owned<T>>(source, &mut make, || {
            Box::new(Owned::new(T::default())) as Box<dyn std::any::Any>
        });
        let slot = self.value_slot_with_kind_dyn(PayloadKind::Effect, &mut init);
        self.table.read_value::<Owned<T>>(slot).clone()
    }

    pub(crate) fn remember_with_kind<T: 'static>(
        &mut self,
        kind: PayloadKind,
        source: crate::Key,
        init: impl FnOnce() -> T,
    ) -> Owned<T> {
        let slot = self.value_slot_with_kind(kind, source, || Owned::new(init()));
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
