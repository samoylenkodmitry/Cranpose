//! Giving back the room a collection's growth left past its items: a
//! collection that lives as long as a screen and doubled to take one more
//! item would keep room for as many again.

use std::hash::{BuildHasher, Hash};

const MIN_TRIMMED_SLACK: usize = 16;

/// The entries a list or a set keeps room for at rest, once a burst took
/// more.
pub const ROOM_KEPT_AT_REST: usize = 64;

fn slack_target(len: usize, capacity: usize) -> Option<usize> {
    let slack = capacity.saturating_sub(len);
    (slack > (len / 4).max(MIN_TRIMMED_SLACK)).then_some(len + len / 8)
}

/// A collection that can give back the room its growth left past its items.
pub trait GrowthSlack {
    /// Gives back the room past the items once it is more than a quarter of
    /// them, and 16 at least, down to an eighth of them.
    fn trim_growth_slack(&mut self);
}

impl<T> GrowthSlack for Vec<T> {
    fn trim_growth_slack(&mut self) {
        if let Some(target) = slack_target(self.len(), self.capacity()) {
            self.shrink_to(target);
        }
    }
}

#[expect(
    clippy::disallowed_types,
    reason = "every hash map trims alike, whatever it hashes with"
)]
impl<K: Eq + Hash, V, S: BuildHasher> GrowthSlack for std::collections::HashMap<K, V, S> {
    fn trim_growth_slack(&mut self) {
        // A map rounds its buckets up to a power of two, which leaves room
        // past the items: asked for room past them, it may keep twice that.
        if slack_target(self.len(), self.capacity()).is_some() {
            self.shrink_to(self.len());
        }
    }
}

/// A collection that keeps room for a few entries at rest.
pub trait RoomAtRest {
    /// Gives back the room past the items and past [`ROOM_KEPT_AT_REST`]
    /// entries.
    fn keep_room_at_rest(&mut self);
}

// The trims at rest run out of line: one copy for each collection type
// serves every collection of it.
impl<T> RoomAtRest for Vec<T> {
    #[inline(never)]
    fn keep_room_at_rest(&mut self) {
        self.shrink_to(ROOM_KEPT_AT_REST);
    }
}

#[expect(
    clippy::disallowed_types,
    reason = "every hash map trims alike, whatever it hashes with"
)]
impl<K: Eq + Hash, V, S: BuildHasher> RoomAtRest for std::collections::HashMap<K, V, S> {
    #[inline(never)]
    fn keep_room_at_rest(&mut self) {
        self.shrink_to(ROOM_KEPT_AT_REST);
    }
}

#[expect(
    clippy::disallowed_types,
    reason = "every hash set trims alike, whatever it hashes with"
)]
impl<T: Eq + Hash, S: BuildHasher> RoomAtRest for std::collections::HashSet<T, S> {
    #[inline(never)]
    fn keep_room_at_rest(&mut self) {
        self.shrink_to(ROOM_KEPT_AT_REST);
    }
}
