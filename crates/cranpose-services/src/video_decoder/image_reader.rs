//! The reader that turns a decoder's pictures into hardware buffers.
//!
//! The reader is an `AImageReader` of the private format that the GPU samples. The decoder
//! renders into its window. `AImageReader_newWithUsage` and `AImage_getHardwareBuffer` need
//! Android 8, and an app links against Android 7, so the code resolves them when it first needs
//! them.
#![expect(
    unsafe_code,
    reason = "AImageReader and AImage are C APIs, and two of their functions are resolved at run time"
)]

use std::{
    ffi::{CStr, c_void},
    ptr::{self, NonNull},
    sync::{Arc, OnceLock},
};

use ndk::native_window::NativeWindow;
use ndk_sys::{
    AHardwareBuffer, AIMAGE_FORMATS, AImage, AImage_delete, AImage_getCropRect, AImage_getHeight,
    AImage_getTimestamp, AImage_getWidth, AImageCropRect, AImageReader,
    AImageReader_acquireNextImage, AImageReader_delete, AImageReader_getWindow, media_status_t,
};

use super::{CropRect, DecoderError};

// AHARDWAREBUFFER_USAGE_GPU_SAMPLED_IMAGE: the GPU samples the buffer as a texture.
const USAGE_GPU_SAMPLED_IMAGE: u64 = 1 << 8;

type NewWithUsage = unsafe extern "C" fn(
    width: i32,
    height: i32,
    format: i32,
    usage: u64,
    max_images: i32,
    reader: *mut *mut AImageReader,
) -> media_status_t;

type GetHardwareBuffer =
    unsafe extern "C" fn(image: *const AImage, buffer: *mut *mut AHardwareBuffer) -> media_status_t;

// The functions that Android 8 added.
struct Symbols {
    new_with_usage: NewWithUsage,
    get_hardware_buffer: GetHardwareBuffer,
}

fn resolve(library: *mut c_void, name: &CStr) -> *mut c_void {
    // SAFETY: the name is a NUL-terminated string, and the handle is the default scope or a
    // library that dlopen returned.
    unsafe { libc::dlsym(library, name.as_ptr()) }
}

fn symbols() -> Option<&'static Symbols> {
    static SYMBOLS: OnceLock<Option<Symbols>> = OnceLock::new();
    SYMBOLS
        .get_or_init(|| {
            // SAFETY: libmediandk.so is a system library, and the transmutes below give the
            // functions the signatures that the NDK documents.
            let library = unsafe { libc::dlopen(c"libmediandk.so".as_ptr(), libc::RTLD_LAZY) };
            if library.is_null() {
                return None;
            }
            let new_with_usage = resolve(library, c"AImageReader_newWithUsage");
            let get_hardware_buffer = resolve(library, c"AImage_getHardwareBuffer");
            if new_with_usage.is_null() || get_hardware_buffer.is_null() {
                return None;
            }
            Some(Symbols {
                // SAFETY: as above
                new_with_usage: unsafe {
                    std::mem::transmute::<*mut c_void, NewWithUsage>(new_with_usage)
                },
                // SAFETY: as above
                get_hardware_buffer: unsafe {
                    std::mem::transmute::<*mut c_void, GetHardwareBuffer>(get_hardware_buffer)
                },
            })
        })
        .as_ref()
}

// An image reader with a window that a decoder renders into.
pub(super) struct Reader {
    reader: NonNull<AImageReader>,
    window: NativeWindow,
    symbols: &'static Symbols,
}

// SAFETY: an AImageReader is a thread-safe object. Its window is a reference counted native
// window.
unsafe impl Send for Reader {}
// SAFETY: as for Send
unsafe impl Sync for Reader {}

// What an attempt to take the next image found.
pub(super) enum Acquired {
    Image(Image),
    // No picture is ready.
    Empty,
    // The app holds as many images as the reader has.
    Full,
    Failed(i32),
}

impl Reader {
    // Makes a reader for pictures of `width` by `height` pixels that holds up to `depth` images.
    pub(super) fn new(width: u32, height: u32, depth: usize) -> Result<Self, DecoderError> {
        let symbols = symbols().ok_or(DecoderError::Unavailable)?;
        let mut reader = ptr::null_mut();
        // SAFETY: the function has the signature of AImageReader_newWithUsage, and the out
        // pointer names a live local.
        let status = unsafe {
            (symbols.new_with_usage)(
                width as i32,
                height as i32,
                AIMAGE_FORMATS::AIMAGE_FORMAT_PRIVATE.0 as i32,
                USAGE_GPU_SAMPLED_IMAGE,
                depth as i32,
                &mut reader,
            )
        };
        let reader = NonNull::new(reader)
            .filter(|_| status == media_status_t::AMEDIA_OK)
            .ok_or_else(|| DecoderError::Configure(format!("image reader status {}", status.0)))?;
        let mut window = ptr::null_mut();
        // SAFETY: the reader is live. The window belongs to it.
        let status = unsafe { AImageReader_getWindow(reader.as_ptr(), &mut window) };
        let Some(window) = NonNull::new(window).filter(|_| status == media_status_t::AMEDIA_OK)
        else {
            // SAFETY: the reader is live and no image came from it.
            unsafe { AImageReader_delete(reader.as_ptr()) };
            return Err(DecoderError::Configure(
                "the image reader has no window".into(),
            ));
        };
        // SAFETY: the pointer is the reader's window. The clone takes a reference of its own.
        let window = unsafe { NativeWindow::clone_from_ptr(window) };
        Ok(Self {
            reader,
            window,
            symbols,
        })
    }

    pub(super) fn window(&self) -> &NativeWindow {
        &self.window
    }

    // Takes the next image that the decoder rendered. The image keeps the reader alive.
    pub(super) fn acquire(self: &Arc<Self>) -> Acquired {
        let mut image = ptr::null_mut();
        // SAFETY: the reader is live, and the out pointer names a live local.
        let status = unsafe { AImageReader_acquireNextImage(self.reader.as_ptr(), &mut image) };
        if status == media_status_t::AMEDIA_IMGREADER_NO_BUFFER_AVAILABLE {
            return Acquired::Empty;
        }
        if status == media_status_t::AMEDIA_IMGREADER_MAX_IMAGES_ACQUIRED {
            return Acquired::Full;
        }
        let Some(image) = NonNull::new(image).filter(|_| status == media_status_t::AMEDIA_OK)
        else {
            return Acquired::Failed(status.0);
        };
        match Image::new(image, Arc::clone(self)) {
            Some(image) => Acquired::Image(image),
            None => Acquired::Failed(-1),
        }
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        // SAFETY: every image holds an Arc of the reader, so none is live now.
        unsafe { AImageReader_delete(self.reader.as_ptr()) };
    }
}

// An image of a decoded picture. Dropping it gives the image back to the reader.
pub(super) struct Image {
    image: NonNull<AImage>,
    buffer: NonNull<AHardwareBuffer>,
    timestamp_ns: i64,
    size: (u32, u32),
    crop: CropRect,
    // Keeps the reader alive until the image is deleted.
    _reader: Arc<Reader>,
}

// SAFETY: an AImage moves between threads. One thread uses it at a time, because the frame that
// owns it is not Sync.
unsafe impl Send for Image {}

impl Image {
    fn new(image: NonNull<AImage>, reader: Arc<Reader>) -> Option<Self> {
        let raw = image.as_ptr();
        let (mut width, mut height, mut timestamp_ns) = (0, 0, 0);
        let mut crop = AImageCropRect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let mut buffer = ptr::null_mut();
        // SAFETY: the image is live, and the out pointers name live locals.
        let ok = unsafe {
            AImage_getWidth(raw, &mut width) == media_status_t::AMEDIA_OK
                && AImage_getHeight(raw, &mut height) == media_status_t::AMEDIA_OK
                && AImage_getTimestamp(raw, &mut timestamp_ns) == media_status_t::AMEDIA_OK
                && AImage_getCropRect(raw, &mut crop) == media_status_t::AMEDIA_OK
                && (reader.symbols.get_hardware_buffer)(raw, &mut buffer)
                    == media_status_t::AMEDIA_OK
        };
        let buffer = NonNull::new(buffer).filter(|_| ok);
        let Some(buffer) = buffer else {
            // SAFETY: the image is live and nothing else owns it.
            unsafe { AImage_delete(raw) };
            return None;
        };
        let side = |from: i32, to: i32| u32::try_from(to - from).unwrap_or(0);
        Some(Self {
            image,
            buffer,
            timestamp_ns,
            size: (width.max(0) as u32, height.max(0) as u32),
            crop: CropRect {
                x: crop.left.max(0) as u32,
                y: crop.top.max(0) as u32,
                width: side(crop.left, crop.right),
                height: side(crop.top, crop.bottom),
            },
            _reader: reader,
        })
    }

    // The time that the decoder rendered the picture with, in nanoseconds: the codec's
    // presentation time.
    pub(super) fn timestamp_ns(&self) -> i64 {
        self.timestamp_ns
    }

    pub(super) fn size(&self) -> (u32, u32) {
        self.size
    }

    pub(super) fn crop(&self) -> CropRect {
        self.crop
    }

    pub(super) fn hardware_buffer(&self) -> *mut c_void {
        self.buffer.as_ptr().cast()
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        // SAFETY: the image is live and owned here.
        unsafe { AImage_delete(self.image.as_ptr()) };
    }
}
