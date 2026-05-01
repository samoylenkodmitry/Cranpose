use super::{
    checked_u32_delta, checked_usize_to_i64,
    ranges::{GroupItemRange, ItemRangeKind, NodeRangeKind, PayloadRangeKind, TypedItemRange},
    CheckedU32Delta, GroupRecord,
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
    let len = groups
        .iter()
        .try_fold(0usize, |len, group| len.checked_add(S::len(group) as usize))
        .unwrap_or_else(|| panic!("{} subtree segment length overflow", S::NAME));
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
    let insert_index = segment_insert_index_for_group::<S>(groups, items.len(), group_index)
        .checked_add(item_offset)
        .unwrap_or_else(|| panic!("{} insert index overflow", S::NAME));
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
    let item_insert_index =
        segment_insert_index_for_group::<S>(groups, items.len(), insert_group_index);
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
    assert!(
        insert_group_index < moving_group_index,
        "{} segment move must move a later group to an earlier index",
        S::NAME
    );
    assert!(
        moving_group_len > 0,
        "{} segment move must include at least one group",
        S::NAME
    );
    assert!(
        moving_group_index + moving_group_len <= groups.len(),
        "{} segment move range must stay inside groups",
        S::NAME
    );

    let Some((item_start, item_len)) = subtree_segment_span::<S>(
        &groups[moving_group_index..moving_group_index + moving_group_len],
    ) else {
        return 0;
    };

    let item_insert_index =
        segment_insert_index_for_group::<S>(groups, items.len(), insert_group_index);
    assert!(
        item_insert_index <= item_start,
        "{} segment move must not move items backward past their target",
        S::NAME
    );

    if item_len > 0 {
        items[item_insert_index..item_start + item_len].rotate_right(item_len);
    }

    let moved_start_delta = checked_usize_to_i64(
        item_start - item_insert_index,
        "moved subtree segment distance",
    );
    let item_len_delta = checked_usize_to_i64(item_len, "moved subtree segment length");
    offset_detached_group_segment_starts::<S>(
        &mut groups[moving_group_index..moving_group_index + moving_group_len],
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
