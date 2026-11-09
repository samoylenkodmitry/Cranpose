// A scope's yes-or-no states. Each takes a bit of `ScopeFlags`.
#[derive(Clone, Copy)]
pub(super) enum ScopeFlag {
    Invalid,
    Enqueued,
    Active,
    ComposedOnce,
    PendingRecompose,
    // A hand-written recompose closure is running: it is out of
    // `recompose_closure` meanwhile, and the scope still recomposes what
    // reads in it.
    Recomposing,
    ForceReuse,
    ForceRecompose,
    Derivation,
    // Something other than a state invalidated the scope.
    UnknownInvalidationSource,
    // The scope's first body waits for a later frame, in pausable content.
    Paused,
    // The scope belongs to pausable content that has not completed: an
    // invalidation waits until it completes.
    Pending,
}

impl ScopeFlag {
    pub(super) const fn bit(self) -> u16 {
        match self {
            Self::Invalid => 1,
            Self::Enqueued => 1 << 1,
            Self::Active => 1 << 2,
            Self::ComposedOnce => 1 << 3,
            Self::PendingRecompose => 1 << 4,
            Self::Recomposing => 1 << 5,
            Self::ForceReuse => 1 << 6,
            Self::ForceRecompose => 1 << 7,
            Self::Derivation => 1 << 8,
            Self::UnknownInvalidationSource => 1 << 9,
            Self::Paused => 1 << 10,
            Self::Pending => 1 << 11,
        }
    }
}
