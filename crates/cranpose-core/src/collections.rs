//! Collections the framework's crates share: the hash map every crate
//! uses, and the bounded caches their hot paths keep.

pub mod bounded_lru;
pub mod pass_aged;

#[cfg(feature = "std-hash")]
pub mod map {
    pub use std::collections::{
        HashMap, HashSet,
        hash_map::{Entry, RandomState},
    };
}

#[cfg(not(feature = "std-hash"))]
pub mod map {
    pub use std::collections::hash_map::Entry;

    pub use foldhash::{HashMap, HashSet, fast::RandomState};
}
