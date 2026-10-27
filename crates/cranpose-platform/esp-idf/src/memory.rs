#![expect(unsafe_code, reason = "calls into ESP-IDF and its drivers")]

use std::ptr;

use esp_idf_svc::sys;

// The heap's internal byte-addressable RAM in bytes: free now, in its largest
// free block, and the least free since startup; and the bytes of the calling
// task's stack that it never used, which tell how far its stack size can go
// down.
pub(crate) struct MemoryReport {
    pub(crate) heap_free: usize,
    pub(crate) heap_largest: usize,
    pub(crate) heap_lowest: usize,
    pub(crate) stack_unused: usize,
}

impl MemoryReport {
    pub(crate) fn now() -> Self {
        const CAPS: u32 = sys::MALLOC_CAP_INTERNAL | sys::MALLOC_CAP_8BIT;
        // SAFETY: the heap queries take the heap's lock and read only its own
        // state; any task may call them. A null task handle names the calling
        // task, whose stack outlives the call.
        unsafe {
            Self {
                heap_free: sys::heap_caps_get_free_size(CAPS),
                heap_largest: sys::heap_caps_get_largest_free_block(CAPS),
                heap_lowest: sys::heap_caps_get_minimum_free_size(CAPS),
                stack_unused: sys::uxTaskGetStackHighWaterMark(ptr::null_mut()) as usize,
            }
        }
    }
}

// The bytes the heap that takes the app's allocations has free and in all:
// internal RAM, and PSRAM where the chip has it.
pub(crate) fn heap_bytes() -> (usize, usize) {
    // SAFETY: as in `MemoryReport::now`, the queries read only the heap's own
    // state under its lock.
    unsafe {
        (
            sys::heap_caps_get_free_size(sys::MALLOC_CAP_DEFAULT),
            sys::heap_caps_get_total_size(sys::MALLOC_CAP_DEFAULT),
        )
    }
}
