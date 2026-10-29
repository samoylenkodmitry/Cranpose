use std::ops::Range;

// A range of byte offsets or indices in 32 bits each, half the room of a
// `Range<usize>` on 64-bit targets, for the lines and runs a layout keeps.
// Text offsets fit in 32 bits, as a glyph's cluster does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct IndexRange {
    start: u32,
    end: u32,
}

impl IndexRange {
    pub(crate) fn start(self) -> usize {
        widen(self.start)
    }

    pub(crate) fn end(self) -> usize {
        widen(self.end)
    }

    pub(crate) fn range(self) -> Range<usize> {
        self.start()..self.end()
    }
}

impl From<Range<usize>> for IndexRange {
    fn from(range: Range<usize>) -> Self {
        Self {
            start: narrow(range.start),
            end: narrow(range.end),
        }
    }
}

// An offset or index as a layout keeps it.
pub(crate) fn narrow(at: usize) -> u32 {
    u32::try_from(at).unwrap_or(u32::MAX)
}

// An offset or index a layout keeps.
pub(crate) fn widen(at: u32) -> usize {
    usize::try_from(at).unwrap_or(usize::MAX)
}
