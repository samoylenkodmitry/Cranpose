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

use super::{output_stream::OutputStream, stream::StreamState};
use crate::backend::{AudioSink, Renderer, atomic_duration::AtomicDuration};

// How often the owner thread reads the stream's latency and underrun count while the stream
// runs. This crate keeps AAudio's timestamp call out of the real-time callback, so the callback
// does no extra work. A reader on a frame loop does not wait for AAudio either. The owner thread
// makes the calls, and the sink returns the last values. The latency changes over seconds. A
// quarter of a second gives the first value soon after the hardware reports its first timestamp.
const READING_INTERVAL: Duration = Duration::from_millis(250);

// What the owner thread read from the stream last.
struct Readings {
    output_latency: AtomicDuration,
    underruns: AtomicU32,
}

impl Readings {
    fn read(&self, stream: &OutputStream) {
        if let Some(latency) = stream.output_latency() {
            self.output_latency.store(latency);
        }
        let underruns = u32::try_from(stream.underrun_count()).unwrap_or(0);
        self.underruns.store(underruns, Ordering::Relaxed);
    }
}

pub(crate) fn open(renderer: Box<dyn Renderer>) -> Result<Box<dyn AudioSink>, AudioError> {
    let connected = Arc::new(AtomicBool::new(true));
    let readings = Arc::new(Readings {
        output_latency: AtomicDuration::new(),
        underruns: AtomicU32::new(0),
    });
    let (requests, request_rx) = mpsc::channel();
    let (startup_tx, startup_rx) = mpsc::sync_channel(1);
    let worker_connected = Arc::clone(&connected);
    let worker_readings = Arc::clone(&readings);
    let worker = thread::Builder::new()
        .name("cranpose-aaudio".into())
        .spawn(move || {
            run_worker(
                renderer,
                worker_connected,
                worker_readings,
                request_rx,
                startup_tx,
            );
        })
        .map_err(|error| {
            AudioError::Backend(format!("failed to start the AAudio owner thread: {error}"))
        })?;

    match startup_rx.recv() {
        Ok(Ok(frames_per_burst)) => Ok(Box::new(AAudioSink {
            requests: Some(requests),
            connected,
            readings,
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
                "AAudio owner thread stopped during startup".into(),
            ))
        }
    }
}

fn run_worker(
    renderer: Box<dyn Renderer>,
    connected: Arc<AtomicBool>,
    readings: Arc<Readings>,
    requests: mpsc::Receiver<StreamRequest>,
    startup: mpsc::SyncSender<Result<Option<u32>, AudioError>>,
) {
    let stream = match open_stream(renderer, Arc::clone(&connected)) {
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
    // The burst does not change while the stream is open: a new route needs a new stream.
    let frames_per_burst = u32::try_from(stream.frames_per_burst())
        .ok()
        .filter(|&frames| frames > 0);
    if startup.send(Ok(frames_per_burst)).is_err() {
        stop_stream(&stream);
        return;
    }

    loop {
        match wait_for_request(&requests, &stream) {
            Wake::Request(request) => {
                let available = apply_operation(&stream, request.operation, &connected);
                let _ = request.completed.send(available);
            }
            Wake::Interval => {}
            Wake::Closed => break,
        }
        readings.read(&stream);
    }

    connected.store(false, Ordering::Release);
    stop_stream(&stream);
}

enum Wake {
    Request(StreamRequest),
    Interval,
    Closed,
}

// Waits for the next request. A stream that runs wakes the thread at an interval to read it. A
// stream that does not run has nothing new to read, so the thread sleeps until a request comes.
fn wait_for_request(requests: &mpsc::Receiver<StreamRequest>, stream: &OutputStream) -> Wake {
    let state = stream.state();
    if state != StreamState::STARTING && state != StreamState::STARTED {
        return requests.recv().map_or(Wake::Closed, Wake::Request);
    }
    match requests.recv_timeout(READING_INTERVAL) {
        Ok(request) => Wake::Request(request),
        Err(mpsc::RecvTimeoutError::Timeout) => Wake::Interval,
        Err(mpsc::RecvTimeoutError::Disconnected) => Wake::Closed,
    }
}

fn open_stream(
    renderer: Box<dyn Renderer>,
    connected: Arc<AtomicBool>,
) -> Result<OutputStream, AudioError> {
    let stream = OutputStream::open(renderer, connected)?;
    stream
        .request_start()
        .map_err(|failure| failure.in_context("failed to start the AAudio output stream"))?;

    log::debug!(
        "cranpose audio: AAudio stream at {} Hz, {} channels, {} frames per burst",
        stream.sample_rate(),
        stream.channel_count(),
        stream.frames_per_burst()
    );

    Ok(stream)
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
    readings: Arc<Readings>,
    frames_per_burst: Option<u32>,
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

    fn output_latency(&self) -> Option<Duration> {
        self.readings.output_latency.load()
    }

    fn frames_per_burst(&self) -> Option<u32> {
        self.frames_per_burst
    }

    fn underrun_count(&self) -> Option<u32> {
        Some(self.readings.underruns.load(Ordering::Relaxed))
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
    stream: &OutputStream,
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
            if stream.state() == StreamState::STARTED
                && let Err(error) = stream.request_stop()
            {
                log::warn!("failed to settle the AAudio stream before starting it: {error}");
            }
            if let Err(error) = stream.request_start() {
                log::warn!("failed to restart the AAudio stream: {error}");
            }
        }
        StreamOperation::IsRunning => {}
        StreamOperation::Park => {
            if let Err(error) = stream.request_stop() {
                log::warn!("failed to release the idle AAudio stream: {error}");
            }
        }
    }
    stream_is_usable(stream, connected)
}

fn stream_is_usable(stream: &OutputStream, connected: &AtomicBool) -> bool {
    let usable = connected.load(Ordering::Acquire)
        && !matches!(
            stream.state(),
            StreamState::UNINITIALIZED
                | StreamState::CLOSING
                | StreamState::CLOSED
                | StreamState::DISCONNECTED
        );
    if !usable {
        connected.store(false, Ordering::Release);
    }
    usable
}

fn stop_stream(stream: &OutputStream) {
    if let Err(error) = stream.request_stop() {
        log::warn!("failed to stop the AAudio stream: {error}");
    }
}

fn join_worker(worker: JoinHandle<()>) {
    if worker.join().is_err() {
        log::error!("AAudio owner thread panicked");
    }
}
