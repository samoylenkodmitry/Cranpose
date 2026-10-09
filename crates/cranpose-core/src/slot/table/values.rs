use std::fmt;

use super::{
    super::{PayloadRecord, ValueSlotId},
    SlotTable,
};
use crate::{AnchorId, slot::PayloadAnchor};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ValueSlotError {
    ForeignTable {
        table_storage_id: usize,
        slot_storage_id: usize,
    },
    InactiveAnchor {
        anchor: PayloadAnchor,
    },
    InactiveOwner {
        anchor: PayloadAnchor,
        owner: AnchorId,
    },
    PayloadIndexOutOfRange {
        anchor: PayloadAnchor,
        owner: AnchorId,
        group_index: usize,
        payload_index: usize,
        payload_len: usize,
        payload_count: usize,
    },
    AnchorMismatch {
        expected: PayloadAnchor,
        actual: PayloadAnchor,
    },
    TypeMismatch {
        anchor: PayloadAnchor,
        expected: &'static str,
        actual: &'static str,
    },
}

impl fmt::Display for ValueSlotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValueSlotError::ForeignTable {
                table_storage_id,
                slot_storage_id,
            } => write!(
                f,
                "value slot belongs to storage {slot_storage_id}, not slot table storage {table_storage_id}"
            ),
            ValueSlotError::InactiveAnchor { anchor } => {
                write!(f, "value slot anchor {anchor:?} is not active")
            }
            ValueSlotError::InactiveOwner { anchor, owner } => write!(
                f,
                "value slot anchor {anchor:?} points at inactive owner {owner:?}"
            ),
            ValueSlotError::PayloadIndexOutOfRange {
                anchor,
                owner,
                group_index,
                payload_index,
                payload_len,
                payload_count,
            } => write!(
                f,
                "value slot anchor {anchor:?} points at payload index {payload_index} for owner {owner:?} in group {group_index}, but the group has {payload_len} payloads and the table has {payload_count} payload records"
            ),
            ValueSlotError::AnchorMismatch { expected, actual } => write!(
                f,
                "value slot anchor mismatch: expected {expected:?}, found {actual:?}"
            ),
            ValueSlotError::TypeMismatch {
                anchor,
                expected,
                actual,
            } => write!(f, "value slot {anchor:?} has type {actual}, not {expected}"),
        }
    }
}

impl std::error::Error for ValueSlotError {}

#[derive(Clone, Copy)]
struct CheckedValueSlot {
    #[cfg(test)]
    group_index: usize,
    #[cfg(test)]
    payload_index: usize,
    absolute_payload_index: usize,
}

impl SlotTable {
    fn checked_value_slot(
        &self,
        slot: ValueSlotId,
    ) -> Result<(CheckedValueSlot, &PayloadRecord), ValueSlotError> {
        if slot.storage_id() != self.storage_id() {
            return Err(ValueSlotError::ForeignTable {
                table_storage_id: self.storage_id(),
                slot_storage_id: slot.storage_id(),
            });
        }
        let (owner, payload_index) = self
            .payload_anchors
            .active_location(slot.anchor())
            .ok_or_else(|| ValueSlotError::InactiveAnchor {
                anchor: slot.anchor(),
            })?;
        let group_index =
            self.active_group_index(owner)
                .ok_or_else(|| ValueSlotError::InactiveOwner {
                    anchor: slot.anchor(),
                    owner,
                })?;
        let group = &self.groups[group_index];
        let payload_start = group.payload_start as usize;
        let payload_len = group.payload_len as usize;
        let absolute_payload_index = payload_start.saturating_add(payload_index);
        let record = if payload_index < payload_len {
            self.payloads.get(absolute_payload_index)
        } else {
            None
        };
        let Some(record) = record else {
            return Err(ValueSlotError::PayloadIndexOutOfRange {
                anchor: slot.anchor(),
                owner,
                group_index,
                payload_index,
                payload_len,
                payload_count: self.payloads.len(),
            });
        };
        if record.anchor != slot.anchor() {
            return Err(ValueSlotError::AnchorMismatch {
                expected: slot.anchor(),
                actual: record.anchor,
            });
        }
        Ok((
            CheckedValueSlot {
                #[cfg(test)]
                group_index,
                #[cfg(test)]
                payload_index,
                absolute_payload_index,
            },
            record,
        ))
    }

    /// The index of the slot's payload record: `record` when the slot's
    /// record sits there, else found through the slot's anchor.
    #[inline]
    fn value_record_index(
        &self,
        slot: ValueSlotId,
        record: Option<usize>,
    ) -> Result<usize, ValueSlotError> {
        if let Some(index) = record
            && slot.storage_id() == self.storage_id()
            && self
                .payloads
                .get(index)
                .is_some_and(|payload| payload.anchor == slot.anchor())
        {
            return Ok(index);
        }
        self.anchored_value_record_index(slot)
    }

    #[inline(never)]
    fn anchored_value_record_index(&self, slot: ValueSlotId) -> Result<usize, ValueSlotError> {
        Ok(self.checked_value_slot(slot)?.0.absolute_payload_index)
    }

    pub(crate) fn try_value_at<T: 'static>(
        &self,
        slot: ValueSlotId,
        record: Option<usize>,
    ) -> Result<&T, ValueSlotError> {
        let index = self.value_record_index(slot, record)?;
        let payload = self
            .payloads
            .get(index)
            .ok_or_else(|| ValueSlotError::InactiveAnchor {
                anchor: slot.anchor(),
            })?;
        payload
            .value
            .downcast_ref::<T>()
            .ok_or_else(|| ValueSlotError::TypeMismatch {
                anchor: slot.anchor(),
                expected: std::any::type_name::<T>(),
                actual: (payload.payload_type.type_name)(),
            })
    }

    fn try_value_at_mut<T: 'static>(
        &mut self,
        slot: ValueSlotId,
        record: Option<usize>,
    ) -> Result<&mut T, ValueSlotError> {
        let index = self.value_record_index(slot, record)?;
        let payload =
            self.payloads
                .get_mut(index)
                .ok_or_else(|| ValueSlotError::InactiveAnchor {
                    anchor: slot.anchor(),
                })?;
        payload
            .value
            .downcast_mut::<T>()
            .ok_or_else(|| ValueSlotError::TypeMismatch {
                anchor: slot.anchor(),
                expected: std::any::type_name::<T>(),
                actual: (payload.payload_type.type_name)(),
            })
    }

    #[cfg(test)]
    pub(crate) fn read_value<T: 'static>(&self, slot: ValueSlotId) -> &T {
        self.value_at(slot, None)
    }

    /// The slot's value, read from `record` when the slot's payload record
    /// sits there.
    pub(crate) fn value_at<T: 'static>(&self, slot: ValueSlotId, record: Option<usize>) -> &T {
        self.try_value_at(slot, record)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    pub(crate) fn value_at_mut<T: 'static>(
        &mut self,
        slot: ValueSlotId,
        record: Option<usize>,
    ) -> &mut T {
        self.try_value_at_mut(slot, record)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    #[cfg(test)]
    pub(crate) fn try_write_value<T: 'static>(
        &mut self,
        slot: ValueSlotId,
        value: T,
    ) -> Result<(), ValueSlotError> {
        let (checked, record) = self.checked_value_slot(slot)?;
        let kind = record.kind;
        drop(self.replace_payload_value(checked.group_index, checked.payload_index, kind, value));
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn write_value<T: 'static>(&mut self, slot: ValueSlotId, value: T) {
        self.try_write_value(slot, value)
            .unwrap_or_else(|error| panic!("{error}"));
    }
}
