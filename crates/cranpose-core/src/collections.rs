#[cfg(feature = "std-hash")]
pub mod map {
    pub use std::collections::{hash_map::Entry, HashMap, HashSet};
}

#[cfg(not(feature = "std-hash"))]
pub mod map {
    pub use std::collections::hash_map::Entry;

    pub use ahash::{AHashMap as HashMap, AHashSet as HashSet};
}
