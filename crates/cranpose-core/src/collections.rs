//! Collections the framework's crates share: the hash map every crate
//! uses, the bounded caches their hot paths keep, and the box that holds
//! the properties few values set.

/// A least-recently-used cache with a fixed bound on its entry count.
pub mod bounded_lru;
pub mod pass_aged;
pub mod perfect_map;
pub mod rare;

mod word_hash;

pub use word_hash::WordHasher;

/// What [`FixedHashMap`] hashes with: foldhash on 64-bit targets and
/// [`WordHasher`] on 32-bit ones, where a 64-bit multiply is a long
/// sequence of 32-bit ones.
#[cfg(not(target_pointer_width = "32"))]
pub type FixedState = foldhash::fast::FixedState;

/// What [`FixedHashMap`] hashes with: foldhash on 64-bit targets and
/// [`WordHasher`] on 32-bit ones, where a 64-bit multiply is a long
/// sequence of 32-bit ones.
#[cfg(target_pointer_width = "32")]
pub type FixedState = std::hash::BuildHasherDefault<WordHasher>;

#[cfg(not(target_pointer_width = "32"))]
const FIXED_STATE: FixedState = foldhash::fast::FixedState::with_seed(0);

#[cfg(target_pointer_width = "32")]
const FIXED_STATE: FixedState = std::hash::BuildHasherDefault::new();

/// A hash map with a fixed hash seed. Unlike [`map::HashMap`], an empty one
/// can be made in a `const` context, which a `thread_local!` initializer
/// needs to stay eagerly initialized.
pub type FixedHashMap<K, V> = std::collections::HashMap<K, V, FixedState>;

/// The set counterpart of [`FixedHashMap`].
pub type FixedHashSet<K> = std::collections::HashSet<K, FixedState>;

/// An empty [`FixedHashMap`].
pub const fn fixed_hash_map<K, V>() -> FixedHashMap<K, V> {
    std::collections::HashMap::with_hasher(FIXED_STATE)
}

/// An empty [`FixedHashSet`].
pub const fn fixed_hash_set<K>() -> FixedHashSet<K> {
    std::collections::HashSet::with_hasher(FIXED_STATE)
}

/// Hash map and set types, backed by the standard library's default hasher.
#[cfg(feature = "std-hash")]
pub mod map {
    pub use std::collections::{
        HashMap, HashSet,
        hash_map::{Entry, RandomState},
    };
}

/// Hash map and set types, backed by a fast non-cryptographic hasher:
/// foldhash on 64-bit targets.
#[cfg(all(not(feature = "std-hash"), not(target_pointer_width = "32")))]
pub mod map {
    pub use std::collections::hash_map::Entry;

    pub use foldhash::{HashMap, HashSet, fast::RandomState};
}

/// Hash map and set types, backed by a fast non-cryptographic hasher:
/// [`WordHasher`](super::WordHasher) on 32-bit targets. Every map hashes
/// alike, so a new one costs no seed.
#[cfg(all(not(feature = "std-hash"), target_pointer_width = "32"))]
pub mod map {
    pub use std::collections::hash_map::Entry;

    /// What the maps of this module hash with.
    pub type RandomState = std::hash::BuildHasherDefault<super::WordHasher>;

    /// A hash map hashed by [`RandomState`].
    pub type HashMap<K, V> = std::collections::HashMap<K, V, RandomState>;

    /// A hash set hashed by [`RandomState`].
    pub type HashSet<K> = std::collections::HashSet<K, RandomState>;
}
