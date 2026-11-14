#![expect(unsafe_code, reason = "implements the global allocator")]

//! The global allocator a web app installs: the standard library's, with the
//! WebAssembly memory grown ahead of need.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

// The bytes of a WebAssembly page.
const PAGE: usize = 64 * 1024;
// The alignment of the block taken to grow ahead: the standard library's
// allocator places it in its heap like any other block.
const AHEAD_ALIGN: usize = 16;

/// The global allocator of a Cranpose web app.
///
/// The standard library's allocator grows the WebAssembly memory a page of
/// 64 KiB at a time as its heap fills, and each growth costs the browser
/// tens of microseconds: the still wall at tier 20 grew its memory 3,903
/// times while composing its first screen, 205 ms in Chrome on an Apple M3
/// Pro. Each time the memory grows, `Heap` takes a block of half the memory
/// from the standard library's allocator and gives it back at once, so the
/// heap holds it free for the allocations that follow: the same screen grew
/// the memory 26 times. The memory ends at most half again as large as the
/// heap needs; the browser backs only the pages written to.
///
/// An app installs it in its WebAssembly build:
///
/// ```ignore
/// #[global_allocator]
/// static HEAP: cranpose::web::Heap = cranpose::web::Heap::new();
/// ```
#[derive(Default)]
pub struct Heap {
    // The memory's size in pages when the heap last grew it ahead.
    pages: AtomicUsize,
}

impl Heap {
    /// A heap that grows the WebAssembly memory ahead from its first growth.
    pub const fn new() -> Self {
        Self {
            pages: AtomicUsize::new(0),
        }
    }

    // Takes and gives back a block of half the memory once the standard
    // library's allocator grew it.
    fn grow_ahead(&self) {
        let pages = core::arch::wasm32::memory_size(0);
        if pages == self.pages.load(Ordering::Relaxed) {
            return;
        }
        if let Ok(ahead) = Layout::from_size_align(pages * PAGE / 2, AHEAD_ALIGN) {
            // SAFETY: the layout's size is not zero, and the block goes back
            // to the allocator it came from with the same layout.
            let block = unsafe { System.alloc(ahead) };
            if !block.is_null() {
                // SAFETY: `block` came from `System.alloc(ahead)` above.
                unsafe { System.dealloc(block, ahead) };
            }
        }
        self.pages
            .store(core::arch::wasm32::memory_size(0), Ordering::Relaxed);
    }
}

// SAFETY: every block comes from the standard library's allocator with the
// caller's layout and goes back to it with the same layout; growing ahead
// only takes and gives back a block of its own.
unsafe impl GlobalAlloc for Heap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's layout, as `GlobalAlloc::alloc` requires.
        let block = unsafe { System.alloc(layout) };
        self.grow_ahead();
        block
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's layout, as `GlobalAlloc::alloc_zeroed` requires.
        let block = unsafe { System.alloc_zeroed(layout) };
        self.grow_ahead();
        block
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from this allocator, which takes every block
        // from `System`, with `layout`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: `ptr` came from this allocator, which takes every block
        // from `System`, with `layout`; `new_size` is the caller's.
        let block = unsafe { System.realloc(ptr, layout, new_size) };
        self.grow_ahead();
        block
    }
}
