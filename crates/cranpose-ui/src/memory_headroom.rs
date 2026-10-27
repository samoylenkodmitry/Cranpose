//! How much memory the device has left, as its host reports it. Lazy layouts
//! read it to decide how many items they keep composed beyond what they show.

/// How much memory the host reports it has left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MemoryHeadroom {
    /// Enough for items kept ready beyond a lazy layout's viewport. A host
    /// that reports nothing has it.
    #[default]
    Ample,
    /// Little: a lazy layout keeps only the items it shows, and its reuse
    /// pool one more.
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

/// The memory the host of the current shell last reported it has left:
/// [`MemoryHeadroom::Ample`] until it reports.
pub fn current_memory_headroom() -> MemoryHeadroom {
    crate::render_state::current_app_context()
        .map_or(MemoryHeadroom::Ample, |context| context.memory_headroom())
}

/// Sets the memory the current shell's host has left. Lazy layouts read it on
/// their next measure. Call it on the shell's thread, inside its context.
pub fn set_memory_headroom(headroom: MemoryHeadroom) {
    crate::render_state::require_current_app_context("a memory headroom report")
        .set_memory_headroom(headroom);
}
