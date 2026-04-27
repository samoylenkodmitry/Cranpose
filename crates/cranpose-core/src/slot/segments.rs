use super::{
    ranges::{GroupItemRange, ItemRangeKind, NodeRangeKind, PayloadRangeKind, TypedItemRange},
    GroupRecord,
};

pub(in crate::slot) trait GroupSegment {
    const NAME: &'static str;
    type RangeKind: ItemRangeKind;

    fn start(group: &GroupRecord) -> u32;
    fn start_mut(group: &mut GroupRecord) -> &mut u32;
    fn len(group: &GroupRecord) -> u32;
    fn len_mut(group: &mut GroupRecord) -> &mut u32;
}

pub(in crate::slot) struct PayloadSegment;

impl GroupSegment for PayloadSegment {
    const NAME: &'static str = "payload";
    type RangeKind = PayloadRangeKind;

    fn start(group: &GroupRecord) -> u32 {
        group.payload_start
    }

    fn start_mut(group: &mut GroupRecord) -> &mut u32 {
        &mut group.payload_start
    }

    fn len(group: &GroupRecord) -> u32 {
        group.payload_len
    }

    fn len_mut(group: &mut GroupRecord) -> &mut u32 {
        &mut group.payload_len
    }
}

pub(in crate::slot) struct NodeSegment;

impl GroupSegment for NodeSegment {
    const NAME: &'static str = "node";
    type RangeKind = NodeRangeKind;

    fn start(group: &GroupRecord) -> u32 {
        group.node_start
    }

    fn start_mut(group: &mut GroupRecord) -> &mut u32 {
        &mut group.node_start
    }

    fn len(group: &GroupRecord) -> u32 {
        group.node_len
    }

    fn len_mut(group: &mut GroupRecord) -> &mut u32 {
        &mut group.node_len
    }
}

pub(in crate::slot) fn group_segment_start<S: GroupSegment>(
    groups: &[GroupRecord],
    group_index: usize,
) -> usize {
    S::start(&groups[group_index]) as usize
}

pub(in crate::slot) fn group_segment_len<S: GroupSegment>(
    groups: &[GroupRecord],
    group_index: usize,
) -> usize {
    S::len(&groups[group_index]) as usize
}

pub(in crate::slot) fn group_segment_range_checked<S: GroupSegment>(
    groups: &[GroupRecord],
    item_count: usize,
    group_index: usize,
) -> Option<TypedItemRange<S::RangeKind>> {
    let start = group_segment_start::<S>(groups, group_index);
    let len = group_segment_len::<S>(groups, group_index);
    let end = start.checked_add(len)?;
    (end <= item_count).then_some(TypedItemRange::new(start, end))
}

pub(in crate::slot) fn group_segment_range_at<S: GroupSegment>(
    groups: &[GroupRecord],
    item_count: usize,
    group_index: usize,
) -> TypedItemRange<S::RangeKind> {
    group_segment_range_checked::<S>(groups, item_count, group_index)
        .unwrap_or_else(|| panic!("{} range should resolve", S::NAME))
}

pub(in crate::slot) fn group_segment_subrange_at<S: GroupSegment>(
    groups: &[GroupRecord],
    item_count: usize,
    group_index: usize,
    start_offset: usize,
    end_offset: usize,
) -> GroupItemRange<S::RangeKind> {
    GroupItemRange::new(
        group_index,
        group_segment_range_at::<S>(groups, item_count, group_index),
        start_offset,
        end_offset,
    )
}

pub(in crate::slot) fn segment_insert_index_for_group<S: GroupSegment>(
    groups: &[GroupRecord],
    item_count: usize,
    group_index: usize,
) -> usize {
    if group_index < groups.len() {
        group_segment_start::<S>(groups, group_index)
    } else {
        item_count
    }
}

pub(in crate::slot) fn shift_group_segment_starts_from<S: GroupSegment>(
    groups: &mut [GroupRecord],
    start_group_index: usize,
    delta: i64,
) {
    if delta == 0 {
        return;
    }
    for group in &mut groups[start_group_index..] {
        apply_group_segment_start_delta::<S>(group, delta);
    }
}

pub(in crate::slot) fn offset_detached_group_segment_starts<S: GroupSegment>(
    groups: &mut [GroupRecord],
    delta: i64,
) {
    if delta == 0 {
        return;
    }
    for group in groups {
        apply_group_segment_start_delta::<S>(group, delta);
    }
}

pub(in crate::slot) fn subtree_segment_span<S: GroupSegment>(
    groups: &[GroupRecord],
) -> Option<(usize, usize)> {
    let start = S::start(groups.first()?) as usize;
    let len = groups
        .iter()
        .map(|group| S::len(group) as usize)
        .sum::<usize>();
    Some((start, len))
}

pub(in crate::slot) fn add_group_segment_len<S: GroupSegment>(
    groups: &mut [GroupRecord],
    group_index: usize,
    delta: i64,
) {
    let len = S::len_mut(&mut groups[group_index]);
    let updated = (*len as i64) + delta;
    debug_assert!(updated >= 0, "{} length cannot become negative", S::NAME);
    *len = updated as u32;
}

pub(in crate::slot) fn insert_group_segment_item<S: GroupSegment, T>(
    groups: &mut [GroupRecord],
    items: &mut Vec<T>,
    group_index: usize,
    item_offset: usize,
    item: T,
) {
    let insert_index =
        segment_insert_index_for_group::<S>(groups, items.len(), group_index) + item_offset;
    items.insert(insert_index, item);
    add_group_segment_len::<S>(groups, group_index, 1);
    shift_group_segment_starts_from::<S>(groups, group_index + 1, 1);
}

pub(in crate::slot) fn remove_group_segment_range<S: GroupSegment, T>(
    groups: &mut [GroupRecord],
    items: &mut Vec<T>,
    item_range: GroupItemRange<S::RangeKind>,
) -> Vec<T> {
    if item_range.is_empty() {
        return Vec::new();
    }
    let group_index = item_range.group_index();
    let removed = items.drain(item_range.as_range()).collect::<Vec<_>>();
    add_group_segment_len::<S>(groups, group_index, -(removed.len() as i64));
    shift_group_segment_starts_from::<S>(groups, group_index + 1, -(removed.len() as i64));
    removed
}

pub(in crate::slot) fn extract_subtree_segment<S: GroupSegment, T>(
    groups: &mut [GroupRecord],
    items: &mut Vec<T>,
    removed_group_index: usize,
    removed_groups: &mut [GroupRecord],
) -> Vec<T> {
    let Some((item_start, item_len)) = subtree_segment_span::<S>(removed_groups) else {
        return Vec::new();
    };
    offset_detached_group_segment_starts::<S>(removed_groups, -(item_start as i64));
    if item_len == 0 {
        return Vec::new();
    }

    let item_range = TypedItemRange::<S::RangeKind>::from_start_len(item_start, item_len);
    let removed = items.drain(item_range.as_range()).collect::<Vec<_>>();
    shift_group_segment_starts_from::<S>(groups, removed_group_index, -(item_len as i64));
    removed
}

pub(in crate::slot) fn restore_subtree_segment<S: GroupSegment, T>(
    groups: &mut [GroupRecord],
    items: &mut Vec<T>,
    insert_group_index: usize,
    restoring_groups: &mut [GroupRecord],
    restoring_items: Vec<T>,
) {
    let item_insert_index =
        segment_insert_index_for_group::<S>(groups, items.len(), insert_group_index);
    shift_group_segment_starts_from::<S>(groups, insert_group_index, restoring_items.len() as i64);
    offset_detached_group_segment_starts::<S>(restoring_groups, item_insert_index as i64);
    items.splice(item_insert_index..item_insert_index, restoring_items);
}

fn apply_group_segment_start_delta<S: GroupSegment>(group: &mut GroupRecord, delta: i64) {
    let start = S::start_mut(group);
    let updated = (*start as i64) + delta;
    debug_assert!(updated >= 0, "{} start cannot become negative", S::NAME);
    *start = updated as u32;
}
