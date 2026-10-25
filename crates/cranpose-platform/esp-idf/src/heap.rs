#![expect(unsafe_code)]

use esp_idf_svc::sys;

// The heap's internal byte-addressable RAM in bytes: free now, in its largest
// free block, and the least free since startup.
pub(crate) struct HeapReport {
    pub(crate) free: usize,
    pub(crate) largest: usize,
    pub(crate) lowest: usize,
}

impl HeapReport {
    pub(crate) fn now() -> Self {
        const CAPS: u32 = sys::MALLOC_CAP_INTERNAL | sys::MALLOC_CAP_8BIT;
        // SAFETY: the queries take the heap's lock and read only its own
        // state; any task may call them.
        unsafe {
            Self {
                free: sys::heap_caps_get_free_size(CAPS),
                largest: sys::heap_caps_get_largest_free_block(CAPS),
                lowest: sys::heap_caps_get_minimum_free_size(CAPS),
            }
        }
    }
}
