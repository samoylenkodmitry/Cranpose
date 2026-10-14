//! Collections the framework's crates share: the hash map every crate
//! uses, the bounded caches their hot paths keep, and the box that holds
//! the properties few values set.

/// A least-recently-used cache with a fixed bound on its entry count.
pub mod bounded_lru;
pub mod pass_aged;
pub mod rare;

/// A hash map with a fixed hash seed. Unlike [`map::HashMap`], an empty one
/// can be made in a `const` context, which a `thread_local!` initializer
/// needs to stay eagerly initialized.
pub type FixedHashMap<K, V> = std::collections::HashMap<K, V, foldhash::fast::FixedState>;

/// The set counterpart of [`FixedHashMap`].
pub type FixedHashSet<K> = std::collections::HashSet<K, foldhash::fast::FixedState>;

/// An empty [`FixedHashMap`].
pub const fn fixed_hash_map<K, V>() -> FixedHashMap<K, V> {
    std::collections::HashMap::with_hasher(foldhash::fast::FixedState::with_seed(0))
}

/// An empty [`FixedHashSet`].
pub const fn fixed_hash_set<K>() -> FixedHashSet<K> {
    std::collections::HashSet::with_hasher(foldhash::fast::FixedState::with_seed(0))
}

/// Hash map and set types, backed by the standard library's default hasher.
#[cfg(feature = "std-hash")]
pub mod map {
    pub use std::collections::{
        HashMap, HashSet,
        hash_map::{Entry, RandomState},
    };
}

/// Hash map and set types, backed by a fast non-cryptographic hasher.
#[cfg(not(feature = "std-hash"))]
pub mod map {
    pub use std::collections::hash_map::Entry;

    pub use foldhash::{HashMap, HashSet, fast::RandomState};
}
