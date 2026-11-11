use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};

use cranpose_services::AudioError;

use super::output_stream::{OutputStream, StreamState};
use crate::backend::{AudioSink, Renderer};

pub(crate) fn open(renderer: Box<dyn Renderer>) -> Result<Box<dyn AudioSink>, AudioError> {
    let connected = Arc::new(AtomicBool::new(true));
    let (requests, request_rx) = mpsc::channel();
    let (startup_tx, startup_rx) = mpsc::sync_channel(1);
    let worker_connected = Arc::clone(&connected);
    let worker = thread::Builder::new()
        .name("cranpose-aaudio".into())
        .spawn(move || run_worker(renderer, worker_connected, request_rx, startup_tx))
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
    renderer: Box<dyn Renderer>,
    connected: Arc<AtomicBool>,
    requests: mpsc::Receiver<StreamRequest>,
    startup: mpsc::SyncSender<Result<(), AudioError>>,
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
