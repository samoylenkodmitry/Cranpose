#[cfg(feature = "std-hash")]
pub mod default {
    pub use std::collections::hash_map::DefaultHasher;

    #[inline]
    pub fn new() -> DefaultHasher {
        DefaultHasher::new()
    }
}

#[cfg(not(feature = "std-hash"))]
pub mod default {
    use std::hash::BuildHasher;

    pub type DefaultHasher = foldhash::fast::FoldHasher<'static>;

    #[inline]
    pub fn new() -> DefaultHasher {
        foldhash::fast::FixedState::default().build_hasher()
    }
}
