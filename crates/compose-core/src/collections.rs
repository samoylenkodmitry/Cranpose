#[cfg(feature = "std-hash")]
pub mod map {
    pub use std::collections::hash_map::Entry;
    pub use std::collections::{HashMap, HashSet};
}

#[cfg(not(feature = "std-hash"))]
pub mod map {
    pub use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
    pub use std::collections::hash_map::Entry;
}
