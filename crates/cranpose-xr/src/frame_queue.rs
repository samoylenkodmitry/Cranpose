//! The decoded frames of a streamed scene that wait to be shown.
//!
//! The host stamps each frame with the time at which its picture should reach the display. At each
//! frame of the headset the queue gives the frame whose stamp lies closest to the display time of
//! the frame being submitted. A frame that no stamp describes follows the rule of a stream with no
//! stamps: the newest frame wins.

use std::{collections::VecDeque, time::Duration};

use crate::StreamCounts;

// The frames that wait. Each holds an image of the decoder, which also keeps the frame shown last
// and writes the next picture, so the decoder's `depth` must exceed this by 2.
pub(crate) const QUEUE_FRAMES: usize = 3;

struct Waiting<T> {
    frame: T,
    target: Option<Duration>,
}

pub(crate) struct FrameQueue<T> {
    // In the order in which the frames came out of the decoder, oldest first
    waiting: VecDeque<Waiting<T>>,
    // The frame shown last, which the compositor repeats while no waiting frame is closer. Its
    // target is `None` for a frame with no stamp.
    shown: Option<Option<Duration>>,
    counts: StreamCounts,
}

impl<T> FrameQueue<T> {
    pub(crate) fn new() -> Self {
        Self {
            waiting: VecDeque::with_capacity(QUEUE_FRAMES),
            shown: None,
            counts: StreamCounts::default(),
        }
    }

    pub(crate) fn counts(&self) -> StreamCounts {
        self.counts
    }

    // Adds a frame that came out of the decoder. A full queue drops its oldest frame.
    pub(crate) fn push(&mut self, frame: T, target: Option<Duration>) {
        if self.waiting.len() == QUEUE_FRAMES && self.waiting.pop_front().is_some() {
            self.counts.skipped += 1;
        }
        self.waiting.push_back(Waiting { frame, target });
    }

    // Counts a frame that never reached the queue and that the caller dropped: its views were
    // refused, or no layer shows it.
    pub(crate) fn skipped(&mut self) {
        self.counts.skipped += 1;
    }

    // Drops every waiting frame, while the runtime shows none of the app's layers.
    pub(crate) fn clear(&mut self) {
        self.counts.skipped += self.waiting.drain(..).count() as u64;
    }

    // The frame to show at `display_time`, or `None` when the frame shown last stays. The frames
    // that came before the chosen one are dropped, so the picture never goes back.
    pub(crate) fn choose(&mut self, display_time: Duration) -> Option<T> {
        let Some(index) = self.closest(display_time) else {
            if self.shown.is_some() {
                self.counts.repeated += 1;
            }
            return None;
        };
        self.counts.skipped += self.waiting.drain(..index).count() as u64;
        let chosen = self.waiting.pop_front()?;
        self.shown = Some(chosen.target);
        self.counts.shown += 1;
        Some(chosen.frame)
    }

    // The index of the waiting frame to show, or `None` when the frame shown last is closer or
    // none waits.
    fn closest(&self, display_time: Duration) -> Option<usize> {
        let newest = self.waiting.len().checked_sub(1)?;
        if self.waiting.iter().any(|waiting| waiting.target.is_none()) {
            return Some(newest);
        }
        // A full queue means that the decoder is ahead of the display, so the frame shown last
        // no longer competes. Otherwise a host whose stamps run far ahead would freeze the
        // picture until the display time reached them.
        let mut best_gap = match self.shown {
            Some(Some(target)) if self.waiting.len() < QUEUE_FRAMES => {
                Some(target.abs_diff(display_time))
            }
            _ => None,
        };
        let mut best = None;
        for (index, waiting) in self.waiting.iter().enumerate() {
            let gap = waiting.target?.abs_diff(display_time);
            // A tie goes to the later frame
            if best_gap.is_none_or(|best| gap <= best) {
                best_gap = Some(gap);
                best = Some(index);
            }
        }
        best
    }
}

#[cfg(test)]
#[path = "tests/frame_queue_tests.rs"]
mod tests;
