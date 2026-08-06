//! The Android output device: AAudio through the `ndk` crate.
//!
//! AAudio is the NDK's native audio API (API 26 and up; Wear OS 3 is API 30),
//! so this backend needs no Java on the activity side and no C++ toolchain —
//! `ndk::audio` is a pure-Rust binding over `libaaudio.so`, which every Android
//! and Wear OS device ships.
//!
//! The stream asks for `LowLatency` with a 32-bit float, two-channel format.
//! AAudio calls back on a real-time thread it owns; the callback below does
//! exactly two things — read the negotiated format off the stream handle and
//! run the mixer — so the real-time budget holds.
//!
//! The crate root denies unsafe code; this module opts back in for the one
//! place it is unavoidable: turning AAudio's raw output pointer into a slice.
#![allow(unsafe_code)]

use crate::backend::AudioSink;
use crate::mixer::{Mixer, MixerSeed};
use cranpose_services::AudioError;
use ndk::audio::{
    AudioCallbackResult, AudioDirection, AudioError as AAudioError, AudioFormat,
    AudioPerformanceMode, AudioSharingMode, AudioStream, AudioStreamBuilder,
};
use std::ffi::c_void;

/// The rate the mixer assumes until the stream reports the one it negotiated.
const NOMINAL_SAMPLE_RATE: f32 = 48_000.0;
const REQUESTED_CHANNELS: i32 = 2;

pub(crate) fn open(seed: MixerSeed) -> Result<Box<dyn AudioSink>, AudioError> {
    let mut mixer = Mixer::new(seed, NOMINAL_SAMPLE_RATE, REQUESTED_CHANNELS as usize);

    let stream = AudioStreamBuilder::new()
        .map_err(backend_error("failed to create an AAudio stream builder"))?
        .direction(AudioDirection::Output)
        .format(AudioFormat::PCM_Float)
        .channel_count(REQUESTED_CHANNELS)
        .performance_mode(AudioPerformanceMode::LowLatency)
        .sharing_mode(AudioSharingMode::Shared)
        // `setUsage` and `setContentType` are deliberately left at their
        // defaults: both entered the NDK at API 28, and calling them would
        // raise this backend's floor above AAudio's own API 26.
        .data_callback(Box::new(
            move |stream: &AudioStream, audio_data: *mut c_void, frames: i32| {
                // Both getters read a field on the stream handle; neither locks
                // nor allocates, and `set_device_format` returns immediately
                // once the format matches what the mixer already assumed.
                let channels = stream.channel_count().max(1) as usize;
                mixer.set_device_format(stream.sample_rate() as f32, channels);

                let samples = frames.max(0) as usize * channels;
                if samples == 0 || audio_data.is_null() {
                    return AudioCallbackResult::Continue;
                }
                let data = audio_data.cast::<f32>();
                // SAFETY: AAudio guarantees `data` points at `frames *
                // channelCount` writable float samples for this call only, and
                // the stream is verified as 32-bit float before it is started.
                let out = unsafe { std::slice::from_raw_parts_mut(data, samples) };
                mixer.render(out);
                AudioCallbackResult::Continue
            },
        ))
        .error_callback(Box::new(|_stream, error| {
            // Runs on an AAudio worker thread, not the real-time one, so this
            // may log. A disconnect ends the stream; the engine reopens the
            // device the next time it is asked to play.
            log::warn!("AAudio stream error: {error:?}");
        }))
        .open_stream()
        .map_err(backend_error("failed to open the AAudio output stream"))?;

    if stream.format() != AudioFormat::PCM_Float {
        return Err(AudioError::Backend(format!(
            "AAudio opened the output stream as {:?} rather than 32-bit float",
            stream.format()
        )));
    }

    stream
        .request_start()
        .map_err(backend_error("failed to start the AAudio output stream"))?;

    log::debug!(
        "cranpose audio: AAudio stream at {} Hz, {} channels, {} frames per burst",
        stream.sample_rate(),
        stream.channel_count(),
        stream.frames_per_burst()
    );

    Ok(Box::new(AAudioSink { stream }))
}

fn backend_error(context: &'static str) -> impl Fn(AAudioError) -> AudioError {
    move |error| AudioError::Backend(format!("{context}: {error}"))
}

struct AAudioSink {
    stream: AudioStream,
}

impl AudioSink for AAudioSink {
    fn suspend(&self) {
        if let Err(error) = self.stream.request_pause() {
            log::debug!("failed to pause the AAudio stream: {error}");
        }
    }

    fn resume(&self) {
        if let Err(error) = self.stream.request_start() {
            log::debug!("failed to restart the AAudio stream: {error}");
        }
    }
}

impl Drop for AAudioSink {
    fn drop(&mut self) {
        // Stopping before the stream closes means the callback has returned for
        // the last time, so the mixer it owns is dropped on this thread.
        if let Err(error) = self.stream.request_stop() {
            log::debug!("failed to stop the AAudio stream: {error}");
        }
    }
}
