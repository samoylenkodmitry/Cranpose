//! How much memory a device has left, as its host reports it.

/// How much memory the host reports it has left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MemoryHeadroom {
    /// Enough for what caches and lazy layouts keep ready. A host that
    /// reports nothing has it.
    #[default]
    Ample,
    /// Little: a lazy layout keeps only the items it shows, and its reuse
    /// pool one more; caches shrink to their least.
    Tight,
}

impl MemoryHeadroom {
    /// The headroom of a heap with `free` of its `total` bytes left, coming
    /// from `self`: tight below a quarter free, and ample again only above
    /// three eighths, so the items a tight list lets go do not make it ample
    /// the next frame.
    pub fn of_heap(self, free: usize, total: usize) -> Self {
        match self {
            Self::Ample if free.saturating_mul(4) < total => Self::Tight,
            Self::Tight if free.saturating_mul(8) > total.saturating_mul(3) => Self::Ample,
            same => same,
        }
    }
}
