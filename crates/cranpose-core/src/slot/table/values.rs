use super::SlotTable;
use crate::slot_storage::ValueSlotId;

impl SlotTable {
    fn checked_value_slot(&self, slot: ValueSlotId) -> (usize, usize) {
        let (owner, payload_index) = self
            .payload_locations
            .get(slot.anchor())
            .expect("value slot anchor should resolve");
        let group_index = self.current_group_index(owner);
        debug_assert_eq!(
            self.payload_owner_at(group_index, payload_index),
            owner,
            "payload location owner must match the payload record owner"
        );
        assert_eq!(
            self.payload_anchor_at(group_index, payload_index),
            slot.anchor(),
            "value slot anchor mismatch"
        );
        assert_eq!(
            self.payload_generation_at(group_index, payload_index),
            slot.anchor().generation(),
            "value slot generation mismatch"
        );
        (group_index, payload_index)
    }

    pub(crate) fn read_value<T: 'static>(&self, slot: ValueSlotId) -> &T {
        let (group_index, payload_index) = self.checked_value_slot(slot);
        self.group_payload_record_at(group_index, payload_index)
            .value
            .downcast_ref::<T>()
            .expect("value slot type mismatch")
    }

    pub(crate) fn read_value_mut<T: 'static>(&mut self, slot: ValueSlotId) -> &mut T {
        let (group_index, payload_index) = self.checked_value_slot(slot);
        self.group_payload_record_at_mut(group_index, payload_index)
            .value
            .downcast_mut::<T>()
            .expect("value slot type mismatch")
    }

    pub(crate) fn write_value<T: 'static>(&mut self, slot: ValueSlotId, value: T) {
        let (group_index, payload_index) = self.checked_value_slot(slot);
        let kind = self
            .group_payload_record_at(group_index, payload_index)
            .kind;
        drop(self.replace_payload_value(group_index, payload_index, kind, value));
    }
}
