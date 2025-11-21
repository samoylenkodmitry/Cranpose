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
    // fast branch
    pub use ahash::AHasher as DefaultHasher;

    #[inline]
    pub fn new() -> DefaultHasher {
        DefaultHasher::default()
    }
}
