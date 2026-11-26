//! Shared feeder-to-worker command mailbox and threaded backend for Windows and Linux decoders.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use parking_lot::{Condvar, Mutex};

use super::{
    AccessUnit, ColorPrimaries, ColorRange, ColorTransfer, DecodeOrder, DecoderConfig,
    DecoderError, DecoderOutput, VideoCodec, VideoColorInfo, VideoFrame, VideoRotation,
    admission::Admission,
    annex_b::AccessUnitNals,
    frame_queue::{FrameQueue, Frames},
    sps::StreamMetadata,
};

pub(super) const MAX_QUEUED_UNITS: usize = 8;
pub(super) const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);

pub(super) struct QueuedUnit {
    pub(super) data: Vec<u8>,
    pub(super) pts_us: u64,
    pub(super) timestamp: Duration,
    pub(super) metadata: u64,
    pub(super) submitted: Instant,
}

pub(super) enum Command {
    Unit(QueuedUnit),
    Drain(Arc<Completion>),
    Flush(Arc<Completion>),
}

#[derive(Default)]
pub(super) struct Completion {
    done: Mutex<Option<Result<(), DecoderError>>>,
    ready: Condvar,
}

impl Completion {
    pub(super) fn finish(&self, result: Result<(), DecoderError>) {
        *self.done.lock() = Some(result);
        self.ready.notify_all();
    }

    pub(super) fn wait(&self, timeout: Duration) -> Result<(), DecoderError> {
        let deadline = Instant::now() + timeout;
        let mut guard = self.done.lock();
        while guard.is_none() {
            if self.ready.wait_until(&mut guard, deadline).timed_out() {
                return Err(DecoderError::Failed("decoder command timed out".into()));
            }
        }
        guard.take().unwrap_or(Ok(()))
    }
}

pub(super) struct Mailbox {
    pub(super) commands: VecDeque<Command>,
    pub(super) stopped: bool,
}

pub(super) struct WorkerShared {
    pub(super) mailbox: Mutex<Mailbox>,
    pub(super) wake_worker: Condvar,
    pub(super) wake_feeder: Condvar,
    pub(super) queue: Arc<FrameQueue>,
    pub(super) live: Arc<AtomicUsize>,
    pub(super) depth: usize,
    pub(super) order: DecodeOrder,
    pub(super) rotation: VideoRotation,
    pub(super) config_color: Option<VideoColorInfo>,
    pub(super) stream_meta: Mutex<StreamMetadata>,
    pub(super) stop: AtomicBool,
}

impl WorkerShared {
    pub(super) fn new(
        depth: usize,
        order: DecodeOrder,
        rotation: VideoRotation,
        config_color: Option<VideoColorInfo>,
    ) -> Arc<Self> {
        Arc::new(Self {
            mailbox: Mutex::new(Mailbox {
                commands: VecDeque::with_capacity(MAX_QUEUED_UNITS + 2),
                stopped: false,
            }),
            wake_worker: Condvar::new(),
            wake_feeder: Condvar::new(),
            queue: Arc::new(FrameQueue::new(depth, 0)),
            live: Arc::new(AtomicUsize::new(0)),
            depth,
            order,
            rotation,
            config_color,
            stream_meta: Mutex::new(StreamMetadata::default()),
            stop: AtomicBool::new(false),
        })
    }

    pub(super) fn next_command_with(
        &self,
        poll_wait: Option<Duration>,
        mut on_idle: impl FnMut(),
    ) -> Option<Command> {
        let mut mailbox = self.mailbox.lock();
        loop {
            if self.stop.load(Ordering::Acquire) || mailbox.stopped {
                return None;
            }
            if let Some(cmd) = mailbox.commands.pop_front() {
                self.wake_feeder.notify_one();
                return Some(cmd);
            }
            if let Some(wait) = poll_wait {
                self.wake_worker.wait_for(&mut mailbox, wait);
                drop(mailbox);
                on_idle();
                mailbox = self.mailbox.lock();
            } else {
                self.wake_worker.wait(&mut mailbox);
            }
        }
    }

    pub(super) fn enqueue_frame(&self, frame: VideoFrame) {
        let mut frames = self.queue.frames.lock();
        while self.live.load(Ordering::Relaxed) > self.depth {
            self.queue.stats.dropped();
            if frames.pop_front().is_none() {
                return;
            }
        }
        frames.push_back(frame);
        drop(frames);
        self.queue.wake();
    }

    pub(super) fn resolve_color(
        &self,
        primaries: Option<ColorPrimaries>,
        range: Option<ColorRange>,
        transfer: Option<ColorTransfer>,
        height: u32,
    ) -> VideoColorInfo {
        let stream = *self.stream_meta.lock();
        VideoColorInfo {
            primaries: primaries
                .or(stream.primaries)
                .or_else(|| self.config_color.map(|c| c.primaries))
                .unwrap_or_else(|| ColorPrimaries::default_for_height(height)),
            range: range
                .or(stream.range)
                .or_else(|| self.config_color.map(|c| c.range))
                .unwrap_or(ColorRange::Limited),
            transfer: transfer
                .or(stream.transfer)
                .or_else(|| self.config_color.map(|c| c.transfer))
                .unwrap_or(ColorTransfer::Sdr),
        }
    }

    pub(super) fn enqueue_unit(
        &self,
        unit: AccessUnit<'_>,
        pts_us: u64,
        wait: Duration,
    ) -> Result<(), DecoderError> {
        let queued = QueuedUnit {
            data: unit.data.to_vec(),
            pts_us,
            timestamp: unit.timestamp,
            metadata: unit.metadata,
            submitted: Instant::now(),
        };
        let deadline = Instant::now() + wait;
        let mut mailbox = self.mailbox.lock();
        while !mailbox.stopped && mailbox.commands.len() >= MAX_QUEUED_UNITS {
            if self
                .wake_feeder
                .wait_until(&mut mailbox, deadline)
                .timed_out()
            {
                self.queue.stats.refused();
                return Err(DecoderError::Full);
            }
        }
        if mailbox.stopped {
            return Err(DecoderError::Failed("decoder worker stopped".into()));
        }
        mailbox.commands.push_back(Command::Unit(queued));
        drop(mailbox);
        self.wake_worker.notify_one();
        Ok(())
    }

    pub(super) fn request_drain(&self) -> Result<(), DecoderError> {
        let completion = Arc::new(Completion::default());
        {
            let mut mailbox = self.mailbox.lock();
            if mailbox.stopped {
                return Err(DecoderError::Failed("decoder worker stopped".into()));
            }
            mailbox
                .commands
                .push_back(Command::Drain(Arc::clone(&completion)));
        }
        self.wake_worker.notify_one();
        completion.wait(COMMAND_TIMEOUT)
    }

    pub(super) fn request_flush(&self) -> Result<(), DecoderError> {
        let completion = Arc::new(Completion::default());
        {
            let mut mailbox = self.mailbox.lock();
            if mailbox.stopped {
                return Err(DecoderError::Failed("decoder worker stopped".into()));
            }
            mailbox
                .commands
                .retain(|cmd| !matches!(cmd, Command::Unit(_)));
            mailbox
                .commands
                .push_back(Command::Flush(Arc::clone(&completion)));
        }
        self.wake_worker.notify_one();
        completion.wait(COMMAND_TIMEOUT)
    }

    pub(super) fn shutdown(&self) {
        self.stop.store(true, Ordering::Release);
        self.mailbox.lock().stopped = true;
        self.wake_worker.notify_all();
        self.wake_feeder.notify_all();
    }

    pub(super) fn mark_worker_stopped(&self) {
        self.mailbox.lock().stopped = true;
        self.wake_feeder.notify_all();
    }
}

pub(super) struct ThreadedBackend {
    shared: Arc<WorkerShared>,
    codec: VideoCodec,
    nals: AccessUnitNals,
    admission: Admission,
    input_wait: Duration,
    worker: Option<JoinHandle<()>>,
}

impl ThreadedBackend {
    pub(super) fn spawn<S>(
        config: DecoderConfig,
        thread_name: &str,
        init: impl FnOnce(
            VideoCodec,
            (u32, u32),
            DecodeOrder,
            Arc<WorkerShared>,
        ) -> Result<S, DecoderError>
        + Send
        + 'static,
        run: impl FnOnce(&mut S) + Send + 'static,
    ) -> Result<Self, DecoderError> {
        let DecoderConfig {
            codec,
            width,
            height,
            output,
            input_wait,
            order,
            rotation,
            color,
        } = config;
        let DecoderOutput::Frames { depth } = output else {
            return Err(DecoderError::Unavailable);
        };
        let depth = depth.max(2);
        let shared = WorkerShared::new(depth, order, rotation, color);
        let init_done = Arc::new(Completion::default());
        let worker_shared = Arc::clone(&shared);
        let worker_init = Arc::clone(&init_done);
        let worker = thread::Builder::new()
            .name(thread_name.into())
            .spawn(move || {
                match init(codec, (width, height), order, Arc::clone(&worker_shared)) {
                    Ok(mut session) => {
                        worker_init.finish(Ok(()));
                        run(&mut session);
                    }
                    Err(error) => {
                        worker_shared.mark_worker_stopped();
                        worker_init.finish(Err(error));
                        return;
                    }
                }
                worker_shared.mark_worker_stopped();
            })
            .map_err(|error| DecoderError::Failed(format!("decoder thread: {error}")))?;
        init_done.wait(COMMAND_TIMEOUT)?;
        Ok(Self {
            shared,
            codec,
            nals: AccessUnitNals::default(),
            admission: Admission::new((width as usize) * (height as usize), order),
            input_wait,
            worker: Some(worker),
        })
    }

    pub(super) fn submit(&mut self, unit: AccessUnit<'_>) -> Result<(), DecoderError> {
        let pts_us = self.admission.check(&unit)?;
        if !matches!(self.codec, VideoCodec::Av1 | VideoCodec::Vp9) {
            self.nals.parse(self.codec, unit.data);
            if let Some(sps) = self.nals.sequence_parameter_set(unit.data)
                && let Some(meta) = StreamMetadata::parse_sps(self.codec, sps)
            {
                *self.shared.stream_meta.lock() = meta;
            }
        }
        self.shared.enqueue_unit(unit, pts_us, self.input_wait)?;
        self.admission.accept(pts_us);
        self.shared.queue.stats.submitted();
        Ok(())
    }

    pub(super) fn drain(&mut self) -> Result<(), DecoderError> {
        self.shared.request_drain()?;
        self.admission.reset();
        Ok(())
    }

    pub(super) fn flush(&mut self) -> Result<(), DecoderError> {
        self.shared.request_flush()?;
        self.admission.reset();
        Ok(())
    }

    pub(super) fn frames(&self) -> Frames {
        Frames(Arc::clone(&self.shared.queue))
    }
}

impl Drop for ThreadedBackend {
    fn drop(&mut self) {
        self.shared.shutdown();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
