use super::state::SlotWriteSessionState;
use crate::{collections::map::HashMap, slot::SlotTable, slot_storage::GroupKey, AnchorId};
use smallvec::SmallVec;

const DEFAULT_SIBLING_INDEX_THRESHOLD: usize = 16;
const SIBLING_INDEX_THRESHOLD: usize =
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

    if parsed == 0 {
        None
    } else {
        Some(parsed)
    }
}

#[derive(Default)]
pub(in crate::slot) struct SiblingIndex {
    by_key: HashMap<GroupKey, SmallVec<[AnchorId; 2]>>,
}

impl SiblingIndex {
    fn build(table: &SlotTable, parent_anchor: AnchorId, search_start: usize) -> Self {
        let parent_end = table.direct_child_range_end(parent_anchor);
        let mut by_key: HashMap<GroupKey, SmallVec<[AnchorId; 2]>> = HashMap::default();
        let mut index = search_start;
        while index < parent_end {
            let group = &table.groups[index];
            debug_assert_eq!(
                group.parent_anchor, parent_anchor,
                "later sibling index must stay on direct children"
            );
            by_key.entry(group.key).or_default().push(group.anchor);
            index += group.subtree_len as usize;
        }
        Self { by_key }
    }

    fn find(
        &self,
        table: &SlotTable,
        parent_anchor: AnchorId,
        key: GroupKey,
        search_start: usize,
    ) -> Option<usize> {
        let parent_end = table.direct_child_range_end(parent_anchor);
        self.by_key.get(&key).and_then(|anchors| {
            anchors
                .iter()
                .filter_map(|anchor| {
                    let index = table.anchors.active_index(*anchor)?;
                    let group = table.groups.get(index)?;
                    (index >= search_start
                        && index < parent_end
                        && group.parent_anchor == parent_anchor
                        && group.key == key)
                        .then_some(index)
                })
                .min()
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
        table: &SlotTable,
        parent_anchor: AnchorId,
        key: GroupKey,
        search_start: usize,
    ) -> Option<usize> {
        if let Some(sibling_index) = self.current_sibling_index().as_ref() {
            return sibling_index.find(table, parent_anchor, key, search_start);
        }

        let parent_end = table.direct_child_range_end(parent_anchor);
        if search_start >= parent_end {
            return None;
        }

        let mut index = search_start;
        let mut direct_children_seen = 0usize;
        while index < parent_end && direct_children_seen < SIBLING_INDEX_THRESHOLD {
            let group = &table.groups[index];
            debug_assert_eq!(
                group.parent_anchor, parent_anchor,
                "later sibling scans must stay on direct children"
            );
            if group.key == key {
                return Some(index);
            }
            direct_children_seen += 1;
            index += group.subtree_len as usize;
        }

        if index >= parent_end {
            return None;
        }

        let sibling_index = SiblingIndex::build(table, parent_anchor, search_start);
        let found = sibling_index.find(table, parent_anchor, key, search_start);
        *self.current_sibling_index() = Some(sibling_index);
        found
    }
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
