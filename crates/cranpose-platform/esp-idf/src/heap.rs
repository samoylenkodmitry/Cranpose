#![expect(unsafe_code)]

//! What the heap has left of the internal RAM, for the frame statistics.

use esp_idf_svc::sys;

/// The heap's internal byte-addressable RAM, in bytes.
pub(crate) struct HeapReport {
    /// Free now.
    pub(crate) free: usize,
    /// The largest free block: the longest allocation that can succeed.
    pub(crate) largest: usize,
    /// The least that was free since startup.
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
