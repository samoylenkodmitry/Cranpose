use super::{GroupRecord, PayloadKind, PayloadRecord, SlotTable};
use crate::{retention::RetentionManager, AnchorId};
use std::any::TypeId;
use std::{mem, ops::Range};

impl SlotTable {
    fn group_payload_start_at(&self, group_index: usize) -> usize {
        self.groups[group_index].payload_start as usize
    }

    pub(super) fn group_payload_len_at(&self, group_index: usize) -> usize {
        self.groups[group_index].payload_len as usize
    }

    pub(in crate::slot) fn group_payload_range_checked(
        &self,
        group_index: usize,
    ) -> Option<Range<usize>> {
        let start = self.group_payload_start_at(group_index);
        let len = self.group_payload_len_at(group_index);
        let end = start.checked_add(len)?;
        (end <= self.payloads.len()).then_some(start..end)
    }

    fn group_payload_range_at(&self, group_index: usize) -> Range<usize> {
        self.group_payload_range_checked(group_index)
            .expect("payload range should resolve")
    }

    fn payload_insert_index_for_group(&self, group_index: usize) -> usize {
        if group_index < self.groups.len() {
            self.group_payload_start_at(group_index)
        } else {
            self.payloads.len()
        }
    }

    fn apply_payload_start_delta(payload_start: &mut u32, delta: i64) {
        let updated = (*payload_start as i64) + delta;
        debug_assert!(updated >= 0, "payload start cannot become negative");
        *payload_start = updated as u32;
    }

    fn shift_payload_starts_from(&mut self, start_group_index: usize, delta: i64) {
        if delta == 0 {
            return;
        }
        for group in &mut self.groups[start_group_index..] {
            Self::apply_payload_start_delta(&mut group.payload_start, delta);
        }
    }

    fn offset_detached_group_payload_starts(groups: &mut [GroupRecord], delta: i64) {
        if delta == 0 {
            return;
        }
        for group in groups {
            Self::apply_payload_start_delta(&mut group.payload_start, delta);
        }
    }

    fn subtree_payload_span(groups: &[GroupRecord]) -> Option<(usize, usize)> {
        let payload_start = groups.first()?.payload_start as usize;
        let payload_len = groups
            .iter()
            .map(|group| group.payload_len as usize)
            .sum::<usize>();
        Some((payload_start, payload_len))
    }

    pub(super) fn allocate_payload_anchor(&mut self) -> usize {
        let anchor = self.next_payload_anchor;
        self.next_payload_anchor += 1;
        anchor
    }

    pub(in crate::slot) fn group_payload_records_at(&self, group_index: usize) -> &[PayloadRecord] {
        let range = self.group_payload_range_at(group_index);
        &self.payloads[range]
    }

    pub(super) fn payload_owner_at(&self, group_index: usize, payload_index: usize) -> AnchorId {
        self.group_payload_record_at(group_index, payload_index)
            .owner
    }

    pub(super) fn payload_anchor_at(&self, group_index: usize, payload_index: usize) -> usize {
        self.group_payload_record_at(group_index, payload_index)
            .anchor
    }

    pub(super) fn payload_generation_at(&self, group_index: usize, payload_index: usize) -> u32 {
        self.group_payload_record_at(group_index, payload_index)
            .generation
    }

    pub(super) fn payload_value_is<T: 'static>(
        &self,
        group_index: usize,
        payload_index: usize,
    ) -> bool {
        self.group_payload_record_at(group_index, payload_index)
            .type_id
            == TypeId::of::<T>()
    }

    pub(in crate::slot) fn group_payload_record_at(
        &self,
        group_index: usize,
        payload_index: usize,
    ) -> &PayloadRecord {
        self.group_payload_records_at(group_index)
            .get(payload_index)
            .expect("payload index should resolve")
    }

    pub(super) fn payload_slot_identity_at(
        &self,
        group_index: usize,
        payload_index: usize,
    ) -> (usize, u32) {
        (
            self.payload_anchor_at(group_index, payload_index),
            self.payload_generation_at(group_index, payload_index),
        )
    }

    pub(in crate::slot) fn group_payload_record_at_mut(
        &mut self,
        group_index: usize,
        payload_index: usize,
    ) -> &mut PayloadRecord {
        let payload_start = self.group_payload_start_at(group_index);
        self.payloads
            .get_mut(payload_start + payload_index)
            .expect("payload index should resolve")
    }

    pub(in crate::slot) fn total_payload_count(&self) -> usize {
        self.payloads.len()
    }

    pub(super) fn payload_heap_bytes(&self) -> usize {
        self.payloads.capacity() * mem::size_of::<PayloadRecord>()
    }

    pub(super) fn payload_debug_capacity(&self) -> usize {
        self.payloads.capacity()
    }

    pub(super) fn insert_value_payload<T: 'static>(
        &mut self,
        owner: AnchorId,
        insert_index: usize,
        generation: u32,
        kind: PayloadKind,
        value: T,
    ) -> usize {
        let owner_index = self.current_group_index(owner);
        let payload_index = self.group_payload_start_at(owner_index) + insert_index;
        let anchor = self.allocate_payload_anchor();
        self.payloads.insert(
            payload_index,
            PayloadRecord {
                owner,
                anchor,
                generation,
                type_id: TypeId::of::<T>(),
                type_name: std::any::type_name::<T>(),
                kind,
                value: Box::new(value),
            },
        );
        self.groups[owner_index].payload_len += 1;
        self.shift_payload_starts_from(owner_index + 1, 1);
        self.refresh_group_payload_locations(owner, insert_index);
        anchor
    }

    pub(super) fn replace_payload_value<T: 'static>(
        &mut self,
        group_index: usize,
        payload_index: usize,
        kind: PayloadKind,
        value: T,
    ) -> (PayloadKind, Box<dyn std::any::Any>) {
        let record = self.group_payload_record_at_mut(group_index, payload_index);
        let old_value = mem::replace(&mut record.value, Box::new(value));
        let old_kind = record.kind;
        record.type_id = TypeId::of::<T>();
        record.type_name = std::any::type_name::<T>();
        record.kind = kind;
        (old_kind, old_value)
    }

    pub(super) fn clear_payload_locations_for_payloads(&mut self, payloads: &[PayloadRecord]) {
        for payload in payloads {
            self.payload_locations.remove(payload.anchor);
        }
    }

    fn refresh_group_payload_locations(&mut self, owner: AnchorId, start: usize) {
        let group_index = self.current_group_index(owner);
        let range = self.group_payload_range_at(group_index);
        for index in start..(range.end - range.start) {
            let payload_anchor = self.payloads[range.start + index].anchor;
            self.payload_locations.insert(payload_anchor, owner, index);
        }
    }

    pub(super) fn rebuild_payload_locations_for_group_range(&mut self, start: usize, end: usize) {
        let group_span = end.saturating_sub(start);
        let mut payload_span = 0usize;
        for group_index in start..end {
            let owner = self.groups[group_index].anchor;
            let range = self.group_payload_range_at(group_index);
            payload_span += range.end - range.start;
            for index in 0..(range.end - range.start) {
                let payload_anchor = self.payloads[range.start + index].anchor;
                self.payload_locations.insert(payload_anchor, owner, index);
            }
        }
        self.mutation_debug_stats
            .record_payload_location_rebuild(group_span, payload_span);
    }

    pub(super) fn remove_payload_range(
        &mut self,
        owner: AnchorId,
        start: usize,
        end: usize,
    ) -> Vec<PayloadRecord> {
        if start >= end {
            return Vec::new();
        }
        let owner_index = self.current_group_index(owner);
        let payload_start = self.group_payload_start_at(owner_index) + start;
        let payload_end = self.group_payload_start_at(owner_index) + end;
        let removed = self
            .payloads
            .drain(payload_start..payload_end)
            .collect::<Vec<_>>();
        self.clear_payload_locations_for_payloads(&removed);
        self.groups[owner_index].payload_len -= removed.len() as u32;
        self.shift_payload_starts_from(owner_index + 1, -(removed.len() as i64));
        self.refresh_group_payload_locations(owner, start);
        removed
    }

    pub(super) fn detach_payloads_for_groups(
        &mut self,
        removed_group_index: usize,
        removed_groups: &mut [GroupRecord],
    ) -> Vec<PayloadRecord> {
        let Some((payload_start, payload_len)) = Self::subtree_payload_span(removed_groups) else {
            return Vec::new();
        };
        Self::offset_detached_group_payload_starts(removed_groups, -(payload_start as i64));
        if payload_len == 0 {
            return Vec::new();
        }
        let removed = self
            .payloads
            .drain(payload_start..payload_start + payload_len)
            .collect::<Vec<_>>();
        self.clear_payload_locations_for_payloads(&removed);
        self.shift_payload_starts_from(removed_group_index, -(payload_len as i64));
        removed
    }

    pub(super) fn restore_payloads_for_groups(
        &mut self,
        insert_group_index: usize,
        groups: &mut [GroupRecord],
        payloads: Vec<PayloadRecord>,
    ) {
        let payload_insert_index = self.payload_insert_index_for_group(insert_group_index);
        self.shift_payload_starts_from(insert_group_index, payloads.len() as i64);
        Self::offset_detached_group_payload_starts(groups, payload_insert_index as i64);
        self.payloads
            .splice(payload_insert_index..payload_insert_index, payloads);
    }

    pub(crate) fn compact_payload_anchor_namespace(
        &mut self,
        retention: Option<&mut RetentionManager>,
    ) {
        let retained_payload_count = retention
            .as_ref()
            .map(|retention| {
                retention
                    .subtrees()
                    .map(|subtree| subtree.payloads.len())
                    .sum::<usize>()
            })
            .unwrap_or(0);
        let total_payload_count = self.payloads.len() + retained_payload_count;
        if total_payload_count == 0 {
            self.payload_locations.clear();
            self.payload_locations.shrink_to_fit();
            self.next_payload_anchor = 1;
            return;
        }

        let max_payload_anchor = self
            .payloads
            .iter()
            .map(|payload| payload.anchor)
            .chain(
                retention
                    .as_ref()
                    .into_iter()
                    .flat_map(|retention| retention.subtrees())
                    .flat_map(|subtree| subtree.payloads.iter().map(|payload| payload.anchor)),
            )
            .max()
            .unwrap_or(0);
        let sparse_namespace = max_payload_anchor > total_payload_count.max(256) * 4;
        let sparse_capacity = self.payload_locations.capacity() > total_payload_count.max(256) * 8;
        if !sparse_namespace && !sparse_capacity {
            return;
        }

        let mut remapped = crate::collections::map::HashMap::default();
        let mut next_anchor = 1usize;
        for payload in &self.payloads {
            remapped.insert(payload.anchor, next_anchor);
            next_anchor += 1;
        }
        if let Some(retention) = retention.as_ref() {
            for subtree in retention.subtrees() {
                for payload in &subtree.payloads {
                    remapped.insert(payload.anchor, next_anchor);
                    next_anchor += 1;
                }
            }
        }

        for payload in &mut self.payloads {
            payload.anchor = remapped
                .get(&payload.anchor)
                .copied()
                .expect("active payload anchor must remap");
        }
        if let Some(retention) = retention {
            for subtree in retention.subtrees_mut() {
                for payload in &mut subtree.payloads {
                    payload.anchor = remapped
                        .get(&payload.anchor)
                        .copied()
                        .expect("retained payload anchor must remap");
                }
            }
        }

        self.payload_locations.clear();
        self.payload_locations.shrink_to_fit();
        self.rebuild_payload_locations_for_group_range(0, self.groups.len());
        self.next_payload_anchor = total_payload_count + 1;
    }
}
