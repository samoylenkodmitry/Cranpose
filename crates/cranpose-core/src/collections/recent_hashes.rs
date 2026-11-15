//! A lossy set of the hashes put in it lately.

use std::num::NonZeroU32;

/// Tags of hashes put lately, each in the slot its hash picks, the newest
/// in place of the one before it: a hash put long ago, or pushed out of its
/// slot by another, reads as never put.
pub struct RecentHashes {
    tags: Vec<Option<NonZeroU32>>,
    // How far a hash shifts down to its top bits, which pick its slot.
    shift: u32,
}

impl RecentHashes {
    /// Holds the tags of `slots` hashes, rounded up to a power of two and
    /// to at least two. The set takes its room at the first put.
    pub const fn new(slots: usize) -> Self {
        let slots = if slots < 2 {
            2
        } else {
            slots.next_power_of_two()
        };
        Self {
            tags: Vec::new(),
            shift: u64::BITS - slots.ilog2(),
        }
    }

    /// Whether `hash` was put lately.
    #[inline]
    pub fn contains(&self, hash: u64) -> bool {
        let (slot, tag) = self.slot_and_tag(hash);
        self.tags.get(slot) == Some(&Some(tag))
    }

    /// Puts `hash` in its slot.
    #[inline]
    pub fn put(&mut self, hash: u64) {
        if self.tags.is_empty() {
            self.take_room();
        }
        let (slot, tag) = self.slot_and_tag(hash);
        if let Some(kept) = self.tags.get_mut(slot) {
            *kept = Some(tag);
        }
    }

    /// Forgets every hash and gives back the room the tags took.
    pub fn clear(&mut self) {
        self.tags = Vec::new();
    }

    // Out of line: only the first put takes the room.
    #[cold]
    #[inline(never)]
    fn take_room(&mut self) {
        self.tags = vec![None; 1 << (u64::BITS - self.shift)];
    }

    // The slot of `hash`, from its top bits, and its tag, from its low bits.
    fn slot_and_tag(&self, hash: u64) -> (usize, NonZeroU32) {
        let slot = usize::try_from(hash >> self.shift).unwrap_or(0);
        let low = u32::try_from(hash & u64::from(u32::MAX)).unwrap_or(0);
        (slot, NonZeroU32::MIN | low)
    }
}
