//! Collections the framework's crates share: the hash map every crate
//! uses, the bounded caches their hot paths keep, and the box that holds
//! the properties few values set.

pub mod bounded_lru;
pub mod pass_aged;
pub mod rare;

#[cfg(feature = "std-hash")]
pub mod map {
    pub use std::collections::{HashMap, HashSet, hash_map::Entry};
}

#[cfg(not(feature = "std-hash"))]
pub mod map {
    pub use std::collections::hash_map::Entry;

    pub use foldhash::{HashMap, HashSet};
}
