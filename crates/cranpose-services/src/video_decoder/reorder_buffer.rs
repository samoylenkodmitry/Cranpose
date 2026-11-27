//! Presentation-timestamp reorder buffer for B-frame video streams.
//!
//! Hardware decoders such as Apple's VideoToolbox deliver pictures in decode order. Holding up
//! to `depth` pictures sorted by presentation timestamp emits pictures in ascending presentation
//! order once `depth` subsequent pictures have arrived, and drains the tail on end-of-stream.

use super::VideoFrame;

// Default reorder depth for Presentation mode when the bitstream omits a reorder count.
pub(super) const DEFAULT_REORDER_DEPTH: usize = 4;
// Upper bound matching the H.264/HEVC DPB maximum of 16 pictures.
pub(super) const MAX_REORDER_DEPTH: usize = 16;

pub(super) struct ReorderBuffer {
    depth: usize,
    frames: Vec<VideoFrame>,
}

impl ReorderBuffer {
    pub(super) fn new(depth: usize) -> Self {
        let depth = depth.min(MAX_REORDER_DEPTH);
        Self {
            depth,
            frames: Vec::with_capacity(depth + 1),
        }
    }

    pub(super) fn depth(&self) -> usize {
        self.depth
    }

    // Updates the reorder depth; callers pop any newly excess buffered pictures with `pop_excess`.
    pub(super) fn set_depth(&mut self, depth: usize) {
        self.depth = depth.min(MAX_REORDER_DEPTH);
    }

    pub(super) fn len(&self) -> usize {
        self.frames.len()
    }

    // Inserts `frame` in ascending presentation-timestamp order and returns the next ready picture
    // with zero heap allocation.
    pub(super) fn push(&mut self, frame: VideoFrame) -> Option<VideoFrame> {
        if self.depth == 0 {
            return Some(frame);
        }
        let pts = frame.info().timestamp;
        let index = self
            .frames
            .iter()
            .position(|existing| existing.info().timestamp > pts)
            .unwrap_or(self.frames.len());
        self.frames.insert(index, frame);
        self.pop_excess()
    }

    // Removes the oldest buffered picture if the buffer exceeds its configured depth.
    pub(super) fn pop_excess(&mut self) -> Option<VideoFrame> {
        if self.frames.len() > self.depth {
            Some(self.frames.remove(0))
        } else {
            None
        }
    }

    // Removes the oldest buffered picture in ascending presentation-timestamp order.
    pub(super) fn pop_front(&mut self) -> Option<VideoFrame> {
        if self.frames.is_empty() {
            None
        } else {
            Some(self.frames.remove(0))
        }
    }

    // Discards all buffered pictures (used on seek / flush).
    pub(super) fn clear(&mut self) {
        self.frames.clear();
    }
}
