use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use cranpose_services::AudioError;

use super::input_stream::{InputStream, Timing};
use crate::backend::microphone::{AudioSource, InputPreset, Recorder};

// How often the owner thread reads the stream's timestamp and overrun count while the stream
// runs. The capture time drifts with the clock of the microphone, which stays within a few
// parts in a million of the system's clock: a quarter of a second keeps it within a microsecond.
const READING_INTERVAL: Duration = Duration::from_millis(250);
// How often the owner thread asks for the first timestamp, which AAudio gives a short time after
// the start. Until then the recorder gets no capture time.
const FIRST_READING_INTERVAL: Duration = Duration::from_millis(10);

pub(crate) fn open(
    recorder: Box<dyn Recorder>,
    preset: InputPreset,
) -> Result<Box<dyn AudioSource>, AudioError> {
    let connected = Arc::new(AtomicBool::new(true));
    let timing = Arc::new(Timing::new());
    let overruns = Arc::new(AtomicU32::new(0));
    let (stop, stopped) = mpsc::channel::<()>();
    let (startup_tx, startup_rx) = mpsc::sync_channel(1);
    let worker = {
        let (connected, timing, overruns) = (
            Arc::clone(&connected),
            Arc::clone(&timing),
            Arc::clone(&overruns),
        );
        thread::Builder::new()
            .name("cranpose-aaudio-input".into())
            .spawn(move || {
                let stream = match start(
                    recorder,
                    preset,
                    Arc::clone(&timing),
                    Arc::clone(&connected),
                ) {
                    Ok(stream) => stream,
                    Err(error) => {
                        connected.store(false, Ordering::Release);
                        let _ = startup_tx.send(Err(error));
                        return;
                    }
                };
                let frames_per_burst = u32::try_from(stream.frames_per_burst())
                    .ok()
                    .filter(|&frames| frames > 0);
                if startup_tx.send(Ok(frames_per_burst)).is_err() {
                    stop_stream(&stream);
                    return;
                }
                let mut interval = FIRST_READING_INTERVAL;
                while let Err(mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(interval) {
                    if stream.publish_timing(&timing) {
                        interval = READING_INTERVAL;
                    }
                    overruns.store(
                        u32::try_from(stream.xrun_count()).unwrap_or(0),
                        Ordering::Relaxed,
                    );
                }
                stop_stream(&stream);
            })
            .map_err(|error| {
                AudioError::Backend(format!("failed to start the AAudio input thread: {error}"))
            })?
    };
    match startup_rx.recv() {
        Ok(Ok(frames_per_burst)) => Ok(Box::new(AAudioSource {
            stop: Some(stop),
            connected,
            timing,
            overruns,
            frames_per_burst,
            worker: Some(worker),
        })),
        Ok(Err(error)) => {
            join_worker(worker);
            Err(error)
        }
        Err(_) => {
            join_worker(worker);
            Err(AudioError::Backend(
                "AAudio input thread stopped during startup".into(),
            ))
        }
    }
}

fn start(
    recorder: Box<dyn Recorder>,
    preset: InputPreset,
    timing: Arc<Timing>,
    connected: Arc<AtomicBool>,
) -> Result<InputStream, AudioError> {
    let stream = InputStream::open(recorder, preset, timing, connected)?;
    stream
        .request_start()
        .map_err(|failure| failure.in_context("failed to start the AAudio input stream"))?;
    log::debug!(
        "cranpose audio: AAudio input at {} Hz, {} channels, {} frames per burst",
        stream.sample_rate(),
        stream.channel_count(),
        stream.frames_per_burst()
    );
    Ok(stream)
}

fn stop_stream(stream: &InputStream) {
    if let Err(error) = stream.request_stop() {
        log::warn!("failed to stop the AAudio input stream: {error}");
    }
}

fn join_worker(worker: JoinHandle<()>) {
    if worker.join().is_err() {
        log::error!("AAudio input thread panicked");
    }
}

struct AAudioSource {
    // Dropping it ends the owner thread, which stops and closes the stream
    stop: Option<mpsc::Sender<()>>,
    connected: Arc<AtomicBool>,
    timing: Arc<Timing>,
    overruns: Arc<AtomicU32>,
    frames_per_burst: Option<u32>,
    worker: Option<JoinHandle<()>>,
}

impl AudioSource for AAudioSource {
    fn is_running(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    fn input_latency(&self) -> Option<Duration> {
        self.timing.age.load()
    }

    fn frames_per_burst(&self) -> Option<u32> {
        self.frames_per_burst
    }

    fn overrun_count(&self) -> Option<u32> {
        Some(self.overruns.load(Ordering::Relaxed))
    }
}

impl Drop for AAudioSource {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(worker) = self.worker.take() {
            join_worker(worker);
        }
    }
}
