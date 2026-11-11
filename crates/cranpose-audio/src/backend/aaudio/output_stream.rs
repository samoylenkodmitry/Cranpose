#![expect(unsafe_code, reason = "owns an AAudio stream and runs its callbacks")]

use std::{
    ffi::c_void,
    ptr::{self, NonNull},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use cranpose_services::AudioError;

use super::library::{Failure, Library, RawStream, StreamFunction};
use crate::{
    backend::{NOMINAL_CHANNELS, Renderer},
    mixer::RenderStatus,
};

const DIRECTION_OUTPUT: i32 = 0;
const FORMAT_PCM_FLOAT: i32 = 2;
const SHARING_MODE_SHARED: i32 = 1;
const PERFORMANCE_MODE_LOW_LATENCY: i32 = 12;
const CALLBACK_CONTINUE: i32 = 0;
const CALLBACK_STOP: i32 = 1;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct StreamState(i32);

impl StreamState {
    pub(super) const UNINITIALIZED: Self = Self(0);
    pub(super) const STARTED: Self = Self(4);
    pub(super) const CLOSING: Self = Self(11);
    pub(super) const CLOSED: Self = Self(12);
    pub(super) const DISCONNECTED: Self = Self(13);
}

// An open 32-bit float output stream that runs a renderer. Dropping it closes
// the stream, then drops the renderer on the dropping thread.
pub(super) struct OutputStream {
    library: &'static Library,
    raw: NonNull<RawStream>,
    _callbacks: Callbacks,
}

impl OutputStream {
    // `connected` turns false when AAudio reports the stream lost.
    pub(super) fn open(
        renderer: Box<dyn Renderer>,
        connected: Arc<AtomicBool>,
    ) -> Result<Self, AudioError> {
        let library = Library::load()?;
        let callbacks = Callbacks::new(library, renderer, connected);
        let mut builder = ptr::null_mut();
        // SAFETY: `builder` is a place for the new builder.
        library
            .check(unsafe { (library.builder.create)(&mut builder) })
            .map_err(|failure| failure.in_context("failed to create an AAudio stream builder"))?;
        let mut raw = ptr::null_mut();
        // SAFETY: `builder` is the builder created above, and this block
        // deletes it after its last use. The stream gets the callbacks'
        // contexts, which live until the stream is closed: `OutputStream`
        // closes it before it drops them, and a stream that failed to open
        // never calls them.
        let opened = unsafe {
            (library.builder.set_direction)(builder, DIRECTION_OUTPUT);
            (library.builder.set_format)(builder, FORMAT_PCM_FLOAT);
            (library.builder.set_channel_count)(builder, NOMINAL_CHANNELS as i32);
            (library.builder.set_performance_mode)(builder, PERFORMANCE_MODE_LOW_LATENCY);
            (library.builder.set_sharing_mode)(builder, SHARING_MODE_SHARED);
            (library.builder.set_data_callback)(
                builder,
                Some(render),
                callbacks.data.as_ptr().cast(),
            );
            (library.builder.set_error_callback)(
                builder,
                Some(report_error),
                callbacks.error.as_ptr().cast(),
            );
            let opened = (library.builder.open_stream)(builder, &mut raw);
            (library.builder.delete)(builder);
            opened
        };
        library
            .check(opened)
            .map_err(|failure| failure.in_context("failed to open the AAudio output stream"))?;
        let raw = NonNull::new(raw)
            .ok_or_else(|| AudioError::Backend("AAudio opened no output stream".into()))?;
        let stream = Self {
            library,
            raw,
            _callbacks: callbacks,
        };
        let format = stream.query(library.stream.format);
        if format != FORMAT_PCM_FLOAT {
            return Err(AudioError::Backend(format!(
                "AAudio opened the output stream as format {format} rather than 32-bit float"
            )));
        }
        Ok(stream)
    }

    pub(super) fn request_start(&self) -> Result<(), Failure> {
        self.library
            .check(self.query(self.library.stream.request_start))
    }

    pub(super) fn request_pause(&self) -> Result<(), Failure> {
        self.library
            .check(self.query(self.library.stream.request_pause))
    }

    pub(super) fn request_stop(&self) -> Result<(), Failure> {
        self.library
            .check(self.query(self.library.stream.request_stop))
    }

    pub(super) fn state(&self) -> StreamState {
        StreamState(self.query(self.library.stream.state))
    }

    pub(super) fn sample_rate(&self) -> i32 {
        self.query(self.library.stream.sample_rate)
    }

    pub(super) fn channel_count(&self) -> i32 {
        self.query(self.library.stream.channel_count)
    }

    pub(super) fn frames_per_burst(&self) -> i32 {
        self.query(self.library.stream.frames_per_burst)
    }

    fn query(&self, function: StreamFunction) -> i32 {
        // SAFETY: every caller passes a stream function of `library`, and
        // `raw` is this open stream.
        unsafe { function(self.raw.as_ptr()) }
    }
}

impl Drop for OutputStream {
    fn drop(&mut self) {
        if let Err(failure) = self.library.check(self.query(self.library.stream.close)) {
            log::warn!("failed to close the AAudio stream: {failure}");
        }
    }
}

struct DataContext {
    library: &'static Library,
    renderer: Box<dyn Renderer>,
}

struct ErrorContext {
    library: &'static Library,
    connected: Arc<AtomicBool>,
}

// The contexts the stream's callbacks receive. They are reached only through
// these pointers, by AAudio while the stream is open and by `drop` after.
struct Callbacks {
    data: NonNull<DataContext>,
    error: NonNull<ErrorContext>,
}

impl Callbacks {
    fn new(
        library: &'static Library,
        renderer: Box<dyn Renderer>,
        connected: Arc<AtomicBool>,
    ) -> Self {
        Self {
            data: NonNull::from(Box::leak(Box::new(DataContext { library, renderer }))),
            error: NonNull::from(Box::leak(Box::new(ErrorContext { library, connected }))),
        }
    }
}

impl Drop for Callbacks {
    fn drop(&mut self) {
        // SAFETY: both contexts came from `Box::leak` in `new`, and no stream
        // calls them any more: the one they were given to is closed, or never
        // opened.
        unsafe {
            drop(Box::from_raw(self.data.as_ptr()));
            drop(Box::from_raw(self.error.as_ptr()));
        }
    }
}

unsafe extern "C" fn render(
    stream: *mut RawStream,
    user_data: *mut c_void,
    audio_data: *mut c_void,
    frames: i32,
) -> i32 {
    // SAFETY: `user_data` is the stream's `DataContext`, which outlives the
    // stream. AAudio runs the data callback on one thread at a time, and no
    // other code reaches the context while the stream is open.
    let context = unsafe { &mut *user_data.cast::<DataContext>() };
    let library = context.library;
    // SAFETY: AAudio passes the open stream the callback runs for.
    let (sample_rate, channels) = unsafe {
        (
            (library.stream.sample_rate)(stream),
            (library.stream.channel_count)(stream),
        )
    };
    let channels = channels.max(1) as usize;
    context
        .renderer
        .set_device_format(sample_rate as f32, channels);

    let samples = frames.max(0) as usize * channels;
    if samples == 0 || audio_data.is_null() {
        return CALLBACK_CONTINUE;
    }
    // SAFETY: AAudio guarantees `audio_data` points at `frames *
    // channelCount` writable samples for this call only, and `open` refuses a
    // stream whose samples are not 32-bit float.
    let out = unsafe { std::slice::from_raw_parts_mut(audio_data.cast::<f32>(), samples) };
    match context.renderer.render(out) {
        RenderStatus::Continue => CALLBACK_CONTINUE,
        RenderStatus::Idle => CALLBACK_STOP,
    }
}

unsafe extern "C" fn report_error(_stream: *mut RawStream, user_data: *mut c_void, error: i32) {
    // SAFETY: `user_data` is the stream's `ErrorContext`, which outlives the
    // stream and is only read while the stream is open.
    let context = unsafe { &*user_data.cast::<ErrorContext>() };
    context.connected.store(false, Ordering::Release);
    log::warn!("AAudio stream error: {}", context.library.failure(error));
}
