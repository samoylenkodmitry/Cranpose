// AAudio arrived in Android 8.0 (API 26), after the oldest Android an app may
// declare as `minSdk`. Linking `libaaudio.so` would fail the build of such an
// app and stop its library loading on an older device, so the functions are
// found when the first stream opens.

#![expect(unsafe_code, reason = "loads AAudio and reads its result texts")]

use std::{
    ffi::{CStr, c_char, c_void},
    fmt,
    sync::OnceLock,
};

use cranpose_services::AudioError;

#[repr(C)]
pub(super) struct RawBuilder {
    _opaque: [u8; 0],
}

#[repr(C)]
pub(super) struct RawStream {
    _opaque: [u8; 0],
}

pub(super) type DataCallback = unsafe extern "C" fn(
    stream: *mut RawStream,
    user_data: *mut c_void,
    audio_data: *mut c_void,
    frames: i32,
) -> i32;

pub(super) type ErrorCallback =
    unsafe extern "C" fn(stream: *mut RawStream, user_data: *mut c_void, error: i32);

pub(super) type StreamFunction = unsafe extern "C" fn(stream: *mut RawStream) -> i32;

type BuilderSetter = unsafe extern "C" fn(builder: *mut RawBuilder, value: i32);

pub(super) struct Library {
    pub(super) builder: BuilderFunctions,
    pub(super) stream: StreamFunctions,
    convert_result_to_text: unsafe extern "C" fn(result: i32) -> *const c_char,
}

// Each field has the signature AAudio.h gives the `AAudioStreamBuilder`
// function of that name, `create` being `AAudio_createStreamBuilder`.
pub(super) struct BuilderFunctions {
    pub(super) create: unsafe extern "C" fn(builder: *mut *mut RawBuilder) -> i32,
    pub(super) set_direction: BuilderSetter,
    pub(super) set_format: BuilderSetter,
    pub(super) set_channel_count: BuilderSetter,
    pub(super) set_performance_mode: BuilderSetter,
    pub(super) set_sharing_mode: BuilderSetter,
    pub(super) set_data_callback: unsafe extern "C" fn(
        builder: *mut RawBuilder,
        callback: Option<DataCallback>,
        user_data: *mut c_void,
    ),
    pub(super) set_error_callback: unsafe extern "C" fn(
        builder: *mut RawBuilder,
        callback: Option<ErrorCallback>,
        user_data: *mut c_void,
    ),
    pub(super) open_stream:
        unsafe extern "C" fn(builder: *mut RawBuilder, stream: *mut *mut RawStream) -> i32,
    pub(super) delete: unsafe extern "C" fn(builder: *mut RawBuilder) -> i32,
}

// Each field has the signature AAudio.h gives the `AAudioStream` function of
// that name, a getter without its `get` prefix.
pub(super) struct StreamFunctions {
    pub(super) close: StreamFunction,
    pub(super) request_start: StreamFunction,
    pub(super) request_pause: StreamFunction,
    pub(super) request_stop: StreamFunction,
    pub(super) state: StreamFunction,
    pub(super) sample_rate: StreamFunction,
    pub(super) channel_count: StreamFunction,
    pub(super) format: StreamFunction,
    pub(super) frames_per_burst: StreamFunction,
}

impl Library {
    pub(super) fn load() -> Result<&'static Self, AudioError> {
        static LOADED: OnceLock<Option<Library>> = OnceLock::new();
        LOADED.get_or_init(resolve).as_ref().ok_or_else(|| {
            AudioError::Backend(
                "this Android has no AAudio, which arrived in Android 8.0 (API 26)".into(),
            )
        })
    }

    pub(super) fn check(&self, result: i32) -> Result<(), Failure> {
        if result >= 0 {
            Ok(())
        } else {
            Err(self.failure(result))
        }
    }

    pub(super) fn failure(&self, result: i32) -> Failure {
        // SAFETY: AAudio returns a static, NUL-terminated text for every
        // result, an unknown one included.
        let text = unsafe { CStr::from_ptr((self.convert_result_to_text)(result)) };
        Failure { result, text }
    }
}

fn resolve() -> Option<Library> {
    // SAFETY: the name ends in a NUL. The handle stays open for the life of
    // the process, so the functions found in it stay valid.
    let handle = unsafe { libc::dlopen(c"libaaudio.so".as_ptr(), libc::RTLD_NOW) };
    if handle.is_null() {
        log::info!("cranpose audio: this Android has no libaaudio.so");
        return None;
    }
    // SAFETY: `handle` is the open libaaudio.so, and the field's type is the
    // signature AAudio.h gives `AAudio_convertResultToText`.
    unsafe {
        Some(Library {
            builder: BuilderFunctions::resolve(handle)?,
            stream: StreamFunctions::resolve(handle)?,
            convert_result_to_text: function(handle, c"AAudio_convertResultToText")?,
        })
    }
}

impl BuilderFunctions {
    // The caller guarantees that `handle` is the open libaaudio.so.
    unsafe fn resolve(handle: *mut c_void) -> Option<Self> {
        // SAFETY: each field's type is the signature AAudio.h gives the
        // function named beside it.
        unsafe {
            Some(Self {
                create: function(handle, c"AAudio_createStreamBuilder")?,
                set_direction: function(handle, c"AAudioStreamBuilder_setDirection")?,
                set_format: function(handle, c"AAudioStreamBuilder_setFormat")?,
                set_channel_count: function(handle, c"AAudioStreamBuilder_setChannelCount")?,
                set_performance_mode: function(handle, c"AAudioStreamBuilder_setPerformanceMode")?,
                set_sharing_mode: function(handle, c"AAudioStreamBuilder_setSharingMode")?,
                set_data_callback: function(handle, c"AAudioStreamBuilder_setDataCallback")?,
                set_error_callback: function(handle, c"AAudioStreamBuilder_setErrorCallback")?,
                open_stream: function(handle, c"AAudioStreamBuilder_openStream")?,
                delete: function(handle, c"AAudioStreamBuilder_delete")?,
            })
        }
    }
}

impl StreamFunctions {
    // The caller guarantees that `handle` is the open libaaudio.so.
    unsafe fn resolve(handle: *mut c_void) -> Option<Self> {
        // SAFETY: each field's type is the signature AAudio.h gives the
        // function named beside it.
        unsafe {
            Some(Self {
                close: function(handle, c"AAudioStream_close")?,
                request_start: function(handle, c"AAudioStream_requestStart")?,
                request_pause: function(handle, c"AAudioStream_requestPause")?,
                request_stop: function(handle, c"AAudioStream_requestStop")?,
                state: function(handle, c"AAudioStream_getState")?,
                sample_rate: function(handle, c"AAudioStream_getSampleRate")?,
                channel_count: function(handle, c"AAudioStream_getChannelCount")?,
                format: function(handle, c"AAudioStream_getFormat")?,
                frames_per_burst: function(handle, c"AAudioStream_getFramesPerBurst")?,
            })
        }
    }
}

// The caller guarantees that `F` is the type of the C function `name`.
unsafe fn function<F: Copy>(handle: *mut c_void, name: &CStr) -> Option<F> {
    const { assert!(size_of::<F>() == size_of::<*mut c_void>()) };
    // SAFETY: `handle` came from `dlopen`, and `name` ends in a NUL.
    let address = unsafe { libc::dlsym(handle, name.as_ptr()) };
    if address.is_null() {
        log::warn!("cranpose audio: libaaudio.so has no {name:?}");
        return None;
    }
    // SAFETY: the address is the function `name`, whose type the caller
    // names as `F`, a function pointer as wide as the address.
    Some(unsafe { std::mem::transmute_copy::<*mut c_void, F>(&address) })
}

pub(super) struct Failure {
    result: i32,
    text: &'static CStr,
}

impl Failure {
    pub(super) fn in_context(self, context: &str) -> AudioError {
        AudioError::Backend(format!("{context}: {self}"))
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.text.to_string_lossy(), self.result)
    }
}
