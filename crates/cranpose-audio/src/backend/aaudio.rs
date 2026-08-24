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
//! The stream is not kept running for its own sake. A `LowLatency` output on
//! Android is an MMAP route with the always-on audio DSP behind it, and it
//! costs power whether or not the samples crossing it are zero, so when the
//! mixer reports that nothing has sounded for a while the callback returns
//! [`AudioCallbackResult::Stop`] and the engine starts the stream again on the
//! next sound. That is AAudio's own idiom for a stream with nothing to do.
//!
//! The crate root denies unsafe code; this module opts back in for the one
//! place it is unavoidable: turning AAudio's raw output pointer into a slice.
#![allow(unsafe_code)]

use std::{
    ffi::c_void,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
};

use cranpose_services::AudioError;
use ndk::audio::{
    AudioCallbackResult, AudioDirection, AudioError as AAudioError, AudioFormat,
    AudioPerformanceMode, AudioSharingMode, AudioStream, AudioStreamBuilder, AudioStreamState,
};

use crate::{
    backend::AudioSink,
    mixer::{Mixer, MixerSeed, RenderStatus},
};

/// The rate the mixer assumes until the stream reports the one it negotiated.
const NOMINAL_SAMPLE_RATE: f32 = 48_000.0;
const REQUESTED_CHANNELS: i32 = 2;

pub(crate) fn open(seed: MixerSeed) -> Result<Box<dyn AudioSink>, AudioError> {
    let connected = Arc::new(AtomicBool::new(true));
    let (requests, request_rx) = mpsc::channel();
    let (startup_tx, startup_rx) = mpsc::sync_channel(1);
    let worker_connected = Arc::clone(&connected);
    let worker = thread::Builder::new()
        .name("cranpose-aaudio".into())
        .spawn(move || run_worker(seed, worker_connected, request_rx, startup_tx))
        .map_err(|error| {
            AudioError::Backend(format!("failed to start the AAudio owner thread: {error}"))
        })?;

    match startup_rx.recv() {
        Ok(Ok(())) => Ok(Box::new(AAudioSink {
            requests: Some(requests),
            connected,
            worker: Some(worker),
        })),
        Ok(Err(error)) => {
            join_worker(worker);
            Err(error)
        }
        Err(_) => {
            join_worker(worker);
            Err(AudioError::Backend(
                "AAudio owner thread stopped during startup".into(),
            ))
        }
    }
}

fn run_worker(
    seed: MixerSeed,
    connected: Arc<AtomicBool>,
    requests: mpsc::Receiver<StreamRequest>,
    startup: mpsc::SyncSender<Result<(), AudioError>>,
) {
    let stream = match open_stream(seed, Arc::clone(&connected)) {
        Ok(stream) => stream,
        Err(error) => {
            connected.store(false, Ordering::Release);
            let _ = startup.send(Err(error));
            return;
        }
    };

    if !stream_is_usable(&stream, &connected) {
        let _ = startup.send(Err(AudioError::Backend(
            "AAudio stream disconnected during startup".into(),
        )));
        return;
    }
    if startup.send(Ok(())).is_err() {
        stop_stream(&stream);
        return;
    }

    while let Ok(request) = requests.recv() {
        let available = apply_operation(&stream, request.operation, &connected);
        let _ = request.completed.send(available);
    }

    connected.store(false, Ordering::Release);
    stop_stream(&stream);
}

fn open_stream(seed: MixerSeed, connected: Arc<AtomicBool>) -> Result<AudioStream, AudioError> {
    let mut mixer = Mixer::new(seed, NOMINAL_SAMPLE_RATE, REQUESTED_CHANNELS as usize);
    let error_connected = Arc::clone(&connected);

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
                match mixer.render(out) {
                    RenderStatus::Continue => AudioCallbackResult::Continue,
                    // Returning `Stop` is what ends this real-time thread; the
                    // engine also calls `request_stop` from the UI thread (see
                    // `AAudioSink::park`) because AAudio only guarantees that
                    // `Stop` ends the callback, not that it releases the route.
                    RenderStatus::Idle => AudioCallbackResult::Stop,
                }
            },
        ))
        .error_callback(Box::new(move |_stream, error| {
            // Runs on an AAudio worker thread, not the real-time one, so this
            // may log. A disconnect ends the stream; the engine reopens the
            // device the next time it is asked to play.
            error_connected.store(false, Ordering::Release);
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

    Ok(stream)
}

fn backend_error(context: &'static str) -> impl Fn(AAudioError) -> AudioError {
    move |error| AudioError::Backend(format!("{context}: {error}"))
}

#[derive(Clone, Copy)]
enum StreamOperation {
    Suspend,
    Resume,
    IsRunning,
    Park,
}

struct StreamRequest {
    operation: StreamOperation,
    completed: mpsc::SyncSender<bool>,
}

struct AAudioSink {
    requests: Option<mpsc::Sender<StreamRequest>>,
    connected: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl AAudioSink {
    fn request(&self, operation: StreamOperation) -> bool {
        let Some(requests) = self.requests.as_ref() else {
            return false;
        };
        let (completed, result) = mpsc::sync_channel(1);
        if requests
            .send(StreamRequest {
                operation,
                completed,
            })
            .is_err()
        {
            self.connected.store(false, Ordering::Release);
            return false;
        }
        result.recv().unwrap_or_else(|_| {
            self.connected.store(false, Ordering::Release);
            false
        })
    }
}

impl AudioSink for AAudioSink {
    fn suspend(&self) {
        self.request(StreamOperation::Suspend);
    }

    fn resume(&self) {
        self.request(StreamOperation::Resume);
    }

    fn is_running(&self) -> bool {
        self.connected.load(Ordering::Acquire) && self.request(StreamOperation::IsRunning)
    }

    fn park(&self) {
        self.request(StreamOperation::Park);
    }
}

impl Drop for AAudioSink {
    fn drop(&mut self) {
        self.requests.take();
        if let Some(worker) = self.worker.take() {
            join_worker(worker);
        }
    }
}

fn apply_operation(
    stream: &AudioStream,
    operation: StreamOperation,
    connected: &AtomicBool,
) -> bool {
    match operation {
        StreamOperation::Suspend => {
            if let Err(error) = stream.request_pause() {
                log::warn!("failed to pause the AAudio stream: {error}");
            }
        }
        StreamOperation::Resume => {
            // A stream the data callback gave up by returning `Stop` may still
            // be in `Started`: on Android 8 that return only ended the
            // callback, and the internal stop that later releases added does
            // not exist there. `request_start` is rejected in that state,
            // which would leave the app permanently silent, so the stop is
            // made explicit first. This is a no-op on a stream that really did
            // stop, and it does not fire on the lifecycle path, where
            // `suspend` left the stream paused.
            if stream.state() == AudioStreamState::Started {
                if let Err(error) = stream.request_stop() {
                    log::warn!("failed to settle the AAudio stream before starting it: {error}");
                }
            }
            if let Err(error) = stream.request_start() {
                log::warn!("failed to restart the AAudio stream: {error}");
            }
        }
        StreamOperation::IsRunning => {}
        StreamOperation::Park => {
            // `request_stop` rather than `request_pause`: a paused stream
            // keeps its route, which is exactly the thing costing power.
            // Stopping an already stopped stream is a no-op in AAudio, so this
            // stays correct on the releases where returning `Stop` from the
            // callback already tore the stream down.
            if let Err(error) = stream.request_stop() {
                log::warn!("failed to release the idle AAudio stream: {error}");
            }
        }
    }
    stream_is_usable(stream, connected)
}

fn stream_is_usable(stream: &AudioStream, connected: &AtomicBool) -> bool {
    let usable = connected.load(Ordering::Acquire)
        && !matches!(
            stream.state(),
            AudioStreamState::Uninitialized
                | AudioStreamState::Closing
                | AudioStreamState::Closed
                | AudioStreamState::Disconnected
        );
    if !usable {
        connected.store(false, Ordering::Release);
    }
    usable
}

fn stop_stream(stream: &AudioStream) {
    // Stopping before the stream closes means the callback has returned for
    // the last time, so the mixer it owns is dropped on its owner thread.
    if let Err(error) = stream.request_stop() {
        log::warn!("failed to stop the AAudio stream: {error}");
    }
}

fn join_worker(worker: JoinHandle<()>) {
    if worker.join().is_err() {
        log::error!("AAudio owner thread panicked");
    }
}
