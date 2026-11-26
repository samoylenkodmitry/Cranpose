// Recording from the platform's default microphone. Jetpack Compose has no API for recording.
// Android apps use `AudioRecord` or AAudio, and this follows AAudio.

use std::time::{Duration, Instant};

use cranpose_services::{
    AudioError, MicrophonePermission, PreparedMicrophone, microphone_permission, prepare_microphone,
};

/// The format of the samples that a [`Recorder`] receives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputFormat {
    /// Frames a second
    pub sample_rate: f32,
    /// Samples in one frame, interleaved
    pub channels: usize,
}

/// How the platform treats the sound before the recorder gets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InputPreset {
    /// For speech recognition: the platform cancels no echo and keeps the delay short. AAudio's
    /// `AAUDIO_INPUT_PRESET_VOICE_RECOGNITION`. A browser gets no echo cancellation, noise
    /// suppression or gain control.
    VoiceRecognition,
    /// For a call: the platform cancels the echo of what the device plays. AAudio's
    /// `AAUDIO_INPUT_PRESET_VOICE_COMMUNICATION`, voice processing on watchOS and echo
    /// cancellation in a browser. Core Audio, WASAPI and ALSA record the same as for
    /// [`InputPreset::VoiceRecognition`].
    VoiceCommunication,
}

/// What a microphone stream hands its samples to.
///
/// The platform calls it on the thread that delivers recorded sound: the real-time input thread of
/// AAudio, Core Audio, WASAPI and ALSA, the tap thread of `AVAudioEngine`, and the page's thread
/// in a browser. It may not allocate, lock or block.
pub trait Recorder: Send {
    /// Takes interleaved samples that the microphone recorded, in the format the device
    /// negotiated. `captured` is when the microphone took the first frame of `input`, on the
    /// clock of [`Instant`], where the platform reports it: AAudio from `AAudioStream_getTimestamp`,
    /// Core Audio, WASAPI and ALSA from the capture time of their callback, `AVAudioEngine` from
    /// the host time of the tap's buffer. A browser reports none.
    fn record(&mut self, input: &[f32], format: InputFormat, captured: Option<Instant>);
}

/// A running microphone stream. Dropping it stops the recording and releases the audio that
/// [`prepare_microphone()`] readied for it.
pub trait AudioSource: Send + Sync {
    /// Whether the stream still records. The platform ends a stream when its device goes away,
    /// such as a headset that the person unplugs, and the recorder then gets nothing more. An app
    /// that still wants to record opens a new stream.
    fn is_running(&self) -> bool {
        true
    }

    /// How long a frame takes from the microphone to the recorder, as the platform reported it
    /// last. `None` means that the platform has no value yet or has none at all.
    fn input_latency(&self) -> Option<Duration> {
        None
    }

    /// How many frames one call of the recorder takes, where the device fixes that number.
    fn frames_per_burst(&self) -> Option<u32> {
        None
    }

    /// How many times the device lost recorded frames because the recorder did not take them in
    /// time, since the stream opened. AAudio reports `AAudioStream_getXRunCount`. `None` means
    /// that the platform keeps no count.
    fn overrun_count(&self) -> Option<u32> {
        None
    }
}

/// Opens the platform's default microphone, starts it and runs `recorder` on it. Jetpack Compose
/// has no API for recording: Android apps use `AudioRecord` or AAudio, and this follows AAudio.
///
/// The person must have allowed the microphone first ([`microphone_permission()`] reads
/// [`MicrophonePermission::Granted`]); otherwise this opens nothing and returns
/// [`AudioError::NotAllowed`]. The stream readies the platform's audio for recording with
/// [`prepare_microphone()`] and keeps it readied while it runs. A build without a backend for the
/// target returns [`AudioError::Unsupported`]: the backends are the ones of [`super::open`].
pub fn open_microphone(
    recorder: Box<dyn Recorder>,
    preset: InputPreset,
) -> Result<Box<dyn AudioSource>, AudioError> {
    if microphone_permission() != MicrophonePermission::Granted {
        return Err(AudioError::NotAllowed);
    }
    if !super::is_compiled() {
        return Err(AudioError::Unsupported);
    }
    let prepared = prepare_microphone().map_err(AudioError::Backend)?;
    let source = open_platform(recorder, preset)?;
    Ok(Box::new(Prepared {
        source,
        _prepared: prepared,
    }))
}

// The platform's stream, with the readied audio that it needs. The stream drops first.
struct Prepared {
    source: Box<dyn AudioSource>,
    _prepared: PreparedMicrophone,
}

impl AudioSource for Prepared {
    fn is_running(&self) -> bool {
        self.source.is_running()
    }

    fn input_latency(&self) -> Option<Duration> {
        self.source.input_latency()
    }

    fn frames_per_burst(&self) -> Option<u32> {
        self.source.frames_per_burst()
    }

    fn overrun_count(&self) -> Option<u32> {
        self.source.overrun_count()
    }
}

// When the microphone took a frame that it took `age` before now. A browser's backend and a build
// with no backend never ask.
#[allow(
    dead_code,
    reason = "the backends of AAudio, Core Audio and AVAudioEngine use it"
)]
pub(crate) fn captured_at(age: Duration) -> Option<Instant> {
    Instant::now().checked_sub(age)
}

// Each arm is a separate `cfg` block, as in `super::open`.
#[allow(
    clippy::needless_return,
    reason = "each cfg arm returns when a platform microphone backend is enabled, and falls through otherwise"
)]
fn open_platform(
    recorder: Box<dyn Recorder>,
    preset: InputPreset,
) -> Result<Box<dyn AudioSource>, AudioError> {
    #[cfg(all(feature = "aaudio", target_os = "android"))]
    {
        return super::aaudio::open_microphone(recorder, preset);
    }

    #[cfg(all(feature = "avaudio", target_os = "watchos"))]
    {
        return super::avaudio_microphone::open(recorder, preset);
    }

    #[cfg(all(
        feature = "cpal-backend",
        not(any(target_os = "android", target_arch = "wasm32"))
    ))]
    {
        return super::cpal_microphone::open(recorder, preset);
    }

    #[cfg(all(feature = "web-audio", target_arch = "wasm32"))]
    {
        return super::web_microphone::open(recorder, preset);
    }

    #[cfg(not(any(
        all(feature = "aaudio", target_os = "android"),
        all(feature = "avaudio", target_os = "watchos"),
        all(
            feature = "cpal-backend",
            not(any(target_os = "android", target_arch = "wasm32"))
        ),
        all(feature = "web-audio", target_arch = "wasm32")
    )))]
    {
        drop((recorder, preset));
        Err(AudioError::Unsupported)
    }
}
