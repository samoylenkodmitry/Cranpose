#[cfg(feature = "std-hash")]
pub mod map {
    pub use std::collections::hash_map::Entry;
    pub use std::collections::{HashMap, HashSet};
}

#[cfg(not(feature = "std-hash"))]
pub mod map {
    pub use ahash::{AHashMap as HashMap, AHashSet as HashSet};
    pub use std::collections::hash_map::Entry;
}
