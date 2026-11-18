use cranpose_core::collections::recent_hashes::RecentHashes;

// The tags the set starts with: 512 bytes.
const FIRST_SLOTS: usize = 128;

// The texts changed in place or drawn lately, by hash, which the shared
// cache keeps once they come back. Its room grows with the texts it was
// given, to as many as the cache holds, so a text is remembered about as
// long as the cache could keep it, and an app of a few texts keeps a few
// tags. It forgets on each growth and at rest.
pub(super) struct ReturningTexts {
    hashes: RecentHashes,
    slots: usize,
    // The most slots it grows to: the texts the shared cache holds.
    most: usize,
    // Hashes put since it last took its room.
    puts: usize,
}

impl ReturningTexts {
    pub(super) const fn new(most: usize) -> Self {
        let slots = if FIRST_SLOTS < most {
            FIRST_SLOTS
        } else {
            most
        };
        Self {
            hashes: RecentHashes::new(slots),
            slots,
            most,
            puts: 0,
        }
    }

    // Whether `hash` was put lately.
    pub(super) fn contains(&self, hash: u64) -> bool {
        self.hashes.contains(hash)
    }

    // Puts `hash`; a set given more texts than it has slots takes twice
    // the room first, up to its most.
    pub(super) fn put(&mut self, hash: u64) {
        if self.slots < self.most {
            self.puts += 1;
            if self.puts > self.slots {
                self.grow();
            }
        }
        self.hashes.put(hash);
    }

    // Out of line: a set grows a few times in an app's life.
    #[cold]
    #[inline(never)]
    fn grow(&mut self) {
        self.slots = (self.slots * 2).min(self.most);
        self.hashes = RecentHashes::new(self.slots);
        self.puts = 0;
    }

    // Forgets every text and gives back the room past its first.
    pub(super) fn clear(&mut self) {
        *self = Self::new(self.most);
    }
}
