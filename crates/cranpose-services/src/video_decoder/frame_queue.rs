//! The pictures that a decoder has output and the app has not taken.

use std::{
    collections::VecDeque,
    sync::Arc,
    task::{Context, Poll, Waker},
};

use parking_lot::Mutex;

use super::{DecoderStats, FrameInfo, VideoFrame, stats::StatsCounters};

// The queues and the counters that the decoder's threads and the app's handles share.
pub(super) struct FrameQueue {
    pub(super) frames: Mutex<VecDeque<VideoFrame>>,
    pub(super) reports: Mutex<VecDeque<FrameInfo>>,
    pub(super) stats: StatsCounters,
    // The task that waits for a picture
    waker: Mutex<Option<Waker>>,
}

impl FrameQueue {
    pub(super) fn new(frames: usize, reports: usize) -> Self {
        Self {
            frames: Mutex::new(VecDeque::with_capacity(frames)),
            reports: Mutex::new(VecDeque::with_capacity(reports)),
            stats: StatsCounters::default(),
            waker: Mutex::new(None),
        }
    }

    // Wakes the task that waits for a picture. A decoder calls it after a picture joined
    // `frames` and the lock is free.
    pub(super) fn wake(&self) {
        if let Some(waker) = self.waker.lock().take() {
            waker.wake();
        }
    }
}

// The receiving side of a decoder.
#[derive(Clone)]
pub(super) struct Frames(pub(super) Arc<FrameQueue>);

impl PartialEq for Frames {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Frames {
    pub(super) fn next_frame(&self) -> Option<VideoFrame> {
        self.0.frames.lock().pop_front()
    }

    pub(super) fn latest_frame(&self) -> Option<VideoFrame> {
        let mut frames = self.0.frames.lock();
        while frames.len() > 1 {
            frames.pop_front();
            self.0.stats.dropped();
        }
        frames.pop_front()
    }

    // Ready when a picture waits. Otherwise the task wakes when the decoder queues one. The
    // waker is stored under the queue's lock, so a picture that arrives at once is not missed.
    pub(super) fn poll_ready(&self, context: &mut Context<'_>) -> Poll<()> {
        let frames = self.0.frames.lock();
        if !frames.is_empty() {
            return Poll::Ready(());
        }
        *self.0.waker.lock() = Some(context.waker().clone());
        drop(frames);
        Poll::Pending
    }

    pub(super) fn next_report(&self) -> Option<FrameInfo> {
        self.0.reports.lock().pop_front()
    }

    pub(super) fn stats(&self) -> DecoderStats {
        self.0.stats.snapshot()
    }
}

/// Producer handle pushing zero-copy [`VideoFrame`]s into a paired [`DecodedFrames`](super::DecodedFrames)
/// queue, waking `PlayerSurface` on arrival without polling.
#[derive(Clone)]
pub struct FrameProducer {
    queue: Arc<FrameQueue>,
    max_depth: usize,
}

impl FrameProducer {
    pub(super) fn channel(depth: usize) -> (Self, Frames) {
        let max_depth = depth.max(1);
        let queue = Arc::new(FrameQueue::new(max_depth + 1, 0));
        let consumer = Frames(Arc::clone(&queue));
        (Self { queue, max_depth }, consumer)
    }

    /// Enqueues a newly decoded [`VideoFrame`], dropping the oldest unread frame if the bounded
    /// queue is full, and wakes any waiting `PlayerSurface` consumer.
    pub fn push(&self, frame: VideoFrame) {
        let decode_time = frame.info().decode_time;
        let mut frames = self.queue.frames.lock();
        while frames.len() >= self.max_depth {
            frames.pop_front();
            self.queue.stats.dropped();
        }
        frames.push_back(frame);
        drop(frames);
        self.queue.stats.decoded(decode_time);
        self.queue.wake();
    }

    /// Discards all unread queued frames (for example on seek, stop, or media item transition).
    pub fn clear(&self) {
        self.queue.frames.lock().clear();
    }
}
