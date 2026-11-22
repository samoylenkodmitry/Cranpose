//! The frame that a device presented or captured last, with the time of it. It is
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
        let latency = self.time_of(written_frames, sample_rate)? - i128::from(now_nanos);
        u64::try_from(latency)
            .ok()
            .filter(|&nanos| nanos > 0)
            .map(Duration::from_nanos)
    }

    // How long before `now_nanos` the hardware of an input stream captured the frame that the
    // stream's reader numbers `frame`, at `sample_rate` frames a second. A frame that the hardware
    // has not captured by then has no age, and `None` says so.
    pub(super) fn age(self, frame: i64, now_nanos: i64, sample_rate: i32) -> Option<Duration> {
        let age = i128::from(now_nanos) - self.time_of(frame, sample_rate)?;
        u64::try_from(age).ok().map(Duration::from_nanos)
    }

    // The time of `frame` on `CLOCK_MONOTONIC`: this position's time plus the frames between the
    // two. A stream without a rate has no time for a frame.
    pub(super) fn time_of(self, frame: i64, sample_rate: i32) -> Option<i128> {
        let rate = i128::from(sample_rate);
        if rate <= 0 {
            return None;
        }
        let frames_between = i128::from(frame) - i128::from(self.frame_position);
        Some(i128::from(self.nano_time) + frames_between * NANOS_PER_SECOND / rate)
    }
}

#[cfg(test)]
#[path = "tests/hardware_position_tests.rs"]
mod tests;
