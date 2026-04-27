use super::super::{PayloadKind, SlotWriteSession};
use crate::{slot_storage::ValueSlotId, Owned};

impl SlotWriteSession<'_> {
    pub(crate) fn value_slot_with_kind<T: 'static>(
        &mut self,
        kind: PayloadKind,
        init: impl FnOnce() -> T,
    ) -> ValueSlotId {
        let frame = self
            .state
            .group_stack
            .last_mut()
            .expect("value slots require an active group");
        let group_anchor = frame.group_anchor;
        let (slot, deferred_drop) =
            self.table
                .use_value_payload_at_cursor(group_anchor, frame.payload_cursor, kind, init);
        if let Some(deferred_drop) = deferred_drop {
            self.lifecycle.queue_drop(deferred_drop);
        }
        frame.advance_payload_cursor();
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
        self.table.read_value::<Owned<T>>(slot).clone()
    }
}
