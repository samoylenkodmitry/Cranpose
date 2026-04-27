use std::ops::Range;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::slot) struct GroupRange {
    start: usize,
    end: usize,
}

impl GroupRange {
    #[inline(always)]
    pub(in crate::slot) fn new(start: usize, end: usize) -> Self {
        assert!(start <= end, "group range start must not exceed end");
        Self { start, end }
    }

    #[inline(always)]
    pub(in crate::slot) fn from_start_len(start: usize, len: usize) -> Self {
        Self {
            start,
            end: start.checked_add(len).expect("group range overflow"),
        }
    }

    #[inline(always)]
    pub(in crate::slot) fn start(self) -> usize {
        self.start
    }

    #[inline(always)]
    pub(in crate::slot) fn end(self) -> usize {
        self.end
    }

    #[inline(always)]
    pub(in crate::slot) fn len(self) -> usize {
        self.end - self.start
    }

    #[inline(always)]
    pub(in crate::slot) fn contains_index(self, index: usize) -> bool {
        self.start <= index && index < self.end
    }

    #[inline(always)]
    pub(in crate::slot) fn as_range(self) -> Range<usize> {
        self.start..self.end
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::slot) struct SubtreeRange {
    groups: GroupRange,
}

impl SubtreeRange {
    #[inline(always)]
    pub(in crate::slot) fn from_root_len(root_index: usize, len: usize) -> Self {
        assert!(len > 0, "subtree range must include a root group");
        Self {
            groups: GroupRange::from_start_len(root_index, len),
        }
    }

    #[inline(always)]
    pub(in crate::slot) fn root_index(self) -> usize {
        self.groups.start()
    }

    #[inline(always)]
    pub(in crate::slot) fn len(self) -> usize {
        self.groups.len()
    }

    #[inline(always)]
    pub(in crate::slot) fn as_group_range(self) -> GroupRange {
        self.groups
    }

    #[inline(always)]
    pub(in crate::slot) fn as_range(self) -> Range<usize> {
        self.groups.as_range()
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::slot) struct DirectChildRange {
    groups: GroupRange,
}

impl DirectChildRange {
    #[inline(always)]
    pub(in crate::slot) fn new(start: usize, end: usize) -> Self {
        Self {
            groups: GroupRange::new(start, end),
        }
    }

    #[inline(always)]
    pub(in crate::slot) fn start(self) -> usize {
        self.groups.start()
    }

    #[inline(always)]
    pub(in crate::slot) fn end(self) -> usize {
        self.groups.end()
    }

    #[inline(always)]
    pub(in crate::slot) fn contains_index(self, index: usize) -> bool {
        self.groups.contains_index(index)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::slot) struct PayloadRange {
    start: usize,
    end: usize,
}

impl PayloadRange {
    #[inline(always)]
    pub(in crate::slot) fn new(start: usize, end: usize) -> Self {
        assert!(start <= end, "payload range start must not exceed end");
        Self { start, end }
    }

    #[inline(always)]
    pub(in crate::slot) fn from_start_len(start: usize, len: usize) -> Self {
        Self {
            start,
            end: start.checked_add(len).expect("payload range overflow"),
        }
    }

    #[inline(always)]
    pub(in crate::slot) fn start(self) -> usize {
        self.start
    }

    #[inline(always)]
    pub(in crate::slot) fn len(self) -> usize {
        self.end - self.start
    }

    #[inline(always)]
    pub(in crate::slot) fn is_empty(self) -> bool {
        self.start == self.end
    }

    #[inline(always)]
    pub(in crate::slot) fn subrange(self, start: usize, end: usize) -> Self {
        assert!(start <= end, "payload subrange start must not exceed end");
        assert!(end <= self.len(), "payload subrange must stay in group");
        Self::new(self.start + start, self.start + end)
    }

    #[inline(always)]
    pub(in crate::slot) fn as_range(self) -> Range<usize> {
        self.start..self.end
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::slot) struct GroupPayloadRange {
    group_index: usize,
    start_offset: usize,
    payloads: PayloadRange,
}

impl GroupPayloadRange {
    #[inline(always)]
    pub(in crate::slot) fn new(
        group_index: usize,
        group_payloads: PayloadRange,
        start_offset: usize,
        end_offset: usize,
    ) -> Self {
        Self {
            group_index,
            start_offset,
            payloads: group_payloads.subrange(start_offset, end_offset),
        }
    }

    #[inline(always)]
    pub(in crate::slot) fn group_index(self) -> usize {
        self.group_index
    }

    #[inline(always)]
    pub(in crate::slot) fn start_offset(self) -> usize {
        self.start_offset
    }

    #[inline(always)]
    pub(in crate::slot) fn is_empty(self) -> bool {
        self.payloads.is_empty()
    }

    #[inline(always)]
    pub(in crate::slot) fn as_payload_range(self) -> PayloadRange {
        self.payloads
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::slot) struct NodeRange {
    start: usize,
    end: usize,
}

impl NodeRange {
    #[inline(always)]
    pub(in crate::slot) fn new(start: usize, end: usize) -> Self {
        assert!(start <= end, "node range start must not exceed end");
        Self { start, end }
    }

    #[inline(always)]
    pub(in crate::slot) fn from_start_len(start: usize, len: usize) -> Self {
        Self {
            start,
            end: start.checked_add(len).expect("node range overflow"),
        }
    }

    #[inline(always)]
    pub(in crate::slot) fn len(self) -> usize {
        self.end - self.start
    }

    #[inline(always)]
    pub(in crate::slot) fn is_empty(self) -> bool {
        self.start == self.end
    }

    #[inline(always)]
    pub(in crate::slot) fn subrange(self, start: usize, end: usize) -> Self {
        assert!(start <= end, "node subrange start must not exceed end");
        assert!(end <= self.len(), "node subrange must stay in group");
        Self::new(self.start + start, self.start + end)
    }

    #[inline(always)]
    pub(in crate::slot) fn as_range(self) -> Range<usize> {
        self.start..self.end
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::slot) struct GroupNodeRange {
    group_index: usize,
    nodes: NodeRange,
}

impl GroupNodeRange {
    #[inline(always)]
    pub(in crate::slot) fn new(
        group_index: usize,
        group_nodes: NodeRange,
        start_offset: usize,
        end_offset: usize,
    ) -> Self {
        Self {
            group_index,
            nodes: group_nodes.subrange(start_offset, end_offset),
        }
    }

    #[inline(always)]
    pub(in crate::slot) fn group_index(self) -> usize {
        self.group_index
    }

    #[inline(always)]
    pub(in crate::slot) fn is_empty(self) -> bool {
        self.nodes.is_empty()
    }

    #[inline(always)]
    pub(in crate::slot) fn as_node_range(self) -> NodeRange {
        self.nodes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_range_tracks_empty_and_non_empty_spans() {
        let empty = GroupRange::new(2, 2);
        assert_eq!(empty.as_range(), 2..2);

        let non_empty = GroupRange::from_start_len(2, 3);
        assert_eq!(non_empty.as_range(), 2..5);
    }

    #[test]
    fn subtree_range_keeps_root_and_span_together() {
        let range = SubtreeRange::from_root_len(4, 3);
        assert_eq!(range.root_index(), 4);
        assert_eq!(range.len(), 3);
        assert_eq!(range.as_range(), 4..7);
    }

    #[test]
    fn group_payload_range_converts_group_offsets_to_table_range() {
        let group_payloads = PayloadRange::new(10, 15);
        let range = GroupPayloadRange::new(2, group_payloads, 1, 4);
        assert_eq!(range.group_index(), 2);
        assert_eq!(range.start_offset(), 1);
        assert_eq!(range.as_payload_range().as_range(), 11..14);
    }

    #[test]
    fn group_node_range_converts_group_offsets_to_table_range() {
        let group_nodes = NodeRange::new(20, 26);
        let range = GroupNodeRange::new(5, group_nodes, 2, 6);
        assert_eq!(range.group_index(), 5);
        assert_eq!(range.as_node_range().as_range(), 22..26);
    }

    #[test]
    fn direct_child_range_accepts_only_indexes_inside_parent_span() {
        let range = DirectChildRange::new(3, 8);
        assert_eq!(range.start(), 3);
        assert!(!range.contains_index(2));
        assert!(range.contains_index(3));
        assert!(range.contains_index(7));
        assert!(!range.contains_index(8));
        assert_eq!(range.end(), 8);
    }
}
