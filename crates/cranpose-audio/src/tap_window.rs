//! The sums that the debug tap of the mixed output keeps between render calls.

use cranpose_services::OutputTap;

// The squares of the samples of the second that runs now.
#[derive(Default)]
pub(crate) struct TapWindow {
    left: f32,
    right: f32,
    frames: f32,
}

impl TapWindow {
    // Adds the frames of `out` and, once a second of output has gone by, publishes the root mean
    // square of each channel to `tap`. The right channel of a mono output is the left.
    pub(crate) fn record(&mut self, out: &[f32], channels: usize, rate: f32, tap: &OutputTap) {
        for frame in out.chunks_exact(channels) {
            let (left, right) = (frame[0], frame[channels.min(2) - 1]);
            self.left += left * left;
            self.right += right * right;
        }
        self.frames += (out.len() / channels) as f32;
        if self.frames >= rate {
            tap.publish(
                (self.left / self.frames).sqrt(),
                (self.right / self.frames).sqrt(),
            );
            *self = Self::default();
        }
    }
}
