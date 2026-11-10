/// The most entries a collection reused every frame held lately, falling by
/// an eighth a frame. The room a burst took, such as a first build's, goes
/// back once later frames hold far fewer, while a collection that fills
/// alike every frame keeps its room.
#[derive(Clone, Copy, Debug, Default)]
pub struct RecentPeak(usize);

impl RecentPeak {
    /// A peak of no entries.
    #[must_use]
    pub const fn new() -> Self {
        Self(0)
    }

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

    /// [`Self::trim`] for a list with room for more than four times `floor`
    /// entries: see [`Self::settle`].
    #[inline]
    pub fn trim_past_floor<T>(&mut self, list: &mut Vec<T>, floor: usize) {
        self.settle(list, list.len(), floor);
    }

    /// Notes a frame at which `list` held `len` entries and gives back the
    /// room the recent frames left unused, keeping its entries and room for
    /// at least `floor`, for a list with room for more than four times
    /// `floor` entries. A list with less room gives back none whatever its
    /// peak, so its frames go unnoted until its room grows past that; the
    /// peak a frame notes then is at most twice `floor` above what noting
    /// every frame would hold.
    #[inline]
    pub fn settle<C: Room>(&mut self, list: &mut C, len: usize, floor: usize) {
        if list.room() > 4 * floor {
            self.settle_roomy(list, len, floor);
        }
    }

    // Out of line, so the checks of many small lists lie together.
    #[cold]
    #[inline(never)]
    fn settle_roomy<C: Room>(&mut self, list: &mut C, len: usize, floor: usize) {
        self.note(len);
        self.shrink(list, floor);
    }

    /// [`Self::settle`] for a hash collection whose entries are spent: one
    /// that gives back room becomes a new, empty collection. A shrink that
    /// kept the entries would bring the code that moves them into every
    /// program for a collection that is empty at this point of its frame.
    #[inline]
    pub fn settle_spent<C: SpentRoom>(&mut self, table: &mut C, len: usize, floor: usize) {
        if table.room() > 4 * floor {
            self.renew_roomy(table, len, floor);
        }
    }

    #[cold]
    #[inline(never)]
    fn renew_roomy<C: SpentRoom>(&mut self, table: &mut C, len: usize, floor: usize) {
        self.note(len);
        self.renew(table, floor);
    }

    /// Gives back the room the recent frames left unused in `table`, a hash
    /// collection whose entries are spent, keeping room for at least
    /// `floor`: see [`Self::settle_spent`].
    #[inline]
    pub fn renew<C: SpentRoom>(self, table: &mut C, floor: usize) {
        if let Some(room) = self.shrink_to(table.room(), floor) {
            table.renew(room);
        }
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

/// A hash collection a [`RecentPeak`] renews once its entries are spent.
pub trait SpentRoom {
    /// How many entries the collection has room for.
    fn room(&self) -> usize;

    /// Replaces the collection with an empty one with room for `room`
    /// entries.
    fn renew(&mut self, room: usize);
}

// A hash map and a hash set renew alike, whatever they hash with.
macro_rules! spent_room {
    ($collection:ident<$($param:ident),*>) => {
        #[expect(
            clippy::disallowed_types,
            reason = "every hash collection renews alike, whatever it hashes with"
        )]
        impl<$($param),*> SpentRoom for std::collections::$collection<$($param),*>
        where
            S: Default,
        {
            fn room(&self) -> usize {
                self.capacity()
            }

            fn renew(&mut self, room: usize) {
                *self = Self::with_capacity_and_hasher(room, S::default());
            }
        }
    };
}

spent_room!(HashMap<K, V, S>);
spent_room!(HashSet<T, S>);

// forwards on purpose: a collection gives back room seldom, and the check
// before it runs for every collection each frame, so the shrink stays out
// of line and out of the checks.
#[cold]
#[inline(never)]
pub(super) fn give_back<C: Room>(list: &mut C, room: usize) {
    list.give_back_to(room);
}
