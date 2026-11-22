#![expect(
    unsafe_code,
    reason = "runs the recorder in an AAudio stream's callback"
)]

use std::{
    ops::Deref,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
};

use cranpose_services::AudioError;

use super::{
    library::{Library, monotonic_nanos},
    stream::{Callback, Direction, Leaked, Running, Stream, data_callback},
};
use crate::backend::{
    atomic_duration::AtomicDuration,
    hardware_position::HardwarePosition,
    microphone::{InputFormat, InputPreset, Recorder, captured_at},
};

const INPUT_PRESET_VOICE_RECOGNITION: i32 = 6;
const INPUT_PRESET_VOICE_COMMUNICATION: i32 = 7;
const INPUT_PRESET_UNPROCESSED: i32 = 9;
// A voice is one channel. The device mixes several microphones down to it.
const CHANNELS: i32 = 1;
// The word of a capture time that the owner thread has not published.
const UNKNOWN: i64 = i64::MIN;

// What the owner thread and the callback tell each other about the stream's timing.
pub(super) struct Timing {
    // The capture time on `CLOCK_MONOTONIC` of the stream's frame 0, from the stream's last
    // timestamp. The callback finds the time of its buffer from the stream's count of frames read.
    frame_zero_nanos: AtomicI64,
    // How old the first frame of the last record call was.
    pub(super) age: AtomicDuration,
}

impl Timing {
    pub(super) fn new() -> Self {
        Self {
            frame_zero_nanos: AtomicI64::new(UNKNOWN),
            age: AtomicDuration::new(),
        }
    }

    fn frame_zero(&self) -> Option<HardwarePosition> {
        match self.frame_zero_nanos.load(Ordering::Relaxed) {
            UNKNOWN => None,
            nano_time => Some(HardwarePosition {
                frame_position: 0,
                nano_time,
            }),
        }
    }
}

struct DataContext {
    library: &'static Library,
    recorder: Box<dyn Recorder>,
    timing: Arc<Timing>,
}

// An open 32-bit float input stream that runs a recorder. Dropping it closes the stream, then
// drops the recorder on the dropping thread: the stream is declared first, so it drops first.
pub(super) struct InputStream {
    stream: Stream,
    _data: Leaked<DataContext>,
}

impl Deref for InputStream {
    type Target = Stream;

    fn deref(&self) -> &Stream {
        &self.stream
    }
}

impl InputStream {
    // `connected` turns false when AAudio reports the stream lost.
    pub(super) fn open(
        recorder: Box<dyn Recorder>,
        preset: InputPreset,
        timing: Arc<Timing>,
        connected: Arc<AtomicBool>,
    ) -> Result<Self, AudioError> {
        let library = Library::load()?;
        let data = Leaked::new(DataContext {
            library,
            recorder,
            timing,
        });
        let preset = match preset {
            InputPreset::VoiceRecognition => INPUT_PRESET_VOICE_RECOGNITION,
            InputPreset::VoiceCommunication => INPUT_PRESET_VOICE_COMMUNICATION,
            InputPreset::Unprocessed => INPUT_PRESET_UNPROCESSED,
        };
        // SAFETY: `data` is the `DataContext` that `record` reads, and it lives until the stream
        // closes: `InputStream` declares it after the stream.
        let stream = unsafe {
            Stream::open(
                library,
                Direction::Input { preset },
                CHANNELS,
                data_callback::<DataContext>,
                data.as_ptr(),
                connected,
            )
        }?;
        Ok(Self {
            stream,
            _data: data,
        })
    }

    // Reads the stream's timestamp and publishes the capture time of its frame 0. The thread that
    // owns the stream calls this, so the real-time callback does no extra work. Returns whether a
    // time was published.
    pub(super) fn publish_timing(&self, timing: &Timing) -> bool {
        let frame_zero = self
            .timestamp()
            .and_then(|position| position.time_of(0, self.sample_rate()))
            .and_then(|nanos| i64::try_from(nanos).ok())
            .filter(|&nanos| nanos != UNKNOWN);
        if let Some(nanos) = frame_zero {
            timing.frame_zero_nanos.store(nanos, Ordering::Relaxed);
        }
        frame_zero.is_some()
    }
}

impl Callback for DataContext {
    fn library(&self) -> &'static Library {
        self.library
    }

    fn run(
        &mut self,
        stream: &Running,
        audio: &mut [f32],
        sample_rate: i32,
        channels: usize,
    ) -> bool {
        // The stream's number of this buffer's first frame. A count of its own from 0 was off by
        // the frames before the first callback: 50 to 150 ms on a Quest 3.
        let first = (stream.frames_read() - (audio.len() / channels) as i64).max(0);
        let age = self
            .timing
            .frame_zero()
            .and_then(|zero| zero.age(first, monotonic_nanos(), sample_rate));
        if let Some(age) = age {
            self.timing.age.store(age);
        }
        let format = InputFormat {
            sample_rate: sample_rate as f32,
            channels,
        };
        self.recorder
            .record(audio, format, age.and_then(captured_at));
        true
    }
}
