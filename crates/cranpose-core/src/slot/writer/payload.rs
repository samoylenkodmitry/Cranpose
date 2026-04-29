use super::super::{PayloadKind, SlotWriteSession, ValueSlotId};
use crate::Owned;

impl SlotWriteSession<'_> {
    pub(in crate::slot) fn flush_payload_location_refreshes(&mut self) {
        self.table.flush_payload_location_refreshes(self.state);
    }

    pub(crate) fn value_slot_with_kind<T: 'static>(
        &mut self,
        kind: PayloadKind,
        init: impl FnOnce() -> T,
    ) -> ValueSlotId {
        let (group_anchor, payload_cursor) = {
            let frame = self
                .state
                .group_stack
                .last()
                .expect("value slots require an active group");
            (frame.group_anchor, frame.payload_cursor)
        };
        let (slot, deferred_drop, location_refresh) =
            self.table
                .use_value_payload_at_cursor(group_anchor, payload_cursor, kind, init);
        if let Some(location_refresh) = location_refresh {
            self.state
                .note_payload_location_refresh(location_refresh.owner, location_refresh.start);
        }
        if let Some(deferred_drop) = deferred_drop {
            self.lifecycle.queue_drop(deferred_drop);
        }
        self.state
            .group_stack
            .last_mut()
            .expect("value slots require an active group")
            .advance_payload_cursor();
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
