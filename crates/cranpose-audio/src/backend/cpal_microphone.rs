// The desktop microphone, and the one of iOS: the default input device through cpal (Core Audio,
// WASAPI or ALSA). These hosts record the same for both presets.

use std::{sync::Arc, time::Duration};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cranpose_services::AudioError;

use crate::backend::{
    atomic_duration::AtomicDuration,
    microphone::{AudioSource, InputFormat, InputPreset, Recorder, captured_at},
};

const SCRATCH_SAMPLES: usize = 8192;

pub(crate) fn open(
    mut recorder: Box<dyn Recorder>,
    _preset: InputPreset,
) -> Result<Box<dyn AudioSource>, AudioError> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or_else(|| {
        AudioError::Backend("the system reports no default audio input device".to_string())
    })?;
    let supported = device
        .default_input_config()
        .map_err(|error| AudioError::Backend(format!("no usable input configuration: {error}")))?;
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let format = InputFormat {
        sample_rate: config.sample_rate as f32,
        channels: usize::from(config.channels).max(1),
    };
    let error_callback = |error| log::warn!("cranpose audio input stream error: {error}");
    let latency = Arc::new(AtomicDuration::new());
    let callback_latency = Arc::clone(&latency);
    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], info: &cpal::InputCallbackInfo| {
                let age = age(&callback_latency, info);
                recorder.record(data, format, age.and_then(captured_at));
            },
            error_callback,
            None,
        ),
        cpal::SampleFormat::I16 => {
            let mut scratch = vec![0.0f32; SCRATCH_SAMPLES];
            device.build_input_stream(
                config,
                move |data: &[i16], info: &cpal::InputCallbackInfo| {
                    let age = age(&callback_latency, info);
                    record_integers(&mut *recorder, &mut scratch, data, format, age, |sample| {
                        f32::from(sample) / -f32::from(i16::MIN)
                    });
                },
                error_callback,
                None,
            )
        }
        other => {
            return Err(AudioError::Backend(format!(
                "the default input device gives {other:?} samples, which the engine does not read"
            )));
        }
    }
    .map_err(|error| AudioError::Backend(format!("failed to build the input stream: {error}")))?;
    stream.play().map_err(|error| {
        AudioError::Backend(format!("failed to start the input stream: {error}"))
    })?;
    log::debug!(
        "cranpose audio: cpal input at {} Hz, {} channels",
        format.sample_rate,
        format.channels
    );
    let frames_per_burst = stream.buffer_size().ok().filter(|&frames| frames > 0);
    Ok(Box::new(CpalSource {
        _stream: stream,
        latency,
        frames_per_burst,
    }))
}

// How long before this callback the device captured its first frame, published for the source.
// A host that gives no capture time reports the two instants as equal, and that is no age.
fn age(latency: &AtomicDuration, info: &cpal::InputCallbackInfo) -> Option<Duration> {
    let timestamp = info.timestamp();
    let age = timestamp
        .callback
        .saturating_duration_since(timestamp.capture);
    if age.is_zero() {
        return None;
    }
    latency.store(age);
    Some(age)
}

// Hands integer samples to the recorder as floats, a scratch's worth at a time. Each later chunk
// starts that many frames after the first, so its capture time is that much later.
fn record_integers<T: Copy>(
    recorder: &mut dyn Recorder,
    scratch: &mut [f32],
    data: &[T],
    format: InputFormat,
    age: Option<Duration>,
    convert: impl Fn(T) -> f32,
) {
    let chunk = (scratch.len() / format.channels) * format.channels;
    if chunk == 0 {
        return;
    }
    for (index, samples) in data.chunks(chunk).enumerate() {
        let frames_before = (index * chunk / format.channels) as f64;
        let offset =
            Duration::from_secs_f64(frames_before / f64::from(format.sample_rate.max(1.0)));
        let captured = age
            .and_then(|age| age.checked_sub(offset))
            .and_then(captured_at);
        for (out, &sample) in scratch.iter_mut().zip(samples) {
            *out = convert(sample);
        }
        recorder.record(&scratch[..samples.len()], format, captured);
    }
}

struct CpalSource {
    _stream: cpal::Stream,
    latency: Arc<AtomicDuration>,
    frames_per_burst: Option<u32>,
}

impl AudioSource for CpalSource {
    fn input_latency(&self) -> Option<Duration> {
        self.latency.load()
    }

    fn frames_per_burst(&self) -> Option<u32> {
        self.frames_per_burst
    }
}
