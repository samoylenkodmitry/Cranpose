use std::{mem, ops::Range};

#[cfg(any(test, debug_assertions))]
use super::SlotInvariantError;
use super::{PayloadRecord, checked_usize_to_u32, growth::GrowthSlack, segments::SegmentItems};

const ORDER_ENTRY_BYTES: usize = mem::size_of::<u32>();

#[derive(Default)]
pub(in crate::slot) struct PayloadStore {
    order: Vec<u32>,
    slots: Vec<Option<PayloadRecord>>,
    free: Vec<u32>,
    shift_bytes: usize,
}

impl PayloadStore {
    pub(in crate::slot) fn len(&self) -> usize {
        self.order.len()
    }

    pub(in crate::slot) fn get(&self, index: usize) -> Option<&PayloadRecord> {
        let slot = *self.order.get(index)?;
        self.slots.get(slot as usize)?.as_ref()
    }

    pub(in crate::slot) fn get_mut(&mut self, index: usize) -> Option<&mut PayloadRecord> {
        let slot = *self.order.get(index)?;
        self.slots.get_mut(slot as usize)?.as_mut()
    }

    pub(in crate::slot) fn range(
        &self,
        range: Range<usize>,
    ) -> impl Iterator<Item = &PayloadRecord> + '_ {
        self.order
            .get(range)
            .unwrap_or_default()
            .iter()
            .filter_map(|&slot| self.slots.get(slot as usize)?.as_ref())
    }

    pub(in crate::slot) fn iter(&self) -> impl Iterator<Item = &PayloadRecord> + '_ {
        self.range(0..self.order.len())
    }

    pub(in crate::slot) fn for_each_mut(&mut self, mut visit: impl FnMut(&mut PayloadRecord)) {
        for &slot in &self.order {
            if let Some(Some(payload)) = self.slots.get_mut(slot as usize) {
                visit(payload);
            }
        }
    }

    pub(in crate::slot) fn drain_rev(&mut self, sink: impl FnMut(PayloadRecord)) {
        let Self {
            order, slots, free, ..
        } = self;
        order
            .drain(..)
            .rev()
            .filter_map(|slot| slots.get_mut(slot as usize)?.take())
            .for_each(sink);
        slots.clear();
        free.clear();
    }

    pub(in crate::slot) fn compact(&mut self) {
        if !self.free.is_empty() {
            let mut dense = Vec::with_capacity(self.order.len());
            let Self { order, slots, .. } = self;
            order.retain_mut(|slot| {
                let Some(payload) = slots.get_mut(*slot as usize).and_then(Option::take) else {
                    log::error!("payload store dropped vacant slot {slot} during compaction");
                    return false;
                };
                *slot = checked_usize_to_u32(dense.len(), "payload slot");
                dense.push(Some(payload));
                true
            });
            self.slots = dense;
            self.free = Vec::new();
        }
        self.order.shrink_to_fit();
        self.slots.shrink_to_fit();
        self.free.shrink_to_fit();
    }

    pub(in crate::slot) fn capacity(&self) -> usize {
        self.slots.capacity()
    }

    pub(in crate::slot) fn heap_bytes(&self) -> usize {
        (self.order.capacity() + self.free.capacity()) * ORDER_ENTRY_BYTES
            + self.slots.capacity() * mem::size_of::<Option<PayloadRecord>>()
    }

    pub(in crate::slot) fn shift_bytes(&self) -> usize {
        self.shift_bytes
    }

    fn note_shift(&mut self, entries: usize) {
        self.shift_bytes = self
            .shift_bytes
            .saturating_add(entries.saturating_mul(ORDER_ENTRY_BYTES));
    }

    fn occupy(
        slots: &mut Vec<Option<PayloadRecord>>,
        free: &mut Vec<u32>,
        payload: PayloadRecord,
    ) -> u32 {
        while let Some(slot) = free.pop() {
            match slots.get_mut(slot as usize) {
                Some(entry @ None) => {
                    *entry = Some(payload);
                    return slot;
                }
                _ => log::error!("payload store skipped unusable free slot {slot}"),
            }
        }
        let slot = checked_usize_to_u32(slots.len(), "payload slot");
        slots.push(Some(payload));
        slot
    }

    fn vacate(
        slots: &mut [Option<PayloadRecord>],
        free: &mut Vec<u32>,
        slot: u32,
    ) -> Option<PayloadRecord> {
        let Some(payload) = slots.get_mut(slot as usize).and_then(Option::take) else {
            log::error!("payload store ignored removal of vacant slot {slot}");
            return None;
        };
        free.push(slot);
        Some(payload)
    }

    #[cfg(any(test, debug_assertions))]
    pub(in crate::slot) fn validate_integrity(&self) -> Result<(), SlotInvariantError> {
        let mut claimed = vec![false; self.slots.len()];
        for (index, &slot) in self.order.iter().enumerate() {
            let slot = slot as usize;
            let Some(Some(_)) = self.slots.get(slot) else {
                return Err(Self::mismatch(
                    "payload order entry must name an occupied slot",
                    index,
                    slot,
                ));
            };
            if mem::replace(&mut claimed[slot], true) {
                return Err(Self::mismatch(
                    "payload slot must appear once in the order",
                    index,
                    slot,
                ));
            }
        }
        for (index, &slot) in self.free.iter().enumerate() {
            let slot = slot as usize;
            let Some(None) = self.slots.get(slot) else {
                return Err(Self::mismatch(
                    "free payload slot must be vacant",
                    index,
                    slot,
                ));
            };
            if mem::replace(&mut claimed[slot], true) {
                return Err(Self::mismatch(
                    "free payload slot must appear once",
                    index,
                    slot,
                ));
            }
        }
        let claimed_len = self.order.len() + self.free.len();
        if claimed_len != self.slots.len() {
            return Err(Self::mismatch(
                "every payload slot must be ordered or free",
                self.slots.len(),
                claimed_len,
            ));
        }
        Ok(())
    }

    #[cfg(any(test, debug_assertions))]
    fn mismatch(detail: &'static str, expected: usize, actual: usize) -> SlotInvariantError {
        SlotInvariantError::PayloadStoreMismatch {
            detail,
            expected,
            actual,
        }
    }
}

impl GrowthSlack for PayloadStore {
    fn trim_growth_slack(&mut self) {
        self.order.trim_growth_slack();
        self.slots.trim_growth_slack();
    }
}

impl SegmentItems for PayloadStore {
    type Item = PayloadRecord;

    fn item_count(&self) -> usize {
        self.order.len()
    }

    #[cfg(any(test, debug_assertions))]
    fn item(&self, index: usize) -> Option<&PayloadRecord> {
        self.get(index)
    }

    fn insert_item(&mut self, index: usize, payload: PayloadRecord) {
        let slot = Self::occupy(&mut self.slots, &mut self.free, payload);
        self.note_shift(self.order.len().saturating_sub(index));
        self.order.insert(index, slot);
    }

    fn remove_items(&mut self, range: Range<usize>) -> Vec<PayloadRecord> {
        self.note_shift(self.order.len().saturating_sub(range.end));
        let Self {
            order, slots, free, ..
        } = self;
        order
            .drain(range)
            .filter_map(|slot| Self::vacate(slots, free, slot))
            .collect()
    }

    fn insert_items(&mut self, index: usize, payloads: Vec<PayloadRecord>) {
        self.note_shift(self.order.len().saturating_sub(index));
        let Self {
            order, slots, free, ..
        } = self;
        order.splice(
            index..index,
            payloads
                .into_iter()
                .map(|payload| Self::occupy(slots, free, payload)),
        );
    }

    fn rotate_items_right(&mut self, range: Range<usize>, count: usize) {
        let Some(entries) = self.order.get_mut(range) else {
            log::error!(
                "payload store ignored rotation outside {} entries",
                self.order.len()
            );
            return;
        };
        let moved = entries.len();
        entries.rotate_right(count);
        self.note_shift(moved);
    }
}

#[cfg(test)]
#[path = "tests/payload_store_tests.rs"]
mod tests;
