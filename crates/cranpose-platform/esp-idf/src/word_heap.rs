#![expect(unsafe_code, reason = "calls into ESP-IDF's heap")]

//! The classic ESP32's IRAM heap as the memory the shape columns of
//! recordings grow into while the heap the rest of the app shares runs
//! short. Only 32-bit loads and stores reach IRAM, so no byte data of the
//! app can live there, and each column it takes leaves its bytes to that
//! heap. A column there copies its items a word at a time, which costs the
//! classic's busy screen 0.05 ms a frame: with room left in the byte heap
//! the columns stay there.

use cranpose::framebuffer::{WordHeap, set_word_heap, set_word_heap_open};
use esp_idf_svc::sys;

// Where SRAM0, which only the instruction bus and 32-bit data loads reach,
// starts: its first 64 KB are the CPUs' caches, of which a single-core build
// gives the second core's 32 KB to the heap.
const SRAM0_LOW: usize = 0x4007_0000;

// Whether the chip is the classic ESP32. The S3 and later chips have no
// memory that takes only 32-bit access, and their builds fold the word heap
// away.
fn classic() -> bool {
    sys::CONFIG_IDF_TARGET.as_slice() == b"esp32\0"
}

// Names the classic ESP32's IRAM heap as the word heap.
pub(crate) fn install() {
    if classic()
        && !set_word_heap(WordHeap {
            alloc: iram_words,
            free: free_words,
        })
    {
        log::warn!("the shape columns keep the word heap named before the IRAM heap");
    }
}

// Opens the word heap while the byte heap, `free` of `total` bytes at a
// frame, has less than an eighth free: under the quarter at which lazy
// lists keep only what they show, so a screen that runs at that edge, as
// the busy screen does, keeps its columns in the byte heap.
pub(crate) fn note_heap(free: usize, total: usize) {
    if classic() {
        set_word_heap_open(free.saturating_mul(8) < total);
    }
}

// `words` words of IRAM, or null when it has no room. An executable
// allocation may also come from the RAM both buses reach, which byte data
// could use, so one from there goes back.
unsafe fn iram_words(words: usize) -> *mut u32 {
    let Some(bytes) = words.checked_mul(4) else {
        return std::ptr::null_mut();
    };
    // SAFETY: the heap call takes its lock; any task may call it.
    let block =
        unsafe { sys::heap_caps_malloc(bytes, sys::MALLOC_CAP_EXEC | sys::MALLOC_CAP_32BIT) };
    let address = block as usize;
    if block.is_null() || (SRAM0_LOW..sys::SOC_DIRAM_IRAM_LOW as usize).contains(&address) {
        return block.cast();
    }
    // SAFETY: the heap returned `block` just now.
    unsafe { sys::heap_caps_free(block) };
    std::ptr::null_mut()
}

// Frees words `iram_words` returned.
unsafe fn free_words(words: *mut u32) {
    // SAFETY: the caller passes a block of ESP-IDF's heap.
    unsafe { sys::heap_caps_free(words.cast()) };
}
