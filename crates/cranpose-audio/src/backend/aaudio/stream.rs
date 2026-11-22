#![expect(unsafe_code, reason = "owns an AAudio stream and its error callback")]

use std::{
    ffi::c_void,
    ptr::{self, NonNull},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use cranpose_services::AudioError;

use super::library::{DataCallback, Failure, Library, RawStream, StreamFunction};
use crate::backend::hardware_position::HardwarePosition;

const FORMAT_PCM_FLOAT: i32 = 2;
const SHARING_MODE_SHARED: i32 = 1;
const PERFORMANCE_MODE_LOW_LATENCY: i32 = 12;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct StreamState(i32);

impl StreamState {
    pub(super) const UNINITIALIZED: Self = Self(0);
    pub(super) const STARTING: Self = Self(3);
    pub(super) const STARTED: Self = Self(4);
    pub(super) const CLOSING: Self = Self(11);
    pub(super) const CLOSED: Self = Self(12);
    pub(super) const DISCONNECTED: Self = Self(13);
}

#[derive(Clone, Copy)]
pub(super) enum Direction {
    Output,
    // With AAudio's input preset
    Input { preset: i32 },
}

impl Direction {
    fn name(self) -> &'static str {
        match self {
            Self::Output => "output",
            Self::Input { .. } => "input",
        }
    }
}

// A value that a stream's callback reaches through a pointer. It lives until it is dropped, which
// its owner does only after the stream is closed: an owner declares it after its `Stream`, so the
// stream drops first.
pub(super) struct Leaked<T>(NonNull<T>);

impl<T> Leaked<T> {
    pub(super) fn new(value: T) -> Self {
        Self(NonNull::from(Box::leak(Box::new(value))))
    }

    pub(super) fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr().cast()
    }
}

impl<T> Drop for Leaked<T> {
    fn drop(&mut self) {
        // SAFETY: the value came from `Box::leak` in `new`, and no stream calls it any more: the
        // owner closed the stream that had the pointer, or the stream never opened.
        unsafe { drop(Box::from_raw(self.0.as_ptr())) };
    }
}

const CALLBACK_CONTINUE: i32 = 0;
pub(super) const CALLBACK_STOP: i32 = 1;

// What the data callback of a stream runs. The context of the stream is a value of this type.
pub(super) trait Callback {
    fn library(&self) -> &'static Library;

    // Runs for one buffer of interleaved samples, `channels` to a frame, at `sample_rate`. Returns
    // whether the stream goes on: [`CALLBACK_STOP`] ends it.
    fn run(
        &mut self,
        stream: &Running,
        audio: &mut [f32],
        sample_rate: i32,
        channels: usize,
    ) -> bool;
}

// The stream that a data callback runs for, during the call.
pub(super) struct Running {
    library: &'static Library,
    raw: *mut RawStream,
}

impl Running {
    // The frames that the app has read from an input stream, the buffer of this callback
    // included: AAudio reads the buffer before it calls the callback. The count starts where
    // AAudio's buffer started, which lies before the first callback: a stream of a shared
    // endpoint starts at the endpoint's position.
    pub(super) fn frames_read(&self) -> i64 {
        // SAFETY: AAudio passed this open stream to the callback that holds this value.
        unsafe { (self.library.stream.frames_read)(self.raw) }
    }
}

// The data callback of a stream whose context is a `T`.
pub(super) unsafe extern "C" fn data_callback<T: Callback>(
    stream: *mut RawStream,
    user_data: *mut c_void,
    audio_data: *mut c_void,
    frames: i32,
) -> i32 {
    // SAFETY: `user_data` is the stream's context, a `T` that outlives the stream. AAudio runs the
    // data callback on one thread at a time, and no other code reaches the context while the
    // stream is open.
    let context = unsafe { &mut *user_data.cast::<T>() };
    let library = context.library();
    // SAFETY: AAudio passes the open stream the callback runs for.
    let (sample_rate, channels) = unsafe {
        (
            (library.stream.sample_rate)(stream),
            (library.stream.channel_count)(stream),
        )
    };
    let channels = channels.max(1) as usize;
    let samples = frames.max(0) as usize * channels;
    if samples == 0 || audio_data.is_null() {
        return CALLBACK_CONTINUE;
    }
    // SAFETY: AAudio guarantees `audio_data` points at `frames * channelCount` samples for this
    // call only, and `Stream::open` refuses a stream whose samples are not 32-bit float.
    let audio = unsafe { std::slice::from_raw_parts_mut(audio_data.cast::<f32>(), samples) };
    let running = Running {
        library,
        raw: stream,
    };
    if context.run(&running, audio, sample_rate, channels) {
        CALLBACK_CONTINUE
    } else {
        CALLBACK_STOP
    }
}

struct ErrorContext {
    library: &'static Library,
    connected: Arc<AtomicBool>,
    direction: Direction,
}

// An open 32-bit float stream in low-latency mode. Dropping it closes the stream.
pub(super) struct Stream {
    library: &'static Library,
    raw: NonNull<RawStream>,
    _error: Leaked<ErrorContext>,
}

impl Stream {
    // Opens a stream that calls `callback` with `data`. `connected` turns false when AAudio
    // reports the stream lost.
    //
    // SAFETY: the caller keeps `data` valid until the stream is closed, and `callback` reads it
    // as what it is.
    pub(super) unsafe fn open(
        library: &'static Library,
        direction: Direction,
        channels: i32,
        callback: DataCallback,
        data: *mut c_void,
        connected: Arc<AtomicBool>,
    ) -> Result<Self, AudioError> {
        let error = Leaked::new(ErrorContext {
            library,
            connected,
            direction,
        });
        let mut builder = ptr::null_mut();
        // SAFETY: `builder` is a place for the new builder.
        library
            .check(unsafe { (library.builder.create)(&mut builder) })
            .map_err(|failure| failure.in_context("failed to create an AAudio stream builder"))?;
        let mut raw = ptr::null_mut();
        // SAFETY: `builder` is the builder created above, and this block deletes it after its
        // last use. The stream gets the two contexts, which live until the stream is closed: the
        // caller vouches for `data`, and `Stream` declares the error context after the stream
        // that it closes. A stream that failed to open never calls them.
        let opened = unsafe {
            let code = match direction {
                Direction::Output => 0,
                Direction::Input { .. } => 1,
            };
            (library.builder.set_direction)(builder, code);
            (library.builder.set_format)(builder, FORMAT_PCM_FLOAT);
            (library.builder.set_channel_count)(builder, channels);
            (library.builder.set_performance_mode)(builder, PERFORMANCE_MODE_LOW_LATENCY);
            (library.builder.set_sharing_mode)(builder, SHARING_MODE_SHARED);
            // An Android older than 9 records with its default preset
            if let (Direction::Input { preset }, Some(set_input_preset)) =
                (direction, library.builder.set_input_preset)
            {
                set_input_preset(builder, preset);
            }
            (library.builder.set_data_callback)(builder, Some(callback), data);
            (library.builder.set_error_callback)(builder, Some(report_error), error.as_ptr());
            let opened = (library.builder.open_stream)(builder, &mut raw);
            (library.builder.delete)(builder);
            opened
        };
        let name = direction.name();
        library.check(opened).map_err(|failure| {
            failure.in_context(&format!("failed to open the AAudio {name} stream"))
        })?;
        let raw = NonNull::new(raw)
            .ok_or_else(|| AudioError::Backend(format!("AAudio opened no {name} stream")))?;
        let stream = Self {
            library,
            raw,
            _error: error,
        };
        let format = stream.query(library.stream.format);
        if format != FORMAT_PCM_FLOAT {
            return Err(AudioError::Backend(format!(
                "AAudio opened the {name} stream as format {format} rather than 32-bit float"
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

    // Underruns of an output stream, overruns of an input stream
    pub(super) fn xrun_count(&self) -> i32 {
        self.query(self.library.stream.xrun_count)
    }

    pub(super) fn frames_written(&self) -> i64 {
        // SAFETY: `raw` is this open stream.
        unsafe { (self.library.stream.frames_written)(self.raw.as_ptr()) }
    }

    // The frame that the hardware presented or captured last, with its time on
    // `CLOCK_MONOTONIC`, or `None` while the stream has no timestamp: AAudio gives none until a
    // short time after `requestStart`, and while the stream does not run.
    pub(super) fn timestamp(&self) -> Option<HardwarePosition> {
        let (mut frame_position, mut nano_time) = (0, 0);
        // SAFETY: the two outputs are places for one `i64` each. `raw` is this open stream.
        let result = unsafe {
            (self.library.stream.timestamp)(
                self.raw.as_ptr(),
                libc::CLOCK_MONOTONIC,
                &mut frame_position,
                &mut nano_time,
            )
        };
        (result >= 0).then_some(HardwarePosition {
            frame_position,
            nano_time,
        })
    }

    fn query(&self, function: StreamFunction) -> i32 {
        // SAFETY: every caller passes a stream function of `library`, and `raw` is this open
        // stream.
        unsafe { function(self.raw.as_ptr()) }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        if let Err(failure) = self.library.check(self.query(self.library.stream.close)) {
            log::warn!("failed to close the AAudio stream: {failure}");
        }
    }
}

unsafe extern "C" fn report_error(_stream: *mut RawStream, user_data: *mut c_void, error: i32) {
    // SAFETY: `user_data` is the stream's `ErrorContext`, which outlives the stream and is only
    // read while the stream is open.
    let context = unsafe { &*user_data.cast::<ErrorContext>() };
    context.connected.store(false, Ordering::Release);
    log::warn!(
        "AAudio {} stream error: {}",
        context.direction.name(),
        context.library.failure(error)
    );
}
