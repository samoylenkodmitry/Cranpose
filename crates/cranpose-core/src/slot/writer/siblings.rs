use smallvec::SmallVec;

use super::{
    super::{ActiveSubtreeRoot, ChildCursor, DirectChildRange, GroupKey, SlotTable},
    state::SlotWriteSessionState,
};
use crate::{AnchorId, collections::map::HashMap};

const DEFAULT_SIBLING_INDEX_THRESHOLD: usize = 16;
const COMPILED_SIBLING_INDEX_THRESHOLD: usize =
    parse_sibling_index_threshold(option_env!("CRANPOSE_SIBLING_INDEX_THRESHOLD"));

const fn parse_sibling_index_threshold(value: Option<&'static str>) -> usize {
    match value {
        Some(value) => match parse_positive_usize(value) {
            Some(parsed) => parsed,
            None => DEFAULT_SIBLING_INDEX_THRESHOLD,
        },
        None => DEFAULT_SIBLING_INDEX_THRESHOLD,
    }
}

const fn parse_positive_usize(value: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    let mut index = 0usize;
    let mut parsed = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if !byte.is_ascii_digit() {
            return None;
        }

        let digit = (byte - b'0') as usize;
        match parsed.checked_mul(10) {
            Some(next) => match next.checked_add(digit) {
                Some(next) => parsed = next,
                None => return None,
            },
            None => return None,
        }
        index += 1;
    }

    if parsed == 0 { None } else { Some(parsed) }
}

#[derive(Default)]
pub(in crate::slot) struct SiblingIndex {
    by_key: HashMap<GroupKey, SmallVec<[AnchorId; 2]>>,
}

impl SiblingIndex {
    fn build(table: &mut SlotTable, parent_anchor: AnchorId, search_start: usize) -> Self {
        let Some(siblings) = repaired_sibling_range(
            table,
            parent_anchor,
            search_start,
            "later sibling index parent range",
        ) else {
            return Self::default();
        };
        let mut by_key: HashMap<GroupKey, SmallVec<[AnchorId; 2]>> = HashMap::default();
        let mut index = search_start;
        while index < siblings.end() {
            let Some((group, next_index)) =
                repaired_sibling_step(table, parent_anchor, index, "later sibling index build")
            else {
                break;
            };
            by_key.entry(group.key).or_default().push(group.anchor);
            index = next_index;
        }
        Self { by_key }
    }

    fn find(
        &self,
        table: &SlotTable,
        parent_anchor: AnchorId,
        key: GroupKey,
        search_start: usize,
    ) -> Option<ActiveSubtreeRoot> {
        let siblings = table.direct_child_range(parent_anchor);
        self.by_key.get(&key).and_then(|anchors| {
            anchors
                .iter()
                .filter_map(|anchor| {
                    let index = table.active_group_index(*anchor)?;
                    let group = table.group_sibling_record_at_index_checked(index)?;
                    (index >= search_start
                        && siblings.contains_index(index)
                        && group.parent_anchor == parent_anchor
                        && group.key == key)
                        .then_some((*anchor, index))
                })
                .min_by_key(|(_, index)| *index)
                .map(|(anchor, _)| ActiveSubtreeRoot::new(anchor))
        })
    }
}

impl SlotWriteSessionState {
    fn current_sibling_index(&mut self) -> &mut Option<SiblingIndex> {
        if let Some(frame) = self.group_stack.last_mut() {
            &mut frame.sibling_index
        } else {
            &mut self.root.sibling_index
        }
    }

    pub(in crate::slot) fn find_later_sibling(
        &mut self,
        table: &mut SlotTable,
        parent_anchor: AnchorId,
        key: GroupKey,
        search_start: usize,
    ) -> Option<ActiveSubtreeRoot> {
        if let Some(sibling_index) = self.current_sibling_index().as_ref() {
            return sibling_index.find(table, parent_anchor, key, search_start);
        }

        let siblings = repaired_sibling_range(
            table,
            parent_anchor,
            search_start,
            "later sibling search parent range",
        )?;
        if search_start >= siblings.end() {
            return None;
        }

        let mut index = search_start;
        let mut direct_children_seen = 0usize;
        while index < siblings.end() && direct_children_seen < COMPILED_SIBLING_INDEX_THRESHOLD {
            let (group, next_index) =
                repaired_sibling_step(table, parent_anchor, index, "later sibling linear scan")?;
            if group.key == key {
                return Some(ActiveSubtreeRoot::new(group.anchor));
            }
            direct_children_seen += 1;
            index = next_index;
        }

        if index >= siblings.end() {
            return None;
        }

        let sibling_index = SiblingIndex::build(table, parent_anchor, search_start);
        let found = sibling_index.find(table, parent_anchor, key, search_start);
        *self.current_sibling_index() = Some(sibling_index);
        found
    }
}

fn repaired_sibling_range(
    table: &mut SlotTable,
    parent_anchor: AnchorId,
    search_start: usize,
    operation: &'static str,
) -> Option<DirectChildRange> {
    if !table.repair_child_cursor_parent_subtree(
        ChildCursor::new(parent_anchor, search_start),
        operation,
    ) {
        return None;
    }

    let siblings = table.direct_child_range(parent_anchor);
    if search_start < siblings.start() {
        log::error!(
            "slot writer rejected later sibling search for parent {parent_anchor:?} during {operation}: search start {search_start} is before child range start {}",
            siblings.start()
        );
        return None;
    }
    Some(siblings)
}

fn repaired_sibling_step(
    table: &mut SlotTable,
    parent_anchor: AnchorId,
    index: usize,
    operation: &'static str,
) -> Option<(SiblingStep, usize)> {
    let next_index = table
        .repair_group_subtree_range_at_index(index, operation)?
        .as_group_range()
        .end();
    if next_index <= index {
        log::error!(
            "slot writer rejected later sibling step at group index {index} during {operation}: repaired next index {next_index} does not advance"
        );
        return None;
    }

    let Some(group) = table.group_sibling_record_at_index_checked(index) else {
        log::error!(
            "slot writer rejected later sibling step at missing group index {index} during {operation}"
        );
        return None;
    };
    if group.parent_anchor != parent_anchor {
        log::error!(
            "slot writer rejected later sibling step at group index {index} during {operation}: group parent {:?} does not match expected parent {parent_anchor:?}",
            group.parent_anchor
        );
        return None;
    }

    Some((
        SiblingStep {
            key: group.key,
            anchor: group.anchor,
        },
        next_index,
    ))
}

struct SiblingStep {
    key: GroupKey,
    anchor: AnchorId,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_index_threshold_parser_accepts_positive_values() {
        assert_eq!(parse_sibling_index_threshold(Some("1")), 1);
        assert_eq!(parse_sibling_index_threshold(Some("4")), 4);
        assert_eq!(parse_sibling_index_threshold(Some("64")), 64);
    }

    #[test]
    fn sibling_index_threshold_parser_rejects_invalid_values() {
        assert_eq!(
            parse_sibling_index_threshold(None),
            DEFAULT_SIBLING_INDEX_THRESHOLD
        );
        assert_eq!(
            parse_sibling_index_threshold(Some("")),
            DEFAULT_SIBLING_INDEX_THRESHOLD
        );
        assert_eq!(
            parse_sibling_index_threshold(Some("0")),
            DEFAULT_SIBLING_INDEX_THRESHOLD
        );
        assert_eq!(
            parse_sibling_index_threshold(Some("abc")),
            DEFAULT_SIBLING_INDEX_THRESHOLD
        );
        assert_eq!(
            parse_sibling_index_threshold(Some("16x")),
            DEFAULT_SIBLING_INDEX_THRESHOLD
        );
    }
}
