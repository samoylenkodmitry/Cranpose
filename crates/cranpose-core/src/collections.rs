#[cfg(feature = "std-hash")]
pub mod map {
    pub use std::collections::{HashMap, HashSet, hash_map::Entry};
}

#[cfg(not(feature = "std-hash"))]
pub mod map {
    pub use std::collections::hash_map::Entry;

    pub use ahash::{AHashMap as HashMap, AHashSet as HashSet};
}
