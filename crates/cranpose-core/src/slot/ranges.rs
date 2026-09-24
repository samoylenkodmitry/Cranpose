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

    #[cfg(test)]
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

pub(in crate::slot) trait ItemRangeKind: Copy {
    const LABEL: &'static str;
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::slot) struct PayloadRangeKind;

impl ItemRangeKind for PayloadRangeKind {
    const LABEL: &'static str = "payload";
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::slot) struct NodeRangeKind;

impl ItemRangeKind for NodeRangeKind {
    const LABEL: &'static str = "node";
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::slot) struct TypedItemRange<K: ItemRangeKind> {
    start: usize,
    end: usize,
    _kind: std::marker::PhantomData<fn() -> K>,
}

impl<K: ItemRangeKind> TypedItemRange<K> {
    #[inline(always)]
    pub(in crate::slot) fn new(start: usize, end: usize) -> Self {
        assert!(start <= end, "{} range start must not exceed end", K::LABEL);
        Self {
            start,
            end,
            _kind: std::marker::PhantomData,
        }
    }

    #[inline(always)]
    pub(in crate::slot) fn from_start_len(start: usize, len: usize) -> Self {
        Self {
            start,
            end: start
                .checked_add(len)
                .unwrap_or_else(|| panic!("{} range overflow", K::LABEL)),
            _kind: std::marker::PhantomData,
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
        assert!(
            start <= end,
            "{} subrange start must not exceed end",
            K::LABEL
        );
        assert!(
            end <= self.len(),
            "{} subrange must stay in group",
            K::LABEL
        );
        Self::new(self.start + start, self.start + end)
    }

    #[inline(always)]
    pub(in crate::slot) fn as_range(self) -> Range<usize> {
        self.start..self.end
    }
}

pub(in crate::slot) type PayloadRange = TypedItemRange<PayloadRangeKind>;
pub(in crate::slot) type NodeRange = TypedItemRange<NodeRangeKind>;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::slot) struct GroupItemRange<K: ItemRangeKind> {
    group_index: usize,
    items: TypedItemRange<K>,
}

impl<K: ItemRangeKind> GroupItemRange<K> {
    #[inline(always)]
    pub(in crate::slot) fn new(
        group_index: usize,
        group_items: TypedItemRange<K>,
        start_offset: usize,
        end_offset: usize,
    ) -> Self {
        Self {
            group_index,
            items: group_items.subrange(start_offset, end_offset),
        }
    }

    #[inline(always)]
    pub(in crate::slot) fn group_index(self) -> usize {
        self.group_index
    }

    #[inline(always)]
    pub(in crate::slot) fn is_empty(self) -> bool {
        self.items.is_empty()
    }

    #[inline(always)]
    pub(in crate::slot) fn as_range(self) -> Range<usize> {
        self.items.as_range()
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::slot) struct GroupPayloadRange {
    start_offset: usize,
    range: GroupItemRange<PayloadRangeKind>,
}

impl GroupPayloadRange {
    #[inline(always)]
    pub(in crate::slot) fn from_range(
        range: GroupItemRange<PayloadRangeKind>,
        start_offset: usize,
    ) -> Self {
        Self {
            start_offset,
            range,
        }
    }

    #[inline(always)]
    pub(in crate::slot) fn group_index(self) -> usize {
        self.range.group_index()
    }

    #[inline(always)]
    pub(in crate::slot) fn start_offset(self) -> usize {
        self.start_offset
    }

    #[inline(always)]
    pub(in crate::slot) fn is_empty(self) -> bool {
        self.range.is_empty()
    }

    #[inline(always)]
    pub(in crate::slot) fn into_inner(self) -> GroupItemRange<PayloadRangeKind> {
        self.range
    }
}

pub(in crate::slot) type GroupNodeRange = GroupItemRange<NodeRangeKind>;

#[cfg(test)]
#[path = "tests/ranges_tests.rs"]
mod tests;
