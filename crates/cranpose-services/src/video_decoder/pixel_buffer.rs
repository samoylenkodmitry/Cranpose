//! The `CVPixelBuffer` of a decoded picture.
#![expect(
    unsafe_code,
    reason = "a CVPixelBuffer is a reference-counted C object that the picture owns one count of"
)]

use std::{
    ffi::c_void,
    ptr::NonNull,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use objc2_core_foundation::CFRetained;
use objc2_core_video::{CVPixelBuffer, CVPixelBufferGetHeight, CVPixelBufferGetWidth};

use super::CropRect;

// The pictures of a decoder that the decoder or the app holds. The count keeps the app from
// holding every picture that the decoder's pool has.
pub(super) type LivePictures = Arc<AtomicUsize>;

// A decoded picture's buffer. It holds one count of the buffer, which the decoder's pool gets
// back when the picture drops.
pub(super) struct Image {
    buffer: CFRetained<CVPixelBuffer>,
    live: LivePictures,
}

// SAFETY: a CVPixelBuffer's reference count changes atomically, and the decoder never writes
// the pixels of a picture that it has output.
unsafe impl Send for Image {}
// SAFETY: as for Send, and the buffer's size does not change.
unsafe impl Sync for Image {}

impl Image {
    // Counts the picture among the live ones.
    pub(super) fn new(buffer: CFRetained<CVPixelBuffer>, live: &LivePictures) -> Self {
        live.fetch_add(1, Ordering::Relaxed);
        Self {
            buffer,
            live: Arc::clone(live),
        }
    }

    pub(super) fn pixel_buffer(&self) -> *mut c_void {
        NonNull::from(&*self.buffer).as_ptr().cast()
    }

    pub(super) fn size(&self) -> (u32, u32) {
        let width = CVPixelBufferGetWidth(&self.buffer);
        let height = CVPixelBufferGetHeight(&self.buffer);
        (
            u32::try_from(width).unwrap_or(u32::MAX),
            u32::try_from(height).unwrap_or(u32::MAX),
        )
    }

    // The whole buffer: the decoder crops the picture it outputs to its visible size.
    pub(super) fn crop(&self) -> CropRect {
        let (width, height) = self.size();
        CropRect {
            x: 0,
            y: 0,
            width,
            height,
        }
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::Relaxed);
    }
}
