//! The frame that an output device presented last, with the time of its presentation. It is
//! `AAudioStream_getTimestamp`'s answer. Android's `AudioTimestamp` holds the same two numbers.

use std::time::Duration;

const NANOS_PER_SECOND: i128 = 1_000_000_000;

// A frame number of the stream, and the time at which the hardware presented that frame, on
// `CLOCK_MONOTONIC`. Frame numbers count from the first frame of the stream, as
// `AAudioStream_getFramesWritten` does.
#[derive(Clone, Copy)]
pub(super) struct HardwarePosition {
    pub(super) frame_position: i64,
    pub(super) nano_time: i64,
}

impl HardwarePosition {
    // How long from `now_nanos` until the hardware presents the frame that the stream's writer
    // numbers `written_frames`, on `CLOCK_MONOTONIC` at `sample_rate` frames a second. The
    // frame's time is the presented frame's time plus the frames between the two. A stream that
    // has stopped for a while gives a stale position, whose frame lies in the past by now. A
    // writer that is behind the hardware means the same. Neither has a latency, so both give
    // `None`.
    pub(super) fn latency(
        self,
        written_frames: i64,
        now_nanos: i64,
        sample_rate: i32,
    ) -> Option<Duration> {
        let rate = i128::from(sample_rate);
        if rate <= 0 {
            return None;
        }
        let frames_between = i128::from(written_frames) - i128::from(self.frame_position);
        let presented = i128::from(self.nano_time) + frames_between * NANOS_PER_SECOND / rate;
        let latency = presented - i128::from(now_nanos);
        u64::try_from(latency)
            .ok()
            .filter(|&nanos| nanos > 0)
            .map(Duration::from_nanos)
    }
}

#[cfg(test)]
#[path = "tests/hardware_position_tests.rs"]
mod tests;
