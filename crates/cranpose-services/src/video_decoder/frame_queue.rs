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
        Poll::Pending
    }

    pub(super) fn next_report(&self) -> Option<FrameInfo> {
        self.0.reports.lock().pop_front()
    }

    pub(super) fn stats(&self) -> DecoderStats {
        self.0.stats.snapshot()
    }
}
