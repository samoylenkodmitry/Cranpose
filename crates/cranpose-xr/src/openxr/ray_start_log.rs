//! A test build's log of where each controller's ray starts, against the tip of its drawn model.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use crate::{Hand, TrackedPose, aim::Aim, controller_states::ControllerStates, vector::sub};

// How often the log writes a line.
const EVERY: Duration = Duration::from_secs(2);

// The time of the last line, in nanoseconds on the frames' clock. A test build logs from the one
// frame loop.
static LAST: AtomicU64 = AtomicU64::new(0);

// Logs, at most every EVERY, how far each hand's ray starts from the model's `laser_begin` as the
// scene draws it, and how far the runtime's aim pose lies from that point.
pub(crate) fn log(
    now: Duration,
    aims: [Option<Aim>; 2],
    states: ControllerStates,
    runtime_aims: [Option<TrackedPose>; 2],
) {
    let nanos = u64::try_from(now.as_nanos()).unwrap_or(u64::MAX);
    let last = LAST.load(Ordering::Relaxed);
    if last != 0 && nanos.saturating_sub(last) < u64::try_from(EVERY.as_nanos()).unwrap_or(0) {
        return;
    }
    LAST.store(nanos.max(1), Ordering::Relaxed);
    for hand in Hand::BOTH {
        let (Some(aim), Some(drawn), Some(runtime)) = (
            aims[hand.index()],
            states.drawn_ray_start(hand),
            runtime_aims[hand.index()],
        ) else {
            continue;
        };
        log::info!(
            "cranpose xr: the {hand:?} ray starts {:.1} mm from the drawn laser_begin; the runtime's aim pose lies {:.1} mm from it",
            millimeters(aim.ray.origin, drawn),
            millimeters(runtime.pose.position, drawn),
        );
    }
}

fn millimeters(a: [f32; 3], b: [f32; 3]) -> f32 {
    let gap = sub(a, b);
    (gap[0] * gap[0] + gap[1] * gap[1] + gap[2] * gap[2]).sqrt() * 1000.0
}
