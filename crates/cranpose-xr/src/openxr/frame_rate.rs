//! The count of drawn frames, logged once a second.

use std::time::{Duration, Instant};

const INTERVAL: Duration = Duration::from_secs(1);

pub(crate) struct FrameRate {
    frames: u32,
    since: Instant,
}

impl FrameRate {
    pub(crate) fn new() -> Self {
        Self {
            frames: 0,
            since: Instant::now(),
        }
    }

    // Counts a frame, and logs the count of the last interval when it ends.
    pub(crate) fn tick(&mut self) {
        self.frames += 1;
        let elapsed = self.since.elapsed();
        if elapsed >= INTERVAL {
            log::info!(
                "cranpose xr: {} frames in {:.1} s",
                self.frames,
                elapsed.as_secs_f32()
            );
            self.frames = 0;
            self.since = Instant::now();
        }
    }
}
