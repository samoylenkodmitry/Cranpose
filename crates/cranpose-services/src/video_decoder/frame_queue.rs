//! The pictures that a decoder has output and the app has not taken.

use std::{collections::VecDeque, sync::Arc};

use parking_lot::Mutex;

use super::{DecoderStats, FrameInfo, VideoFrame, stats::StatsCounters};

// The queues and the counters that the decoder's threads and the app's handles share.
pub(super) struct FrameQueue {
    pub(super) frames: Mutex<VecDeque<VideoFrame>>,
    pub(super) reports: Mutex<VecDeque<FrameInfo>>,
    pub(super) stats: StatsCounters,
}

impl FrameQueue {
    pub(super) fn new(frames: usize, reports: usize) -> Self {
        Self {
            frames: Mutex::new(VecDeque::with_capacity(frames)),
            reports: Mutex::new(VecDeque::with_capacity(reports)),
            stats: StatsCounters::default(),
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

    pub(super) fn next_report(&self) -> Option<FrameInfo> {
        self.0.reports.lock().pop_front()
    }

    pub(super) fn stats(&self) -> DecoderStats {
        self.0.stats.snapshot()
    }
}
