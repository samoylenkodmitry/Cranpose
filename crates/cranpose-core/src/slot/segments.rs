use super::{
    CheckedU32Delta, GroupRecord, checked_u32_delta, checked_usize_to_i64, checked_usize_to_u32,
    ranges::{GroupItemRange, ItemRangeKind, NodeRangeKind, PayloadRangeKind, TypedItemRange},
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

pub(in crate::slot) struct SegmentLengthRepair {
    pub(in crate::slot) len: usize,
    pub(in crate::slot) repaired: bool,
}

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
    let group = groups.get(group_index)?;
    let start = S::start(group) as usize;
    let len = S::len(group) as usize;
    let end = start.checked_add(len)?;
    (end <= item_count).then_some(TypedItemRange::new(start, end))
}

fn group_segment_storage_available_len<S: GroupSegment>(
    groups: &[GroupRecord],
    item_count: usize,
    group_index: usize,
) -> usize {
    let Some(group) = groups.get(group_index) else {
        return 0;
    };
    let start = S::start(group) as usize;
    if start >= item_count {
        return 0;
    }
    let next_start = groups
        .get(group_index + 1)
        .map(|group| S::start(group) as usize)
        .unwrap_or(item_count);
    next_start.min(item_count).saturating_sub(start)
}

fn expected_group_segment_start<S: GroupSegment>(
    groups: &[GroupRecord],
    item_count: usize,
    group_index: usize,
) -> Option<usize> {
    if group_index == 0 {
        return Some(0);
    }
    let previous = groups.get(group_index.checked_sub(1)?)?;
    let previous_start = S::start(previous) as usize;
    let previous_len = S::len(previous) as usize;
    let expected = previous_start.checked_add(previous_len).unwrap_or_else(|| {
        log::error!(
            "slot table clamped overflowing expected {} segment start before group index {group_index}",
            S::NAME
        );
        item_count
    });
    Some(expected.min(item_count))
}

fn repair_group_segment_start_to_storage<S: GroupSegment>(
    groups: &mut [GroupRecord],
    item_count: usize,
    group_index: usize,
    operation: &'static str,
) -> (usize, bool) {
    let expected =
        expected_group_segment_start::<S>(groups, item_count, group_index).unwrap_or(item_count);
    let Some(group) = groups.get_mut(group_index) else {
        log::error!(
            "slot table ignored {} segment start repair for missing group index {group_index} during {operation}",
            S::NAME
        );
        return (expected, false);
    };
    let declared = S::start(group) as usize;
    if declared == expected {
        return (declared, false);
    }
    log::error!(
        "slot table repaired {} segment start for group index {group_index} during {operation}: declared {declared}, expected {expected}",
        S::NAME
    );
    *S::start_mut(group) = checked_usize_to_u32(expected, "group segment start repair");
    (expected, true)
}

fn removed_subtree_segment_end<S: GroupSegment>(
    groups: &[GroupRecord],
    item_count: usize,
    removed_group_index: usize,
) -> usize {
    groups
        .get(removed_group_index)
        .map(|group| S::start(group) as usize)
        .unwrap_or(item_count)
        .min(item_count)
}

pub(in crate::slot) fn repair_group_segment_len_to_storage<S: GroupSegment>(
    groups: &mut [GroupRecord],
    item_count: usize,
    group_index: usize,
    operation: &'static str,
) -> SegmentLengthRepair {
    if let Some(range) = group_segment_range_checked::<S>(groups, item_count, group_index) {
        return SegmentLengthRepair {
            len: range.len(),
            repaired: false,
        };
    }

    let available = group_segment_storage_available_len::<S>(groups, item_count, group_index);
    let Some(group) = groups.get_mut(group_index) else {
        log::error!(
            "slot table ignored {} segment repair for missing group index {group_index} during {operation}",
            S::NAME
        );
        return SegmentLengthRepair {
            len: 0,
            repaired: false,
        };
    };
    let declared = S::len(group) as usize;
    log::error!(
        "slot table repaired {} segment length for group index {group_index} during {operation}: declared {declared}, available {available}",
        S::NAME
    );
    *S::len_mut(group) = checked_usize_to_u32(available, "group segment length repair");
    SegmentLengthRepair {
        len: available,
        repaired: true,
    }
}

pub(in crate::slot) fn repair_group_segment_start_and_len_to_storage<S: GroupSegment>(
    groups: &mut [GroupRecord],
    item_count: usize,
    group_index: usize,
    operation: &'static str,
) -> SegmentLengthRepair {
    let (_, start_repaired) =
        repair_group_segment_start_to_storage::<S>(groups, item_count, group_index, operation);
    let mut repair =
        repair_group_segment_len_to_storage::<S>(groups, item_count, group_index, operation);
    repair.repaired |= start_repaired;
    repair
}

pub(in crate::slot) fn group_segment_subrange_at<S: GroupSegment>(
    groups: &[GroupRecord],
    item_count: usize,
    group_index: usize,
    start_offset: usize,
    end_offset: usize,
) -> GroupItemRange<S::RangeKind> {
    let Some(group_range) = group_segment_range_checked::<S>(groups, item_count, group_index)
    else {
        log::error!(
            "slot table produced empty {} segment subrange for corrupt segment at group index {group_index}",
            S::NAME
        );
        return empty_group_segment_range::<S>(groups, item_count, group_index);
    };
    if start_offset > end_offset {
        log::error!(
            "slot table produced empty {} segment subrange for reversed offsets {start_offset}..{end_offset} at group index {group_index}",
            S::NAME
        );
        return empty_group_segment_range::<S>(groups, item_count, group_index);
    }
    if end_offset > group_range.len() {
        log::error!(
            "slot table produced empty {} segment subrange for offsets {start_offset}..{end_offset} outside group length {} at group index {group_index}",
            S::NAME,
            group_range.len()
        );
        return empty_group_segment_range::<S>(groups, item_count, group_index);
    }
    GroupItemRange::new(group_index, group_range, start_offset, end_offset)
}

fn empty_group_segment_range<S: GroupSegment>(
    groups: &[GroupRecord],
    item_count: usize,
    group_index: usize,
) -> GroupItemRange<S::RangeKind> {
    let start = groups
        .get(group_index)
        .map(|group| S::start(group) as usize)
        .unwrap_or(item_count)
        .min(item_count);
    GroupItemRange::new(group_index, TypedItemRange::new(start, start), 0, 0)
}

fn segment_insert_index_for_group_mut<S: GroupSegment>(
    groups: &mut [GroupRecord],
    item_count: usize,
    group_index: usize,
    operation: &'static str,
) -> usize {
    repair_group_segment_start_to_storage::<S>(groups, item_count, group_index, operation).0
}

pub(in crate::slot) fn shift_group_segment_starts_from<S: GroupSegment>(
    groups: &mut [GroupRecord],
    start_group_index: usize,
    delta: i64,
) {
    if delta == 0 {
        return;
    }
    let delta = CheckedU32Delta::from_i64(delta, S::NAME);
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
    let delta = CheckedU32Delta::from_i64(delta, S::NAME);
    for group in groups {
        apply_group_segment_start_delta::<S>(group, delta);
    }
}

pub(in crate::slot) fn subtree_segment_span<S: GroupSegment>(
    groups: &[GroupRecord],
) -> Option<(usize, usize)> {
    let start = S::start(groups.first()?) as usize;
    let Some(len) = groups
        .iter()
        .try_fold(0usize, |len, group| len.checked_add(S::len(group) as usize))
    else {
        log::error!(
            "slot table ignored {} subtree segment span overflow",
            S::NAME
        );
        return None;
    };
    Some((start, len))
}

pub(in crate::slot) fn add_group_segment_len<S: GroupSegment>(
    groups: &mut [GroupRecord],
    group_index: usize,
    delta: i64,
) {
    let len = S::len_mut(&mut groups[group_index]);
    let delta = CheckedU32Delta::from_i64(delta, S::NAME);
    *len = checked_u32_delta(*len, delta, 0, S::NAME);
}

pub(in crate::slot) fn insert_group_segment_item<S: GroupSegment, T>(
    groups: &mut [GroupRecord],
    items: &mut Vec<T>,
    group_index: usize,
    item_offset: usize,
    item: T,
) {
    if group_index >= groups.len() {
        log::error!(
            "slot table ignored {} segment item insertion for missing group index {group_index}",
            S::NAME
        );
        return;
    }
    let insert_index = segment_insert_index_for_group_mut::<S>(
        groups,
        items.len(),
        group_index,
        "segment item insertion",
    );
    let repaired_len = repair_group_segment_len_to_storage::<S>(
        groups,
        items.len(),
        group_index,
        "segment item insertion",
    )
    .len;
    if item_offset > repaired_len {
        log::error!(
            "slot table ignored {} segment item insertion for group index {group_index}: item offset {item_offset} exceeds segment length {repaired_len}",
            S::NAME
        );
        return;
    }
    let Some(insert_index) = insert_index.checked_add(item_offset) else {
        log::error!(
            "slot table ignored overflowing {} segment item insertion for group index {group_index}: base {insert_index}, offset {item_offset}",
            S::NAME
        );
        return;
    };
    if insert_index > items.len() {
        log::error!(
            "slot table ignored {} segment item insertion for group index {group_index}: item index {insert_index} exceeds storage length {}",
            S::NAME,
            items.len()
        );
        return;
    }
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
    let removed_len = checked_usize_to_i64(removed.len(), "removed segment length");
    add_group_segment_len::<S>(groups, group_index, -removed_len);
    shift_group_segment_starts_from::<S>(groups, group_index + 1, -removed_len);
    removed
}

pub(in crate::slot) fn extract_subtree_segment<S: GroupSegment, T>(
    groups: &mut [GroupRecord],
    items: &mut Vec<T>,
    removed_group_index: usize,
    removed_groups: &mut [GroupRecord],
) -> Vec<T> {
    let segment_end = removed_subtree_segment_end::<S>(groups, items.len(), removed_group_index);
    for group_index in 0..removed_groups.len() {
        repair_group_segment_len_to_storage::<S>(
            removed_groups,
            segment_end,
            group_index,
            "subtree segment extraction",
        );
    }

    let Some((item_start, item_len)) = subtree_segment_span::<S>(removed_groups) else {
        return Vec::new();
    };
    let item_start_delta = checked_usize_to_i64(item_start, "detached segment start");
    offset_detached_group_segment_starts::<S>(removed_groups, -item_start_delta);
    if item_len == 0 {
        return Vec::new();
    }

    let item_range = TypedItemRange::<S::RangeKind>::from_start_len(item_start, item_len);
    let removed = items.drain(item_range.as_range()).collect::<Vec<_>>();
    let item_len_delta = checked_usize_to_i64(item_len, "removed subtree segment length");
    shift_group_segment_starts_from::<S>(groups, removed_group_index, -item_len_delta);
    removed
}

pub(in crate::slot) fn restore_subtree_segment<S: GroupSegment, T>(
    groups: &mut [GroupRecord],
    items: &mut Vec<T>,
    insert_group_index: usize,
    restoring_groups: &mut [GroupRecord],
    restoring_items: Vec<T>,
) {
    let item_insert_index = segment_insert_index_for_group_mut::<S>(
        groups,
        items.len(),
        insert_group_index,
        "subtree segment restore",
    );
    let restoring_len = checked_usize_to_i64(restoring_items.len(), "restoring segment length");
    shift_group_segment_starts_from::<S>(groups, insert_group_index, restoring_len);
    let item_insert_delta = checked_usize_to_i64(item_insert_index, "restoring segment start");
    offset_detached_group_segment_starts::<S>(restoring_groups, item_insert_delta);
    items.splice(item_insert_index..item_insert_index, restoring_items);
}

pub(in crate::slot) fn move_subtree_segment_to_earlier_group<S: GroupSegment, T>(
    groups: &mut [GroupRecord],
    items: &mut [T],
    insert_group_index: usize,
    moving_group_index: usize,
    moving_group_len: usize,
) -> usize {
    if insert_group_index >= moving_group_index {
        log::error!(
            "slot table ignored {} segment move from group {moving_group_index} to non-earlier group {insert_group_index}",
            S::NAME
        );
        return 0;
    }
    if moving_group_len == 0 {
        log::error!(
            "slot table ignored {} segment move with empty moving group range",
            S::NAME
        );
        return 0;
    }
    let Some(moving_group_end) = moving_group_index.checked_add(moving_group_len) else {
        log::error!(
            "slot table ignored {} segment move because moving group range overflowed",
            S::NAME
        );
        return 0;
    };
    if moving_group_end > groups.len() {
        log::error!(
            "slot table ignored {} segment move for group range {moving_group_index}..{moving_group_end} outside {} groups",
            S::NAME,
            groups.len()
        );
        return 0;
    }

    for group_index in insert_group_index..moving_group_end {
        repair_group_segment_start_and_len_to_storage::<S>(
            groups,
            items.len(),
            group_index,
            "subtree segment move",
        );
    }

    let Some((item_start, item_len)) =
        subtree_segment_span::<S>(&groups[moving_group_index..moving_group_end])
    else {
        return 0;
    };

    let item_insert_index = segment_insert_index_for_group_mut::<S>(
        groups,
        items.len(),
        insert_group_index,
        "subtree segment move",
    );
    if item_insert_index > item_start {
        log::error!(
            "slot table ignored {} segment move because target item index {item_insert_index} is after moving item start {item_start}",
            S::NAME
        );
        return 0;
    }
    let Some(item_end) = item_start.checked_add(item_len) else {
        log::error!(
            "slot table ignored {} segment move because moving item range overflowed",
            S::NAME
        );
        return 0;
    };
    if item_end > items.len() {
        log::error!(
            "slot table ignored {} segment move because item range {item_start}..{item_end} exceeds {} items",
            S::NAME,
            items.len()
        );
        return 0;
    }

    if item_len > 0 {
        items[item_insert_index..item_end].rotate_right(item_len);
    }

    let moved_start_delta = checked_usize_to_i64(
        item_start - item_insert_index,
        "moved subtree segment distance",
    );
    let item_len_delta = checked_usize_to_i64(item_len, "moved subtree segment length");
    offset_detached_group_segment_starts::<S>(
        &mut groups[moving_group_index..moving_group_end],
        -moved_start_delta,
    );
    shift_group_segment_starts_from::<S>(
        &mut groups[insert_group_index..moving_group_index],
        0,
        item_len_delta,
    );
    item_len
}

fn apply_group_segment_start_delta<S: GroupSegment>(
    group: &mut GroupRecord,
    delta: CheckedU32Delta,
) {
    let start = S::start_mut(group);
    *start = checked_u32_delta(*start, delta, 0, S::NAME);
}
