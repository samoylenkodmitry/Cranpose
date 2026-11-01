//! Memory that takes only aligned 32-bit loads and stores, which no byte
//! data of an app can use, such as the classic ESP32's IRAM heap. A platform
//! names it with [`set_word_heap`] and opens it with [`set_word_heap_open`];
//! on the classic ESP32 the shape columns of recordings that grow while it
//! is open take it while it has room.

#![cfg_attr(
    word_heap,
    expect(
        unsafe_code,
        reason = "the words come from an allocator the platform names"
    )
)]

#[cfg(word_heap)]
use std::{
    ptr::NonNull,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

/// An allocator of memory that takes only aligned 32-bit loads and stores.
#[derive(Clone, Copy, Debug)]
pub struct WordHeap {
    /// Allocates `words` 32-bit words, at least one, 4-byte aligned; null
    /// when the heap has no room for them.
    pub alloc: unsafe fn(words: usize) -> *mut u32,
    /// Frees the words `alloc` returned.
    pub free: unsafe fn(words: *mut u32),
}

#[cfg(word_heap)]
static WORD_HEAP: OnceLock<WordHeap> = OnceLock::new();

#[cfg(word_heap)]
static OPEN: AtomicBool = AtomicBool::new(false);

/// Makes the shape columns of recordings take their room from `heap` while
/// it is open and has room, and from the global allocator after. Returns
/// whether `heap` is the word heap now: only on the classic ESP32, whose
/// columns there read and write their items a 32-bit word at a time, and
/// only at the first call, as a later heap would leave the columns of the
/// first without the allocator that frees them.
pub fn set_word_heap(heap: WordHeap) -> bool {
    #[cfg(word_heap)]
    return WORD_HEAP.set(heap).is_ok();
    #[cfg(not(word_heap))]
    {
        let _ = heap;
        false
    }
}

/// Opens the word heap to the shape columns that grow from now on, or
/// closes it: a column there copies its items a word at a time, so a
/// platform opens it only while the heap the rest of the app shares runs
/// short. Columns already in it stay until they grow with it closed.
pub fn set_word_heap_open(open: bool) {
    #[cfg(word_heap)]
    OPEN.store(open, Ordering::Relaxed);
    #[cfg(not(word_heap))]
    let _ = open;
}

// Whether columns that grow now go to the word heap.
#[cfg(word_heap)]
pub(crate) fn open() -> bool {
    OPEN.load(Ordering::Relaxed)
}

// `words` words, at least one, from the word heap when it has room.
#[cfg(word_heap)]
pub(crate) fn take_words(words: usize) -> Option<NonNull<u32>> {
    let heap = WORD_HEAP.get()?;
    // SAFETY: the platform's allocator takes any word count above zero.
    NonNull::new(unsafe { (heap.alloc)(words.max(1)) })
}

// Frees `block`, which `take_words` returned.
//
// # Safety
//
// `block` came from `take_words` and is not used after.
#[cfg(word_heap)]
pub(crate) unsafe fn give_words(block: NonNull<u32>) {
    let heap = WORD_HEAP.get().expect("a word heap block's heap");
    // SAFETY: the word heap returned `block`.
    unsafe { (heap.free)(block.as_ptr()) }
}
