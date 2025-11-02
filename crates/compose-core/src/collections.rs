#[cfg(feature = "std-hash")]
pub mod map {
    pub use std::collections::{HashMap, HashSet};
    pub use std::collections::hash_map::Entry;
}

#[cfg(not(feature = "std-hash"))]
pub mod map {
    pub use hashbrown::{HashMap, HashSet};
    pub use hashbrown::hash_map::Entry;
}