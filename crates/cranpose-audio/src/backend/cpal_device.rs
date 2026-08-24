//! The desktop output device: `cpal` (CoreAudio, WASAPI, ALSA).
//!
//! This exists so a developer on macOS, Windows or Linux hears the same mix the
//! device will produce. It shares the mixer with the Android backend, so the
//! only thing that differs between the two is how the callback arrives.
//!
//! Linux builds link ALSA, which needs `libasound2-dev` (or the distribution's
//! equivalent) present at build time. That is why the feature is off by
//! default.
//!
//! One thing does differ, and it is worth stating rather than hiding: a cpal
//! data callback returns nothing, so unlike AAudio it cannot give the device
//! up from the inside. The mixer still publishes that it has gone idle, and
//! the engine pauses the stream from the UI thread on its next call — which
//! means a desktop app that plays a sound and then never touches the engine
//! again keeps its stream open a while longer than an Android one would. That
//! is a deliberate trade: desktop has no always-on audio DSP to keep awake,
//! and a timer thread to close the gap would cost more than it saves.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cranpose_services::AudioError;

use crate::{
    backend::AudioSink,
    mixer::{Mixer, MixerSeed, RenderStatus},
};

/// Scratch used when the device wants integer samples. Allocated once, before
/// the stream starts, and reused for every callback.
const SCRATCH_SAMPLES: usize = 8192;

pub(crate) fn open(seed: MixerSeed) -> Result<Box<dyn AudioSink>, AudioError> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or_else(|| {
        AudioError::Backend("the system reports no default audio output device".to_string())
    })?;
    let supported = device
        .default_output_config()
        .map_err(|error| AudioError::Backend(format!("no usable output configuration: {error}")))?;
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let channels = usize::from(config.channels).max(1);
    let sample_rate = config.sample_rate as f32;
    let mut mixer = Mixer::new(seed, sample_rate, channels);

    let error_callback = |error| log::warn!("cranpose audio stream error: {error}");

    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_output_stream(
            config,
            // The idle verdict is dropped here on purpose: `render` has already
            // published it for the engine, and this callback has no way to stop
            // its own stream. See the module comment.
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let _ = mixer.render(data);
            },
            error_callback,
            None,
        ),
        cpal::SampleFormat::I16 => {
            let mut scratch = vec![0.0f32; SCRATCH_SAMPLES];
            device.build_output_stream(
                config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    let _ =
                        render_into_integer(&mut mixer, &mut scratch, data, channels, |sample| {
                            (sample * f32::from(i16::MAX)) as i16
                        });
                },
                error_callback,
                None,
            )
        }
        other => return Err(unwritable_sample_format(other)),
    }
    .map_err(|error| AudioError::Backend(format!("failed to build the output stream: {error}")))?;

    stream.play().map_err(|error| {
        AudioError::Backend(format!("failed to start the output stream: {error}"))
    })?;

    log::debug!("cranpose audio: cpal stream at {sample_rate} Hz, {channels} channels");
    Ok(Box::new(CpalSink { stream }))
}

fn unwritable_sample_format(format: cpal::SampleFormat) -> AudioError {
    AudioError::Backend(format!(
        "the default output device wants {format:?} samples, which the engine does not write"
    ))
}

/// Renders through the preallocated scratch buffer and converts, in whole
/// frames, so no allocation happens inside the callback.
///
/// The verdict of the last chunk is the one that counts: an earlier chunk that
/// still had a voice in it is precisely what stops the mixer going idle.
fn render_into_integer<T>(
    mixer: &mut Mixer,
    scratch: &mut [f32],
    data: &mut [T],
    channels: usize,
    convert: impl Fn(f32) -> T,
) -> RenderStatus {
    let chunk = (scratch.len() / channels) * channels;
    if chunk == 0 {
        return RenderStatus::Continue;
    }
    let mut status = RenderStatus::Continue;
    let mut offset = 0;
    while offset < data.len() {
        let take = (data.len() - offset).min(chunk);
        status = mixer.render(&mut scratch[..take]);
        for index in 0..take {
            data[offset + index] = convert(scratch[index]);
        }
        offset += take;
    }
    status
}

struct CpalSink {
    stream: cpal::Stream,
}

impl AudioSink for CpalSink {
    fn suspend(&self) {
        if let Err(error) = self.stream.pause() {
            log::warn!("failed to pause the output stream: {error}");
        }
    }

    fn resume(&self) {
        if let Err(error) = self.stream.play() {
            log::warn!("failed to restart the output stream: {error}");
        }
    }

    fn park(&self) {
        // cpal has no stop, only pause; it releases the callback thread, which
        // is the part that costs anything on a desktop.
        if let Err(error) = self.stream.pause() {
            log::warn!("failed to release the idle output stream: {error}");
        }
    }
}
