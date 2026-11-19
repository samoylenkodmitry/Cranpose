//! A lossy set of the hashes put in it lately.

use std::num::NonZeroU32;

// The most buckets: a tag holds the top 32 bits of its hash and one bit
// set, so 31 of them pick a bucket.
const MOST_BUCKETS: usize = 1 << 31;

/// Tags of hashes put lately in buckets of `WAYS`, the newest first. The top
/// bits of a hash pick its bucket. A hash put long ago, or pushed out of its
/// bucket by `WAYS` newer ones, reads as never put. A tag is the top half of
/// its hash, so a set that grows moves each tag to the bucket picked by its
/// hash at the new size.
pub struct RecentHashes<const WAYS: usize = 1> {
    buckets: Vec<[Option<NonZeroU32>; WAYS]>,
    // How far a tag shifts down to its top bits, which pick its bucket.
    shift: u32,
}

impl<const WAYS: usize> RecentHashes<WAYS> {
    /// Holds the tags of `slots` hashes, in buckets of `WAYS`: as many
    /// buckets as `slots` fill, rounded up to a power of two, at least two
    /// and at most 2^31. The set takes its room at the first put.
    pub const fn new(slots: usize) -> Self {
        let buckets = slots.div_ceil(WAYS);
        let buckets = if buckets < 2 {
            2
        } else if buckets > MOST_BUCKETS {
            MOST_BUCKETS
        } else {
            buckets.next_power_of_two()
        };
        Self {
            buckets: Vec::new(),
            shift: u32::BITS - buckets.ilog2(),
        }
    }

    /// Whether `hash` was put lately.
    #[inline]
    pub fn contains(&self, hash: u64) -> bool {
        let (bucket, tag) = self.bucket_and_tag(hash);
        self.buckets
            .get(bucket)
            .is_some_and(|tags| tags.contains(&Some(tag)))
    }

    /// Puts `hash` first in its bucket.
    #[inline]
    pub fn put(&mut self, hash: u64) {
        if self.buckets.is_empty() {
            self.take_room();
        }
        let (bucket, tag) = self.bucket_and_tag(hash);
        if let Some(tags) = self.buckets.get_mut(bucket) {
            put_first(tags, tag);
        }
    }

    /// The number of hashes it holds the tags of.
    pub const fn slots(&self) -> usize {
        self.bucket_count() * WAYS
    }

    /// Holds the tags of `slots` hashes from now on, rounded as [`Self::new`]
    /// rounds them, when that is more than it holds. It keeps the hashes put,
    /// each in the bucket its hash picks now, in their order.
    pub fn grow_to(&mut self, slots: usize) {
        let grown = Self::new(slots);
        if grown.shift >= self.shift {
            return;
        }
        self.shift = grown.shift;
        if self.buckets.is_empty() {
            return;
        }
        let room = vec![[None; WAYS]; self.bucket_count()];
        let old = std::mem::replace(&mut self.buckets, room);
        for tag in old.iter().flat_map(|tags| tags.iter().rev().flatten()) {
            let bucket = self.bucket_of(*tag);
            if let Some(tags) = self.buckets.get_mut(bucket) {
                put_first(tags, *tag);
            }
        }
    }

    /// Forgets every hash and gives back the room the tags took.
    pub fn clear(&mut self) {
        self.buckets = Vec::new();
    }

    const fn bucket_count(&self) -> usize {
        1 << (u32::BITS - self.shift)
    }

    // Out of line: only the first put takes the room.
    #[cold]
    #[inline(never)]
    fn take_room(&mut self) {
        self.buckets = vec![[None; WAYS]; self.bucket_count()];
    }

    // The bucket of `hash`, from its top bits, and its tag, its top half.
    fn bucket_and_tag(&self, hash: u64) -> (usize, NonZeroU32) {
        let top = u32::try_from(hash >> u32::BITS).unwrap_or(0);
        let tag = NonZeroU32::MIN | top;
        (self.bucket_of(tag), tag)
    }

    // The bucket of `tag`, from the top bits of its hash, which it holds.
    fn bucket_of(&self, tag: NonZeroU32) -> usize {
        usize::try_from(tag.get() >> self.shift).unwrap_or(0)
    }
}

// Puts `tag` first among `tags` unless they hold it, each tag after it
// moving one way down up to the first free way, the oldest leaving a full
// bucket.
fn put_first<const WAYS: usize>(tags: &mut [Option<NonZeroU32>; WAYS], tag: NonZeroU32) {
    if let [way] = tags.as_mut_slice() {
        *way = Some(tag);
        return;
    }
    if tags.contains(&Some(tag)) {
        return;
    }
    let mut moved = Some(tag);
    for way in tags {
        moved = std::mem::replace(way, moved);
        if moved.is_none() {
            break;
        }
    }
}

#[cfg(test)]
#[path = "tests/recent_hashes_tests.rs"]
mod tests;
