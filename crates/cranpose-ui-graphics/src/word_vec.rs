//! A list of plain-data items that lives in the global allocator's memory as
//! a `Vec`, or while the word heap is open (see [`crate::set_word_heap`]) in
//! word memory. There an item moves between the block and a copy on the
//! stack a word at a time, by `l32i` and `s32i` written out in assembly on
//! Xtensa, so no byte or float access reaches the block: the classic
//! ESP32's IRAM faults on byte access, and its floating-point loads and
//! stores cannot reach IRAM at all. A word the compiler loads and uses as
//! float bits becomes a float load (`lsi`) on Xtensa, volatile or not, and
//! each volatile access waits on a `memw` there. The copies cost the
//! classic's busy screen 0.05 ms a frame, which a list in the byte heap
//! does not pay.

#![expect(
    unsafe_code,
    reason = "the list owns raw blocks that only word access may reach"
)]

use std::{fmt, marker::PhantomData, mem::MaybeUninit, ptr::NonNull};

use bytemuck::Pod;

use crate::{FxHasher, shape_records::trim_growth, word_heap};

pub(crate) enum WordVec<T: Pod> {
    Bytes(Vec<T>),
    Words(WordBlock<T>),
}

// Items in a block of word memory, which only word access reaches.
pub(crate) struct WordBlock<T: Pod> {
    words: NonNull<u32>,
    len: usize,
    capacity: usize,
    items: PhantomData<T>,
}

// SAFETY: the block owns its words, as a `Vec` owns its items.
unsafe impl<T: Pod + Send> Send for WordBlock<T> {}
// SAFETY: as for `Send`; a shared block is only read.
unsafe impl<T: Pod + Sync> Sync for WordBlock<T> {}

// Copies `count` words from `from` to `to`, each by one 32-bit load and one
// 32-bit store that the compiler can neither see through nor merge.
//
// # Safety
//
// `from` holds and `to` has room for `count` words, all 4-byte aligned, and
// they do not overlap.
unsafe fn copy_words(from: *const u32, to: *mut u32, count: usize) {
    for word in 0..count {
        // SAFETY: both hold word `word`.
        unsafe { store_word(to.add(word), load_word(from.add(word))) };
    }
}

// SAFETY (both): `at` is a 4-byte aligned word the caller may read or write.
#[cfg(target_arch = "xtensa")]
unsafe fn load_word(at: *const u32) -> u32 {
    let word;
    // SAFETY: the caller's.
    unsafe {
        std::arch::asm!("l32i {0}, {1}, 0", out(reg) word, in(reg) at,
            options(nostack, readonly, preserves_flags));
    }
    word
}

#[cfg(target_arch = "xtensa")]
unsafe fn store_word(at: *mut u32, word: u32) {
    // SAFETY: the caller's.
    unsafe {
        std::arch::asm!("s32i {0}, {1}, 0", in(reg) word, in(reg) at,
            options(nostack, preserves_flags));
    }
}

// Host builds of `cfg(word_heap)`, for its tests, have no word memory.
#[cfg(not(target_arch = "xtensa"))]
unsafe fn load_word(at: *const u32) -> u32 {
    // SAFETY: the caller's.
    unsafe { at.read_volatile() }
}

#[cfg(not(target_arch = "xtensa"))]
unsafe fn store_word(at: *mut u32, word: u32) {
    // SAFETY: the caller's.
    unsafe { at.write_volatile(word) }
}

impl<T: Pod> WordBlock<T> {
    const WORDS: usize = {
        assert!(
            size_of::<T>().is_multiple_of(4) && size_of::<T>() > 0,
            "a word item is whole 32-bit words"
        );
        size_of::<T>() / 4
    };

    // An empty block with room for `capacity` items, at least one, when the
    // word heap has it.
    #[inline(never)]
    fn with_room(capacity: usize) -> Option<Self> {
        let words = word_heap::take_words(capacity.checked_mul(Self::WORDS)?)?;
        Some(Self {
            words,
            len: 0,
            capacity,
            items: PhantomData,
        })
    }

    // Appends `item`; the block has room for it. Out of line, as are the
    // other word copies, so that the byte heap's lists inline as a `Vec`'s.
    #[inline(never)]
    fn push(&mut self, item: T) {
        debug_assert!(self.len < self.capacity, "a word block's room");
        // SAFETY: `item` is `WORDS` words, and the block has room for them at
        // `len`.
        unsafe {
            copy_words(
                (&raw const item).cast(),
                self.words.as_ptr().add(self.len * Self::WORDS),
                Self::WORDS,
            );
        }
        self.len += 1;
    }

    // Item `index`, below `len`.
    #[inline(never)]
    fn read(&self, index: usize) -> T {
        debug_assert!(index < self.len, "a word block's item");
        let mut item = MaybeUninit::<T>::uninit();
        // SAFETY: the block holds item `index`, and `item` has room for its
        // words; `T` takes any bits.
        unsafe {
            copy_words(
                self.words.as_ptr().add(index * Self::WORDS),
                item.as_mut_ptr().cast(),
                Self::WORDS,
            );
            item.assume_init()
        }
    }

    fn words(&self) -> &[u32] {
        // SAFETY: the block holds `len` items, all written.
        unsafe { std::slice::from_raw_parts(self.words.as_ptr(), self.len * Self::WORDS) }
    }

    // The block's items in a block with room for `capacity`, at least `len`,
    // when the word heap has it.
    #[inline(never)]
    fn moved(&self, capacity: usize) -> Option<Self> {
        let mut block = Self::with_room(capacity)?;
        // SAFETY: both blocks hold `len` items and do not overlap.
        unsafe {
            copy_words(
                self.words.as_ptr(),
                block.words.as_ptr(),
                self.len * Self::WORDS,
            );
        }
        block.len = self.len;
        Some(block)
    }

    // A copy of the list in word memory when the word heap has room, else in
    // the byte heap.
    #[inline(never)]
    fn cloned(&self) -> WordVec<T> {
        match self.moved(self.len.max(1)) {
            Some(copy) => WordVec::Words(copy),
            None => WordVec::Bytes((0..self.len).map(|index| self.read(index)).collect()),
        }
    }
}

impl<T: Pod> Drop for WordBlock<T> {
    #[inline(never)]
    fn drop(&mut self) {
        // SAFETY: the word heap returned the block, which is not used after.
        unsafe { word_heap::give_words(self.words) };
    }
}

impl<T: Pod> WordVec<T> {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        if capacity != 0
            && word_heap::open()
            && let Some(block) = WordBlock::with_room(capacity)
        {
            return Self::Words(block);
        }
        Self::Bytes(Vec::with_capacity(capacity))
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Bytes(items) => items.len(),
            Self::Words(block) => block.len,
        }
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub(crate) fn capacity(&self) -> usize {
        match self {
            Self::Bytes(items) => items.capacity(),
            Self::Words(block) => block.capacity,
        }
    }

    #[inline]
    pub(crate) fn clear(&mut self) {
        match self {
            Self::Bytes(items) => items.clear(),
            Self::Words(block) => block.len = 0,
        }
    }

    #[inline]
    pub(crate) fn push(&mut self, item: T) {
        if self.len() == self.capacity() && (matches!(self, Self::Words(_)) || word_heap::open()) {
            self.regrow(self.len() + 1);
        }
        match self {
            // A full list here grows as a `Vec` does, the word heap closed.
            Self::Bytes(items) => items.push(item),
            Self::Words(block) => block.push(item),
        }
    }

    pub(crate) fn reserve(&mut self, additional: usize) {
        if let Self::Bytes(items) = self
            && !word_heap::open()
        {
            items.reserve(additional);
        } else if self.capacity() - self.len() < additional {
            self.regrow(self.len() + additional);
        }
    }

    #[inline]
    pub(crate) fn get(&self, index: usize) -> Option<T> {
        match self {
            Self::Bytes(items) => items.get(index).copied(),
            Self::Words(block) => (index < block.len).then(|| block.read(index)),
        }
    }

    pub(crate) fn iter(&self) -> impl ExactSizeIterator<Item = T> + DoubleEndedIterator + '_ {
        (0..self.len()).map(|index| match self {
            Self::Bytes(items) => items[index],
            Self::Words(block) => block.read(index),
        })
    }

    // Hashes the items' bytes; from word memory a word at a time, to the
    // same hash.
    pub(crate) fn hash_into(&self, hasher: &mut FxHasher) {
        match self {
            Self::Bytes(items) => std::hash::Hasher::write(hasher, bytemuck::cast_slice(items)),
            Self::Words(block) => hasher.write_words(block.words()),
        }
    }

    // `shape_records::trim_growth` for this list. A smaller block from the
    // byte heap would take back what word memory gave: without room for a
    // smaller one in word memory, a block there keeps its room.
    pub(crate) fn trim_growth(&mut self) {
        match self {
            Self::Bytes(items) => trim_growth(items),
            Self::Words(_) => self.trim_words(),
        }
    }

    #[inline(never)]
    fn trim_words(&mut self) {
        let Self::Words(block) = self else {
            return;
        };
        let len = block.len;
        if len == 0 {
            *self = Self::default();
        } else if block.capacity > len + len / 4
            && let Some(smaller) = block.moved(len + len / 8)
        {
            *block = smaller;
        }
    }

    // Room for `needed` items, grown as a `Vec` grows: double, at least what
    // is asked, and four items at least. The items go to word memory while
    // the word heap is open and has room, else to the byte heap.
    #[cold]
    fn regrow(&mut self, needed: usize) {
        let capacity = needed.max(self.capacity() * 2).max(4);
        let words = word_heap::open()
            .then(|| match self {
                Self::Bytes(items) => WordBlock::with_room(capacity).map(|mut block| {
                    items.iter().for_each(|&item| block.push(item));
                    block
                }),
                Self::Words(block) => block.moved(capacity),
            })
            .flatten();
        *self = match words {
            Some(block) => Self::Words(block),
            None => {
                let mut items = Vec::with_capacity(capacity);
                items.extend(self.iter());
                Self::Bytes(items)
            }
        };
    }
}

impl<T: Pod> Clone for WordVec<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Bytes(items) => Self::Bytes(items.clone()),
            Self::Words(block) => block.cloned(),
        }
    }
}

impl<T: Pod> Default for WordVec<T> {
    fn default() -> Self {
        Self::Bytes(Vec::new())
    }
}

impl<T: Pod + PartialEq> PartialEq for WordVec<T> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Bytes(items), Self::Bytes(others)) => items == others,
            _ => self.len() == other.len() && self.iter().eq(other.iter()),
        }
    }
}

impl<T: Pod + fmt::Debug> fmt::Debug for WordVec<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}
