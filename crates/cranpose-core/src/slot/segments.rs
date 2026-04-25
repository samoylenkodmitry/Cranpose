use super::GroupRecord;
use std::ops::Range;

pub(in crate::slot) trait GroupSegment {
    const NAME: &'static str;

    fn start(group: &GroupRecord) -> u32;
    fn start_mut(group: &mut GroupRecord) -> &mut u32;
    fn len(group: &GroupRecord) -> u32;
    fn len_mut(group: &mut GroupRecord) -> &mut u32;
}

pub(in crate::slot) struct PayloadSegment;

impl GroupSegment for PayloadSegment {
    const NAME: &'static str = "payload";

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
) -> Option<Range<usize>> {
    let start = group_segment_start::<S>(groups, group_index);
    let len = group_segment_len::<S>(groups, group_index);
    let end = start.checked_add(len)?;
    (end <= item_count).then_some(start..end)
}

pub(in crate::slot) fn group_segment_range_at<S: GroupSegment>(
    groups: &[GroupRecord],
    item_count: usize,
    group_index: usize,
) -> Range<usize> {
    group_segment_range_checked::<S>(groups, item_count, group_index)
        .unwrap_or_else(|| panic!("{} range should resolve", S::NAME))
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

fn apply_group_segment_start_delta<S: GroupSegment>(group: &mut GroupRecord, delta: i64) {
    let start = S::start_mut(group);
    let updated = (*start as i64) + delta;
    debug_assert!(updated >= 0, "{} start cannot become negative", S::NAME);
    *start = updated as u32;
}
