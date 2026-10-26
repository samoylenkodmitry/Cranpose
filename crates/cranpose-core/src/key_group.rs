use std::hash::{Hash, Hasher};

use crate::{Key, location_key, slot, with_current_composer};

fn key_scoped<K: Hash, R>(
    key: &K,
    caller: &'static std::panic::Location<'static>,
    content: impl FnOnce() -> R,
) -> R {
    let seed = explicit_group_key_seed(key, caller);
    with_current_composer(|composer| composer.with_group_seed(seed, |_| content()))
}

/// Runs `content` in a group identified by `key`, so state and nodes follow the key when siblings
/// move. Like `key` for a single key and no return value.
#[track_caller]
pub fn with_key<K: Hash>(key: &K, content: impl FnOnce()) {
    key_scoped(key, std::panic::Location::caller(), content);
}

/// Scopes composition identity to `keys` for the duration of `content`.
///
/// Mirrors Jetpack Compose's `key(vararg keys) { content }`. A call keeps its
/// identity by call-site position alone by default, so a slot survives a
/// recomposition even when what a caller passes in changes; wrapping the
/// call in `key(keys, || ...)` folds `keys` into that identity instead, so
/// changing `keys` discards the previous state and starts fresh, and two
/// `key` calls at the same call site with different `keys` never share
/// state. Pass a tuple to key on more than one value, e.g.
/// `key((a, b), || ...)`.
#[track_caller]
pub fn key<K: Hash, R>(keys: K, content: impl FnOnce() -> R) -> R {
    key_scoped(&keys, std::panic::Location::caller(), content)
}

pub(crate) fn hash_key<K: Hash>(key: &K) -> Key {
    let mut hasher = crate::hash::default::new();
    key.hash(&mut hasher);
    Key(hasher.finish())
}

pub(crate) fn explicit_group_key_seed<K: Hash>(
    key: &K,
    caller: &'static std::panic::Location<'static>,
) -> slot::GroupKeySeed {
    let source_key = location_key(caller.file(), caller.line(), caller.column());
    let explicit_key = hash_key(key);
    slot::GroupKeySeed::keyed(source_key, explicit_key)
}
