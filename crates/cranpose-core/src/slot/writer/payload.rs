use super::super::{DeferredDrop, PayloadKind, SlotWriteSession};
use crate::{slot_storage::ValueSlotId, Owned};

impl SlotWriteSession<'_> {
    #[cfg(test)]
    pub(crate) fn value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> ValueSlotId {
        self.value_slot_with_kind(PayloadKind::Internal, init)
    }

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
        let group_index = self.table.current_group_index(group_anchor);
        let payload_len = self.table.group_payload_len_at(group_index);

        let (anchor, generation) = if frame.payload_cursor < payload_len {
            let (anchor, generation) = self
                .table
                .payload_slot_identity_at(group_index, frame.payload_cursor);
            if self
                .table
                .payload_value_is::<T>(group_index, frame.payload_cursor)
            {
                self.table
                    .update_payload_kind(group_index, frame.payload_cursor, kind);
                (anchor, generation)
            } else {
                let (anchor, generation, old_value) = self.table.replace_payload_identity(
                    group_index,
                    frame.payload_cursor,
                    kind,
                    init(),
                );
                self.lifecycle.queue_drop(DeferredDrop::payload(old_value));
                (anchor, generation)
            }
        } else {
            let (anchor, generation) =
                self.table
                    .insert_value_payload(group_anchor, frame.payload_cursor, kind, init());
            (anchor, generation)
        };

        let slot = ValueSlotId::new(anchor, generation);
        frame.payload_cursor += 1;
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
