use std::ops::Range;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::slot) struct GroupRange {
    pub(in crate::slot) start: usize,
    pub(in crate::slot) end: usize,
}

impl GroupRange {
    #[inline(always)]
    pub(in crate::slot) fn from_start_len(start: usize, len: usize) -> Self {
        Self {
            start,
            end: start + len,
        }
    }

    #[inline(always)]
    pub(in crate::slot) fn len(self) -> usize {
        self.end - self.start
    }

    #[inline(always)]
    pub(in crate::slot) fn as_range(self) -> Range<usize> {
        self.start..self.end
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::slot) struct PayloadRange {
    pub(in crate::slot) start: usize,
    pub(in crate::slot) end: usize,
}

impl PayloadRange {
    #[inline(always)]
    pub(in crate::slot) fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    #[inline(always)]
    pub(in crate::slot) fn len(self) -> usize {
        self.end - self.start
    }

    #[inline(always)]
    pub(in crate::slot) fn as_range(self) -> Range<usize> {
        self.start..self.end
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(in crate::slot) struct NodeRange {
    pub(in crate::slot) start: usize,
    pub(in crate::slot) end: usize,
}

impl NodeRange {
    #[inline(always)]
    pub(in crate::slot) fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    #[inline(always)]
    pub(in crate::slot) fn as_range(self) -> Range<usize> {
        self.start..self.end
    }
}
