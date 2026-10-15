#![expect(unsafe_code)]

//! The global allocator: blocks of up to 128 bytes come from lists each task
//! keeps, larger ones from ESP-IDF's heap. See
//! `components/cranpose_small_alloc/small_alloc.c`.

use std::alloc::{GlobalAlloc, Layout};

unsafe extern "C" {
    fn cranpose_small_alloc(size: usize, align: usize) -> *mut u8;
    fn cranpose_small_free(ptr: *mut u8, size: usize, align: usize);
    fn cranpose_small_realloc(
        ptr: *mut u8,
        old_size: usize,
        align: usize,
        new_size: usize,
    ) -> *mut u8;
}

/// Serves blocks of up to 128 bytes and an alignment of up to 8 from lists
/// each task keeps of the blocks it freed, without the heap's lock, and every
/// other block from ESP-IDF's heap.
pub struct SmallBlockAllocator;

// SAFETY: every block comes from ESP-IDF's heap with at least the layout's
// size and alignment, and the C side gives each one back by the size and
// alignment it was allocated with, which `GlobalAlloc` passes again.
unsafe impl GlobalAlloc for SmallBlockAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { cranpose_small_alloc(layout.size(), layout.align()) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { cranpose_small_free(ptr, layout.size(), layout.align()) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe { cranpose_small_realloc(ptr, layout.size(), layout.align(), new_size) }
    }
}
