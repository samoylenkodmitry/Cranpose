/// The framework's default hasher, which is the standard library's default hasher with fixed keys.
#[cfg(feature = "std-hash")]
pub mod default {
    pub use std::collections::hash_map::DefaultHasher;

    /// Creates a hasher that gives the same hash for the same input on every run.
    #[inline]
    pub fn new() -> DefaultHasher {
        DefaultHasher::new()
    }
}

/// The framework's default hasher. It is deterministic and is not suited to hashing untrusted
/// input.
#[cfg(not(feature = "std-hash"))]
pub mod default {
    use std::hash::BuildHasher;

    /// The hasher type that `new` returns.
    pub type DefaultHasher = foldhash::fast::FoldHasher<'static>;

    /// Creates a hasher that gives the same hash for the same input on every run.
    #[inline]
    pub fn new() -> DefaultHasher {
        foldhash::fast::FixedState::default().build_hasher()
    }
}
