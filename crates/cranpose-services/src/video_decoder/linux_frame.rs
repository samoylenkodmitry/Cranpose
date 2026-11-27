//! Linux decoded video frame backing storage: zero-copy VA-API DMABUF planes or pooled software
//! bi-planar YUV buffers.
#![expect(
    unsafe_code,
    reason = "DMABUF file descriptors and libavutil AVFrame handles are OS/C FFI resources"
)]

use std::{
    ffi::c_void,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use parking_lot::Mutex;

// Count of decoded pictures currently held by the queue or the application.
pub(super) type LivePictures = Arc<AtomicUsize>;

// Maximum pooled CPU buffers kept for software-decoded fallback frames.
const MAX_SOFTWARE_SLOTS: usize = 8;

/// Bi-planar pixel format of a Linux decoded [`VideoFrame`](super::VideoFrame).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinuxPixelFormat {
    /// 8-bit 4:2:0 bi-planar YUV (`DRM_FORMAT_NV12`: `R8` luma + `GR88` chroma).
    Nv12,
    /// 10-bit 4:2:0 bi-planar YUV (`DRM_FORMAT_P010`: upper 10 bits of `R16` luma + `GR1616` chroma).
    P010,
}

/// One exported DMABUF plane of a hardware-decoded Linux picture.
///
/// `fd` remains open and valid for as long as the owning [`VideoFrame`](super::VideoFrame) lives;
/// importers duplicate it (for example with `F_DUPFD_CLOEXEC`) when transferring ownership to Vulkan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaBufPlane {
    /// Borrowed DMABUF file descriptor for this plane.
    pub fd: i32,
    /// Byte offset of this plane inside the DMABUF object.
    pub offset: u64,
    /// Row stride in bytes.
    pub stride: u64,
    /// DRM format modifier (`DRM_FORMAT_MOD_*`) describing GPU tiling.
    pub modifier: u64,
    /// Width of this plane in texels.
    pub width: u32,
    /// Height of this plane in texels.
    pub height: u32,
}

/// Zero-copy VA-API exported DMABUF descriptor for a bi-planar NV12 or P010 picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaBufFrame {
    /// Luma (`Y`) plane (`R8Unorm` for NV12, `R16Unorm` for P010).
    pub luma: DmaBufPlane,
    /// Interleaved chroma (`UV`) plane (`Rg8Unorm` for NV12, `Rg16Unorm` for P010).
    pub chroma: DmaBufPlane,
    /// Pixel bit-depth layout of both planes.
    pub format: LinuxPixelFormat,
}

/// Borrowed CPU bi-planar YUV planes from an explicit software-decode fallback frame.
#[derive(Clone, Copy, Debug)]
pub struct SoftwareBiPlanarPlanes<'a> {
    /// Tightly or padded row-major luma plane bytes.
    pub luma: &'a [u8],
    /// Luma row stride in bytes.
    pub luma_stride: u32,
    /// Tightly or padded row-major interleaved chroma (`UV`) plane bytes.
    pub chroma: &'a [u8],
    /// Chroma row stride in bytes.
    pub chroma_stride: u32,
    /// Pixel bit-depth layout of both planes.
    pub format: LinuxPixelFormat,
}

/// How a Linux [`VideoFrame`](super::VideoFrame) stores its decoded pixels.
#[derive(Clone, Copy, Debug)]
pub enum LinuxFrameMemory<'a> {
    /// Zero-copy GPU DMABUF planes exported directly from VA-API hardware decode.
    DmaBuf(DmaBufFrame),
    /// Explicit software-decoded bi-planar YUV planes (when VA-API hardware decode is absent).
    Software(SoftwareBiPlanarPlanes<'a>),
}

// Owns one cloned `AVFrame` keeping the underlying VA-API surface pinned until GPU reads finish.
pub(super) struct AvFramePin {
    frame: *mut c_void,
    free_fn: unsafe extern "C" fn(*mut *mut c_void),
}

// SAFETY: `AVFrame` reference counts are thread-safe atomics inside `libavutil`.
unsafe impl Send for AvFramePin {}
// SAFETY: `AvFramePin` only exposes immutable destruction on drop.
unsafe impl Sync for AvFramePin {}

impl AvFramePin {
    // SAFETY: `frame` must be an owned `AVFrame*` allocated by `libavutil`, freed by `free_fn`.
    pub(super) unsafe fn new(
        frame: *mut c_void,
        free_fn: unsafe extern "C" fn(*mut *mut c_void),
    ) -> Self {
        Self { frame, free_fn }
    }
}

impl Drop for AvFramePin {
    fn drop(&mut self) {
        if !self.frame.is_null() {
            // SAFETY: frees the owned `AVFrame` reference, returning the VA-API surface to the pool.
            unsafe { (self.free_fn)(&mut self.frame) };
        }
    }
}

// Reusable CPU byte storage for software-decoded fallback frames (zero steady-state allocation).
pub(super) struct SoftwareSlot {
    pub(super) luma: Vec<u8>,
    pub(super) luma_stride: u32,
    pub(super) chroma: Vec<u8>,
    pub(super) chroma_stride: u32,
    pub(super) format: LinuxPixelFormat,
}

pub(super) type SoftwarePool = Arc<Mutex<Vec<SoftwareSlot>>>;

enum Storage {
    DmaBuf {
        fds: [i32; 4],
        fd_count: usize,
        desc: DmaBufFrame,
        _pin: AvFramePin,
    },
    Software {
        slot: Option<SoftwareSlot>,
        recycler: SoftwarePool,
    },
}

pub(super) struct Image {
    storage: Storage,
    live: LivePictures,
}

impl Image {
    pub(super) fn from_dmabuf(
        fds: [i32; 4],
        fd_count: usize,
        desc: DmaBufFrame,
        pin: AvFramePin,
        live: &LivePictures,
    ) -> Self {
        live.fetch_add(1, Ordering::Relaxed);
        Self {
            storage: Storage::DmaBuf {
                fds,
                fd_count,
                desc,
                _pin: pin,
            },
            live: Arc::clone(live),
        }
    }

    pub(super) fn from_software(
        slot: SoftwareSlot,
        recycler: &SoftwarePool,
        live: &LivePictures,
    ) -> Self {
        live.fetch_add(1, Ordering::Relaxed);
        Self {
            storage: Storage::Software {
                slot: Some(slot),
                recycler: Arc::clone(recycler),
            },
            live: Arc::clone(live),
        }
    }

    pub(super) fn memory(&self) -> LinuxFrameMemory<'_> {
        match &self.storage {
            Storage::DmaBuf { desc, .. } => LinuxFrameMemory::DmaBuf(*desc),
            Storage::Software { slot, .. } => {
                let slot = slot.as_ref().expect("slot present until drop");
                LinuxFrameMemory::Software(SoftwareBiPlanarPlanes {
                    luma: &slot.luma,
                    luma_stride: slot.luma_stride,
                    chroma: &slot.chroma,
                    chroma_stride: slot.chroma_stride,
                    format: slot.format,
                })
            }
        }
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        match &mut self.storage {
            Storage::DmaBuf { fds, fd_count, .. } => {
                for &fd in fds.iter().take(*fd_count) {
                    if fd >= 0 {
                        // SAFETY: each exported DMABUF FD is owned by this `Image` until drop.
                        unsafe { libc::close(fd) };
                    }
                }
            }
            Storage::Software { slot, recycler } => {
                if let Some(slot) = slot.take() {
                    let mut pool = recycler.lock();
                    if pool.len() < MAX_SOFTWARE_SLOTS {
                        pool.push(slot);
                    }
                }
            }
        }
        self.live.fetch_sub(1, Ordering::Relaxed);
    }
}

pub(super) fn take_software_slot(
    recycler: &SoftwarePool,
    luma_bytes: usize,
    chroma_bytes: usize,
    luma_stride: u32,
    chroma_stride: u32,
    format: LinuxPixelFormat,
) -> SoftwareSlot {
    let mut slot = recycler.lock().pop().unwrap_or(SoftwareSlot {
        luma: Vec::new(),
        luma_stride,
        chroma: Vec::new(),
        chroma_stride,
        format,
    });
    slot.luma.resize(luma_bytes, 0);
    slot.chroma.resize(chroma_bytes, 0);
    slot.luma_stride = luma_stride;
    slot.chroma_stride = chroma_stride;
    slot.format = format;
    slot
}
