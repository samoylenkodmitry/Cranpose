use std::hash::{BuildHasher, Hash};

/// The most entries a collection reused every frame held lately, falling by
/// an eighth a frame. The room a burst took, such as a first build's, goes
/// back once later frames hold far fewer, while a collection that fills
/// alike every frame keeps its room.
#[derive(Clone, Copy, Debug, Default)]
pub struct RecentPeak(usize);

impl RecentPeak {
    /// Notes a frame at which the collection held `len` entries.
    #[inline]
    pub fn note(&mut self, len: usize) {
        self.0 = len.max(self.0 - self.0 / 8);
    }

    /// The room to shrink a collection of `capacity` to: twice the peak, at
    /// least `floor`, once the capacity passes four times that.
    #[must_use]
    #[inline]
    pub fn shrink_to(self, capacity: usize, floor: usize) -> Option<usize> {
        let bound = self.0.max(floor);
        (capacity > 4 * bound).then_some(2 * bound)
    }

    /// Gives back the room the recent frames left unused in `list`, a
    /// collection that fills like the one the peak notes, keeping its
    /// entries and room for at least `floor`.
    #[inline]
    pub fn shrink<C: Room>(self, list: &mut C, floor: usize) {
        if let Some(room) = self.shrink_to(list.room(), floor) {
            give_back(list, room);
        }
    }

    /// Notes the frame `list` holds and gives back the room the recent
    /// frames left unused, keeping its entries and room for at least
    /// `floor`.
    #[inline]
    pub fn trim<T>(&mut self, list: &mut Vec<T>, floor: usize) {
        self.note(list.len());
        self.shrink(list, floor);
    }

    /// Notes the frame `list` held, empties it and gives back the room the
    /// recent frames left unused, keeping room for at least `floor`.
    #[inline]
    pub fn clear<T>(&mut self, list: &mut Vec<T>, floor: usize) {
        self.note(list.len());
        list.clear();
        self.shrink(list, floor);
    }
}

/// A collection whose unused room a [`RecentPeak`] gives back.
pub trait Room {
    /// How many entries the collection has room for.
    fn room(&self) -> usize;

    /// Gives back the room past `room` entries, keeping the entries.
    fn give_back_to(&mut self, room: usize);
}

impl<T> Room for Vec<T> {
    fn room(&self) -> usize {
        self.capacity()
    }

    fn give_back_to(&mut self, room: usize) {
        self.shrink_to(room);
    }
}

// A hash map and a hash set give back room alike, whatever they hash with.
macro_rules! hashed_room {
    ($collection:ident<$($param:ident),*> where $($bound:tt)*) => {
        #[expect(
            clippy::disallowed_types,
            reason = "every hash collection gives back room alike, whatever it hashes with"
        )]
        impl<$($param),*> Room for std::collections::$collection<$($param),*>
        where
            $($bound)*
        {
            fn room(&self) -> usize {
                self.capacity()
            }

            fn give_back_to(&mut self, room: usize) {
                self.shrink_to(room);
            }
        }
    };
}

hashed_room!(HashMap<K, V, S> where K: Eq + Hash, S: BuildHasher);
hashed_room!(HashSet<T, S> where T: Eq + Hash, S: BuildHasher);

// forwards on purpose: a collection gives back room seldom, and the check
// before it runs for every collection each frame, so the shrink stays out
// of line and out of the checks.
#[cold]
#[inline(never)]
pub(super) fn give_back<C: Room>(list: &mut C, room: usize) {
    list.give_back_to(room);
}
