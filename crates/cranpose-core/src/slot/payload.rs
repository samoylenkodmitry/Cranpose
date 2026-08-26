use std::{any::TypeId, mem};

use super::{
    segments::{
        extract_subtree_segment, group_segment_len, group_segment_range_checked,
        group_segment_start, group_segment_subrange_at, insert_group_segment_item,
        move_subtree_segment_to_earlier_group, remove_group_segment_range,
        repair_group_segment_start_and_len_to_storage, restore_subtree_segment, PayloadSegment,
    },
    DeferredDrop, GroupPayloadRange, GroupRange, GroupRecord, PayloadAnchor, PayloadKind,
    PayloadRange, PayloadRecord, SlotTable, SlotWriteSessionState, ValueSlotId,
};
use crate::{retention::RetentionManager, AnchorId};

#[derive(Clone, Copy)]
pub(in crate::slot) struct PayloadLocationRefresh {
    pub(in crate::slot) owner: AnchorId,
    pub(in crate::slot) start: usize,
}

/// Monomorphic payload initializer: carries the value type's identity plus a
/// one-shot factory, so the slot-table payload machinery is compiled once
/// instead of once per remembered value type.
pub(in crate::slot) struct PayloadInit<'a> {
    type_id: TypeId,
    type_name: &'static str,
    source: crate::Key,
    make: &'a mut dyn FnMut() -> Box<dyn std::any::Any>,
    fresh: Option<fn() -> Box<dyn std::any::Any>>,
}

impl<'a> PayloadInit<'a> {
    pub(in crate::slot) fn new<T: 'static>(
        source: crate::Key,
        make: &'a mut dyn FnMut() -> Box<dyn std::any::Any>,
    ) -> Self {
        Self {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            source,
            make,
            fresh: None,
        }
    }

    pub(in crate::slot) fn new_startable<T: 'static>(
        source: crate::Key,
        make: &'a mut dyn FnMut() -> Box<dyn std::any::Any>,
        fresh: fn() -> Box<dyn std::any::Any>,
    ) -> Self {
        Self {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            source,
            make,
            fresh: Some(fresh),
        }
    }

    pub(in crate::slot) fn mix_source(&mut self, fold: crate::Key) {
        if fold != super::BRANCH_PATH_ROOT {
            self.source = (fold ^ self.source).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn make_value(&mut self) -> Box<dyn std::any::Any> {
        (self.make)()
    }
}

fn replace_payload_record(
    record: &mut PayloadRecord,
    kind: PayloadKind,
    init: &mut PayloadInit<'_>,
) -> Box<dyn std::any::Any> {
    let old_value = mem::replace(&mut record.value, init.make_value());
    record.type_id = init.type_id;
    record.type_name = init.type_name;
    record.source = init.source;
    record.kind = kind;
    record.fresh = init.fresh;
    old_value
}

impl SlotTable {
    fn group_payload_start_at(&self, group_index: usize) -> usize {
        group_segment_start::<PayloadSegment>(&self.groups, group_index)
    }

    pub(super) fn group_payload_len_at(&self, group_index: usize) -> usize {
        group_segment_len::<PayloadSegment>(&self.groups, group_index)
    }

    fn group_payload_range_checked_at(&self, group_index: usize) -> Option<PayloadRange> {
        group_segment_range_checked::<PayloadSegment>(
            &self.groups,
            self.payloads.len(),
            group_index,
        )
    }

    fn repair_group_payload_len_to_storage(
        &mut self,
        group_index: usize,
        operation: &'static str,
    ) -> usize {
        let repair = repair_group_segment_start_and_len_to_storage::<PayloadSegment>(
            &mut self.groups,
            self.payloads.len(),
            group_index,
            operation,
        );
        if repair.repaired {
            self.record_segment_range_update_from(group_index);
        }
        repair.len
    }

    pub(in crate::slot) fn group_payload_subrange_at(
        &self,
        group_index: usize,
        start: usize,
        end: usize,
    ) -> GroupPayloadRange {
        GroupPayloadRange::from_range(
            group_segment_subrange_at::<PayloadSegment>(
                &self.groups,
                self.payloads.len(),
                group_index,
                start,
                end,
            ),
            start,
        )
    }

    fn insert_group_payload(
        &mut self,
        group_index: usize,
        payload_index: usize,
        payload: PayloadRecord,
    ) {
        // Payload insertion/removal updates segment starts before touching the
        // payload vector so every following group keeps a coherent range.
        self.record_segment_range_update_from(group_index);
        insert_group_segment_item::<PayloadSegment, _>(
            &mut self.groups,
            &mut self.payloads,
            group_index,
            payload_index,
            payload,
        );
    }

    fn remove_group_payload_range(
        &mut self,
        payload_range: GroupPayloadRange,
    ) -> Vec<PayloadRecord> {
        // Removed payloads are invalidated by the caller after the segment is
        // extracted, then surviving anchors are refreshed from the removed start.
        if !payload_range.is_empty() {
            self.record_segment_range_update_from(payload_range.group_index());
        }
        remove_group_segment_range::<PayloadSegment, _>(
            &mut self.groups,
            &mut self.payloads,
            payload_range.into_inner(),
        )
    }

    fn extract_payload_segment_for_groups(
        &mut self,
        removed_group_index: usize,
        removed_groups: &mut [GroupRecord],
    ) -> Vec<PayloadRecord> {
        if removed_groups.iter().any(|group| group.payload_len > 0) {
            let active_span = self.groups.len().saturating_sub(removed_group_index);
            self.record_segment_range_update_span(active_span + removed_groups.len());
        }
        extract_subtree_segment::<PayloadSegment, _>(
            &mut self.groups,
            &mut self.payloads,
            removed_group_index,
            removed_groups,
        )
    }

    fn restore_payload_segment_for_groups(
        &mut self,
        insert_group_index: usize,
        groups: &mut [GroupRecord],
        payloads: Vec<PayloadRecord>,
    ) {
        if !payloads.is_empty() {
            let active_span = self.groups.len().saturating_sub(insert_group_index);
            self.record_segment_range_update_span(active_span + groups.len());
        }
        restore_subtree_segment::<PayloadSegment, _>(
            &mut self.groups,
            &mut self.payloads,
            insert_group_index,
            groups,
            payloads,
        );
    }

    pub(super) fn allocate_payload_anchor(&mut self) -> Option<PayloadAnchor> {
        self.payload_anchors.try_allocate()
    }

    pub(in crate::slot) fn group_payload_records_at(&self, group_index: usize) -> &[PayloadRecord] {
        let Some(range) = self.group_payload_range_checked_at(group_index) else {
            log::error!(
                "slot table ignored payload record read for corrupt payload segment at group index {group_index}"
            );
            return &[];
        };
        &self.payloads[range.as_range()]
    }

    pub(super) fn payload_owner_at(&self, group_index: usize, payload_index: usize) -> AnchorId {
        self.group_payload_record_at(group_index, payload_index)
            .owner
    }

    pub(super) fn payload_anchor_at(
        &self,
        group_index: usize,
        payload_index: usize,
    ) -> PayloadAnchor {
        self.group_payload_record_at(group_index, payload_index)
            .anchor
    }

    fn payload_value_type_matches(
        &self,
        group_index: usize,
        payload_index: usize,
        type_id: TypeId,
    ) -> bool {
        self.group_payload_record_at(group_index, payload_index)
            .type_id
            == type_id
    }

    fn payload_value_source_matches(
        &self,
        group_index: usize,
        payload_index: usize,
        source: crate::Key,
    ) -> bool {
        self.group_payload_record_at(group_index, payload_index)
            .source
            == source
    }

    fn find_matching_payload_from(
        &self,
        group_index: usize,
        from_index: usize,
        payload_len: usize,
        type_id: TypeId,
        source: crate::Key,
    ) -> Option<usize> {
        let records = self.group_payload_records_at(group_index);
        (from_index..payload_len)
            .find(|&index| records[index].type_id == type_id && records[index].source == source)
    }

    fn rotate_payload_record_to_cursor(
        &mut self,
        group_index: usize,
        found_index: usize,
        cursor_index: usize,
    ) {
        let start = self.group_payload_start_at(group_index);
        self.payloads[start + cursor_index..=start + found_index].rotate_right(1);
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

    fn payload_slot_identity_at(&self, group_index: usize, payload_index: usize) -> PayloadAnchor {
        self.payload_anchor_at(group_index, payload_index)
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

    fn insert_value_payload_internal(
        &mut self,
        owner: AnchorId,
        owner_index: usize,
        insert_index: usize,
        kind: PayloadKind,
        init: &mut PayloadInit<'_>,
        refresh_index: bool,
    ) -> Option<PayloadAnchor> {
        let Some(anchor) = self.allocate_payload_anchor() else {
            log::error!(
                "slot table rejected value payload insertion because payload anchor ids are exhausted"
            );
            return None;
        };
        self.insert_group_payload(
            owner_index,
            insert_index,
            PayloadRecord {
                owner,
                anchor,
                type_id: init.type_id,
                type_name: init.type_name,
                source: init.source,
                kind,
                value: init.make_value(),
                fresh: init.fresh,
            },
        );
        if refresh_index {
            self.refresh_group_payload_anchor_locations(owner, insert_index);
        } else {
            self.set_group_payload_anchor_active_location(owner, insert_index);
        }
        Some(anchor)
    }

    #[cfg(test)]
    pub(super) fn insert_value_payload<T: 'static>(
        &mut self,
        owner: AnchorId,
        insert_index: usize,
        kind: PayloadKind,
        value: T,
    ) -> PayloadAnchor {
        let source = super::BRANCH_PATH_ROOT;
        let owner_index = self
            .active_group_index(owner)
            .expect("test payload owner should resolve");
        let mut value = Some(value);
        let mut make =
            move || -> Box<dyn std::any::Any> { Box::new(value.take().expect("value once")) };
        let mut init = PayloadInit::new::<T>(source, &mut make);
        self.insert_value_payload_internal(owner, owner_index, insert_index, kind, &mut init, true)
            .unwrap_or(PayloadAnchor::INVALID)
    }

    #[cfg(test)]
    pub(super) fn replace_payload_value<T: 'static>(
        &mut self,
        group_index: usize,
        payload_index: usize,
        kind: PayloadKind,
        value: T,
    ) -> Box<dyn std::any::Any> {
        let source = super::BRANCH_PATH_ROOT;
        let mut value = Some(value);
        let mut make =
            move || -> Box<dyn std::any::Any> { Box::new(value.take().expect("value once")) };
        let mut init = PayloadInit::new::<T>(source, &mut make);
        let record = self.group_payload_record_at_mut(group_index, payload_index);
        replace_payload_record(record, kind, &mut init)
    }

    fn update_payload_kind(&mut self, group_index: usize, payload_index: usize, kind: PayloadKind) {
        self.group_payload_record_at_mut(group_index, payload_index)
            .kind = kind;
    }

    fn replace_payload_identity(
        &mut self,
        group_index: usize,
        payload_index: usize,
        kind: PayloadKind,
        init: &mut PayloadInit<'_>,
    ) -> Option<(PayloadAnchor, Box<dyn std::any::Any>)> {
        let old_anchor = self.payload_anchor_at(group_index, payload_index);
        let anchor = match self.payload_anchors.bump_generation(old_anchor) {
            Some(anchor) => anchor,
            None if self.payload_anchors.active_location(old_anchor).is_none() => {
                log::error!(
                    "slot table replaced stale payload anchor record {old_anchor:?} with a fresh anchor"
                );
                self.allocate_payload_anchor()
                    .unwrap_or(PayloadAnchor::INVALID)
            }
            None => {
                log::error!(
                    "slot table rejected value payload replacement because active payload anchor {old_anchor:?} could not advance generation"
                );
                return None;
            }
        };
        if anchor == PayloadAnchor::INVALID {
            log::error!(
                "slot table rejected value payload replacement because payload anchor ids are exhausted"
            );
            return None;
        }
        let owner = self.payload_owner_at(group_index, payload_index);
        let record = self.group_payload_record_at_mut(group_index, payload_index);
        record.anchor = anchor;
        let old_value = replace_payload_record(record, kind, init);
        self.payload_anchors
            .set_active(anchor, owner, payload_index);
        Some((anchor, old_value))
    }

    pub(super) fn use_value_payload_at_cursor(
        &mut self,
        owner: AnchorId,
        group_index: usize,
        payload_index: usize,
        kind: PayloadKind,
        init: &mut PayloadInit<'_>,
    ) -> (
        ValueSlotId,
        Option<DeferredDrop>,
        Option<PayloadLocationRefresh>,
    ) {
        let payload_len =
            self.repair_group_payload_len_to_storage(group_index, "value payload cursor");
        let payload_index = if payload_index > payload_len {
            log::error!(
                "slot table clamped payload cursor {payload_index} to repaired payload length {payload_len} for owner {owner:?}"
            );
            payload_len
        } else {
            payload_index
        };
        let mut location_refresh = None;

        let (anchor, deferred_drop) = if payload_index < payload_len {
            if self.payload_value_type_matches(group_index, payload_index, init.type_id)
                && self.payload_value_source_matches(group_index, payload_index, init.source)
            {
                let anchor = self.payload_slot_identity_at(group_index, payload_index);
                self.update_payload_kind(group_index, payload_index, kind);
                (anchor, None)
            } else if let Some(found) = self.find_matching_payload_from(
                group_index,
                payload_index + 1,
                payload_len,
                init.type_id,
                init.source,
            ) {
                self.rotate_payload_record_to_cursor(group_index, found, payload_index);
                self.refresh_group_payload_anchor_locations(owner, payload_index);
                let anchor = self.payload_slot_identity_at(group_index, payload_index);
                self.update_payload_kind(group_index, payload_index, kind);
                (anchor, None)
            } else {
                match self.replace_payload_identity(group_index, payload_index, kind, init) {
                    Some((anchor, old_value)) => (anchor, Some(DeferredDrop::payload(old_value))),
                    None => (PayloadAnchor::INVALID, None),
                }
            }
        } else {
            let Some(anchor) = self.insert_value_payload_internal(
                owner,
                group_index,
                payload_index,
                kind,
                init,
                false,
            ) else {
                return (
                    ValueSlotId::new_for_table(PayloadAnchor::INVALID, self.storage_id()),
                    None,
                    None,
                );
            };
            location_refresh = Some(PayloadLocationRefresh {
                owner,
                start: payload_index,
            });
            (anchor, None)
        };

        (
            ValueSlotId::new_for_table(anchor, self.storage_id()),
            deferred_drop,
            location_refresh,
        )
    }

    fn set_group_payload_anchor_active_location(&mut self, owner: AnchorId, index: usize) {
        let Some(group_index) = self.active_group_index(owner) else {
            log::error!(
                "slot table ignored payload-location activation for stale owner anchor {owner:?}"
            );
            return;
        };
        let Some(range) = self.group_payload_range_checked_at(group_index) else {
            log::error!(
                "slot table ignored payload-location activation for corrupt payload segment at group index {group_index}"
            );
            return;
        };
        let Some(payload_index) = range.start().checked_add(index) else {
            log::error!(
                "slot table ignored payload-location activation for overflowing payload index {index} in group index {group_index}"
            );
            return;
        };
        let Some(payload_record) = self.payloads.get(payload_index) else {
            log::error!(
                "slot table ignored payload-location activation for missing payload index {index} in group index {group_index}"
            );
            return;
        };
        let payload_anchor = payload_record.anchor;
        self.payload_anchors
            .set_active(payload_anchor, owner, index);
    }

    pub(in crate::slot) fn refresh_group_payload_anchor_locations(
        &mut self,
        owner: AnchorId,
        start: usize,
    ) {
        let Some(group_index) = self.active_group_index(owner) else {
            log::error!(
                "slot table ignored payload-location refresh for stale owner anchor {owner:?}"
            );
            return;
        };
        let Some(range) = self.group_payload_range_checked_at(group_index) else {
            log::error!(
                "slot table ignored payload-location refresh for corrupt payload segment at group index {group_index}"
            );
            return;
        };
        let payload_span = range.len().saturating_sub(start);
        if payload_span == 0 {
            return;
        }
        for index in start..range.len() {
            let payload_anchor = self.payloads[range.start() + index].anchor;
            self.payload_anchors
                .set_active(payload_anchor, owner, index);
        }
        self.diagnostics
            .record_payload_location_refresh(payload_span);
    }

    pub(in crate::slot) fn flush_payload_location_refreshes(
        &mut self,
        state: &mut SlotWriteSessionState,
    ) {
        let refreshes = state.drain_payload_location_refreshes().collect::<Vec<_>>();
        for (owner, start) in refreshes {
            self.refresh_group_payload_anchor_locations(owner, start);
        }
        #[cfg(any(test, debug_assertions))]
        state.debug_assert_no_pending_payload_location_refreshes("slot table payload flush");
    }

    pub(super) fn refresh_payload_locations_for_group_range(&mut self, group_range: GroupRange) {
        let group_span = group_range.len();
        let mut payload_span = 0usize;
        for group_index in group_range.as_range() {
            let Some(group) = self.groups.get(group_index) else {
                log::error!(
                    "slot table ignored payload-location range refresh for missing group index {group_index}"
                );
                continue;
            };
            let owner = group.anchor;
            let Some(range) = self.group_payload_range_checked_at(group_index) else {
                log::error!(
                    "slot table ignored payload-location range refresh for corrupt payload segment at group index {group_index}"
                );
                continue;
            };
            payload_span += range.len();
            for index in 0..range.len() {
                let payload_anchor = self.payloads[range.start() + index].anchor;
                self.payload_anchors
                    .set_active(payload_anchor, owner, index);
            }
        }
        self.diagnostics
            .record_payload_location_range_refresh(group_span, payload_span);
    }

    pub(super) fn remove_payload_range(
        &mut self,
        owner: AnchorId,
        payload_range: GroupPayloadRange,
    ) -> Vec<PayloadRecord> {
        let Some(owner_index) = self.active_group_index(owner) else {
            log::error!("slot table ignored payload removal for stale owner anchor {owner:?}");
            return Vec::new();
        };
        if payload_range.group_index() != owner_index {
            log::error!(
                "slot table ignored payload removal for owner {owner:?}: range group index {} does not match owner group index {owner_index}",
                payload_range.group_index()
            );
            return Vec::new();
        }
        if payload_range.is_empty() {
            return Vec::new();
        }
        let Some(current_range) = self.group_payload_range_checked_at(owner_index) else {
            log::error!(
                "slot table ignored payload removal for corrupt payload segment at group index {owner_index}"
            );
            return Vec::new();
        };
        let requested_range = payload_range.into_inner().as_range();
        let current_range = current_range.as_range();
        if requested_range.start < current_range.start || requested_range.end > current_range.end {
            log::error!(
                "slot table ignored payload removal for owner {owner:?}: requested range {:?} is outside current group payload range {:?}",
                requested_range,
                current_range
            );
            return Vec::new();
        }
        let start_offset = payload_range.start_offset();
        let removed = self.remove_group_payload_range(payload_range);
        self.invalidate_payload_anchors(&removed);
        self.refresh_group_payload_anchor_locations(owner, start_offset);
        removed
    }

    pub(super) fn remove_payload_tail_at_cursor(
        &mut self,
        owner: AnchorId,
        payload_cursor: usize,
    ) -> Vec<PayloadRecord> {
        let Some(owner_index) = self.active_group_index(owner) else {
            log::error!("slot table ignored payload-tail removal for stale owner anchor {owner:?}");
            return Vec::new();
        };
        let payload_len =
            self.repair_group_payload_len_to_storage(owner_index, "payload tail cleanup");
        if payload_cursor >= payload_len {
            return Vec::new();
        }

        let payload_range =
            self.group_payload_subrange_at(owner_index, payload_cursor, payload_len);
        self.remove_payload_range(owner, payload_range)
    }

    fn extract_payloads_for_groups(
        &mut self,
        removed_group_index: usize,
        removed_groups: &mut [GroupRecord],
        clear_locations: bool,
    ) -> Vec<PayloadRecord> {
        let removed = self.extract_payload_segment_for_groups(removed_group_index, removed_groups);
        if clear_locations {
            self.mark_payload_anchors_detached(&removed);
        }
        removed
    }

    pub(super) fn detach_payloads_for_groups(
        &mut self,
        removed_group_index: usize,
        removed_groups: &mut [GroupRecord],
    ) -> Vec<PayloadRecord> {
        self.extract_payloads_for_groups(removed_group_index, removed_groups, true)
    }

    pub(super) fn move_payloads_to_earlier_group(
        &mut self,
        insert_group_index: usize,
        moving_group_index: usize,
        moving_group_len: usize,
    ) -> usize {
        let moved_payload_count = move_subtree_segment_to_earlier_group::<PayloadSegment, _>(
            &mut self.groups,
            &mut self.payloads,
            insert_group_index,
            moving_group_index,
            moving_group_len,
        );
        if moved_payload_count > 0 {
            self.record_segment_range_update_span(
                moving_group_index + moving_group_len - insert_group_index,
            );
        }
        moved_payload_count
    }

    pub(super) fn restore_payloads_for_groups(
        &mut self,
        insert_group_index: usize,
        groups: &mut [GroupRecord],
        payloads: Vec<PayloadRecord>,
    ) {
        self.restore_payload_segment_for_groups(insert_group_index, groups, payloads);
    }

    pub(crate) fn compact_payload_anchor_registry_storage(
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
            self.payload_anchors.shrink_to_fit();
            return;
        }

        let max_payload_anchor = self
            .payloads
            .iter()
            .map(|payload| payload.anchor.id())
            .chain(
                retention
                    .as_ref()
                    .into_iter()
                    .flat_map(|retention| retention.subtrees())
                    .flat_map(|subtree| subtree.payloads.iter().map(|payload| payload.anchor.id())),
            )
            .max()
            .unwrap_or(0);
        let sparse_payload_anchor_ids = max_payload_anchor > total_payload_count.max(256) * 4;
        let sparse_capacity = self.payload_anchors.capacity() > total_payload_count.max(256) * 8;
        if !sparse_payload_anchor_ids && !sparse_capacity {
            return;
        }

        self.payload_anchors.shrink_to_fit();
        self.refresh_payload_locations_for_group_range(GroupRange::new(0, self.groups.len()));
    }
}
