#[cfg(feature = "std-hash")]
pub mod map {
    pub use std::collections::{HashMap, HashSet};
    pub use std::collections::hash_map::Entry;
}

#[cfg(not(feature = "std-hash"))]
pub mod map {
    pub use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
    pub use std::collections::hash_map::Entry;
}
