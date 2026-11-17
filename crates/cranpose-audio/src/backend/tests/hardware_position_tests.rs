use std::time::Duration;

use super::HardwarePosition;

const MS: i64 = 1_000_000;

// The hardware presented frame 1000 at 10 ms.
fn position() -> HardwarePosition {
    HardwarePosition {
        frame_position: 1000,
        nano_time: 10 * MS,
    }
}

#[test]
fn frames_ahead_of_the_hardware_present_after_their_distance_minus_the_time_since() {
    // 480 frames at 48 kHz are 10 ms after 10 ms, so at 20 ms. It is 12 ms now.
    let latency = position().latency(1480, 12 * MS, 48_000);
    assert_eq!(latency, Some(Duration::from_millis(8)));
}

#[test]
fn a_writer_behind_the_hardware_has_no_latency() {
    assert_eq!(position().latency(900, 10 * MS, 48_000), None);
    assert_eq!(position().latency(1000, 10 * MS, 48_000), None);
}

#[test]
fn a_stale_position_has_no_latency() {
    // The stream was paused: the last presented frame is two seconds old.
    assert_eq!(position().latency(1100, 2_000 * MS, 48_000), None);
}

#[test]
fn a_stream_without_a_rate_has_no_latency() {
    assert_eq!(position().latency(1480, 12 * MS, 0), None);
    assert_eq!(position().latency(1480, 12 * MS, -1), None);
}
