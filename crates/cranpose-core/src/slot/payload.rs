use super::{
    segments::{
        extract_subtree_segment, group_segment_len, group_segment_range_at, group_segment_start,
        group_segment_subrange_at, insert_group_segment_item, remove_group_segment_range,
        restore_subtree_segment, PayloadSegment,
    },
    DeferredDrop, GroupPayloadRange, GroupRange, GroupRecord, PayloadAnchor, PayloadKind,
    PayloadRange, PayloadRecord, SlotTable, ValueSlotId,
};
use crate::{retention::RetentionManager, AnchorId};
use std::any::TypeId;
use std::mem;

fn replace_payload_record<T: 'static>(
    record: &mut PayloadRecord,
    kind: PayloadKind,
    value: T,
) -> Box<dyn std::any::Any> {
    let old_value = mem::replace(&mut record.value, Box::new(value));
    record.type_id = TypeId::of::<T>();
    record.type_name = std::any::type_name::<T>();
    record.kind = kind;
    old_value
}

impl SlotTable {
    fn group_payload_start_at(&self, group_index: usize) -> usize {
        group_segment_start::<PayloadSegment>(&self.groups, group_index)
    }

    pub(super) fn group_payload_len_at(&self, group_index: usize) -> usize {
        group_segment_len::<PayloadSegment>(&self.groups, group_index)
    }

    fn group_payload_range_at(&self, group_index: usize) -> PayloadRange {
        group_segment_range_at::<PayloadSegment>(&self.groups, self.payloads.len(), group_index)
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
        restore_subtree_segment::<PayloadSegment, _>(
            &mut self.groups,
            &mut self.payloads,
            insert_group_index,
            groups,
            payloads,
        );
    }

    pub(super) fn allocate_payload_anchor(&mut self) -> PayloadAnchor {
        self.payload_anchors.allocate()
    }

    pub(in crate::slot) fn group_payload_records_at(&self, group_index: usize) -> &[PayloadRecord] {
        let range = self.group_payload_range_at(group_index);
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

    pub(super) fn payload_generation_at(&self, group_index: usize, payload_index: usize) -> u32 {
        self.group_payload_record_at(group_index, payload_index)
            .anchor
            .generation()
    }

    fn payload_value_is<T: 'static>(&self, group_index: usize, payload_index: usize) -> bool {
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

    pub(super) fn insert_value_payload<T: 'static>(
        &mut self,
        owner: AnchorId,
        insert_index: usize,
        kind: PayloadKind,
        value: T,
    ) -> PayloadAnchor {
        let owner_index = self.current_group_index(owner);
        let anchor = self.allocate_payload_anchor();
        self.insert_group_payload(
            owner_index,
            insert_index,
            PayloadRecord {
                owner,
                anchor,
                type_id: TypeId::of::<T>(),
                type_name: std::any::type_name::<T>(),
                kind,
                value: Box::new(value),
            },
        );
        self.refresh_group_payload_anchor_index(owner, insert_index);
        anchor
    }

    pub(super) fn replace_payload_value<T: 'static>(
        &mut self,
        group_index: usize,
        payload_index: usize,
        kind: PayloadKind,
        value: T,
    ) -> Box<dyn std::any::Any> {
        let record = self.group_payload_record_at_mut(group_index, payload_index);
        replace_payload_record(record, kind, value)
    }

    fn update_payload_kind(&mut self, group_index: usize, payload_index: usize, kind: PayloadKind) {
        self.group_payload_record_at_mut(group_index, payload_index)
            .kind = kind;
    }

    fn replace_payload_identity<T: 'static>(
        &mut self,
        group_index: usize,
        payload_index: usize,
        kind: PayloadKind,
        value: T,
    ) -> (PayloadAnchor, Box<dyn std::any::Any>) {
        let old_anchor = self.payload_anchor_at(group_index, payload_index);
        let anchor = self
            .payload_anchors
            .bump_generation(old_anchor)
            .expect("active payload anchor generation must bump");
        let record = self.group_payload_record_at_mut(group_index, payload_index);
        record.anchor = anchor;
        let old_value = replace_payload_record(record, kind, value);
        (anchor, old_value)
    }

    pub(super) fn use_value_payload_at_cursor<T: 'static>(
        &mut self,
        owner: AnchorId,
        payload_index: usize,
        kind: PayloadKind,
        init: impl FnOnce() -> T,
    ) -> (ValueSlotId, Option<DeferredDrop>) {
        let group_index = self.current_group_index(owner);
        let payload_len = self.group_payload_len_at(group_index);

        let (anchor, deferred_drop) = if payload_index < payload_len {
            let anchor = self.payload_slot_identity_at(group_index, payload_index);
            if self.payload_value_is::<T>(group_index, payload_index) {
                self.update_payload_kind(group_index, payload_index, kind);
                (anchor, None)
            } else {
                let (anchor, old_value) =
                    self.replace_payload_identity(group_index, payload_index, kind, init());
                (anchor, Some(DeferredDrop::payload(old_value)))
            }
        } else {
            let anchor = self.insert_value_payload(owner, payload_index, kind, init());
            (anchor, None)
        };

        (ValueSlotId::new(anchor), deferred_drop)
    }

    pub(super) fn clear_payload_anchor_index_for_payloads(&mut self, payloads: &[PayloadRecord]) {
        for payload in payloads {
            self.payload_anchor_index.remove(payload.anchor);
        }
    }

    fn refresh_group_payload_anchor_index(&mut self, owner: AnchorId, start: usize) {
        let group_index = self.current_group_index(owner);
        let range = self.group_payload_range_at(group_index);
        for index in start..range.len() {
            let payload_anchor = self.payloads[range.start() + index].anchor;
            self.payload_anchor_index
                .insert(payload_anchor, owner, index);
            self.payload_anchors
                .set_active(payload_anchor, owner, index);
        }
    }

    pub(super) fn rebuild_payload_anchor_index_for_group_range(&mut self, group_range: GroupRange) {
        let group_span = group_range.len();
        let mut payload_span = 0usize;
        for group_index in group_range.as_range() {
            let owner = self.groups[group_index].anchor;
            let range = self.group_payload_range_at(group_index);
            payload_span += range.len();
            for index in 0..range.len() {
                let payload_anchor = self.payloads[range.start() + index].anchor;
                self.payload_anchor_index
                    .insert(payload_anchor, owner, index);
                self.payload_anchors
                    .set_active(payload_anchor, owner, index);
            }
        }
        self.mutation_debug_stats
            .record_payload_location_rebuild(group_span, payload_span);
    }

    pub(super) fn remove_payload_range(
        &mut self,
        owner: AnchorId,
        payload_range: GroupPayloadRange,
    ) -> Vec<PayloadRecord> {
        let owner_index = self.current_group_index(owner);
        assert_eq!(
            payload_range.group_index(),
            owner_index,
            "payload removal range must belong to the owner group",
        );
        if payload_range.is_empty() {
            return Vec::new();
        }
        let start_offset = payload_range.start_offset();
        let removed = self.remove_group_payload_range(payload_range);
        self.clear_payload_anchor_index_for_payloads(&removed);
        self.invalidate_payload_anchors(&removed);
        self.refresh_group_payload_anchor_index(owner, start_offset);
        removed
    }

    pub(super) fn remove_payload_tail_at_cursor(
        &mut self,
        owner: AnchorId,
        payload_cursor: usize,
    ) -> Vec<PayloadRecord> {
        let owner_index = self.current_group_index(owner);
        let payload_len = self.group_payload_len_at(owner_index);
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
            self.clear_payload_anchor_index_for_payloads(&removed);
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

    pub(super) fn move_payloads_for_groups(
        &mut self,
        removed_group_index: usize,
        removed_groups: &mut [GroupRecord],
    ) -> Vec<PayloadRecord> {
        self.extract_payloads_for_groups(removed_group_index, removed_groups, false)
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
            self.payload_anchor_index.clear();
            self.payload_anchor_index.shrink_to_fit();
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
        let sparse_capacity =
            self.payload_anchor_index.capacity() > total_payload_count.max(256) * 8;
        if !sparse_payload_anchor_ids && !sparse_capacity {
            return;
        }

        self.payload_anchor_index.clear();
        self.payload_anchor_index.shrink_to_fit();
        self.payload_anchors.shrink_to_fit();
        self.rebuild_payload_anchor_index_for_group_range(GroupRange::new(0, self.groups.len()));
    }
}
