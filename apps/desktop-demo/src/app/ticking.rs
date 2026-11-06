// Work that runs once a frame while it is on, told how long each frame took.

use cranpose_core::LaunchedEffect;
use cranpose_ui::composable;

// The longest step a frame takes, in seconds: a frame that comes late, as
// after the window was hidden, moves things no farther than this.
pub(crate) const LONGEST_STEP_SECONDS: f32 = 0.1;

// Runs `tick` with the seconds each frame took while `on`, a step at most
// `LONGEST_STEP_SECONDS` long.
#[composable(no_skip)]
pub(crate) fn Ticking(on: bool, tick: impl Fn(f32) + 'static) {
    LaunchedEffect(on, async move |scope| {
        if !on {
            return;
        }
        let mut last = scope.with_frame_nanos(|now| now).await;
        loop {
            let now = scope.with_frame_nanos(|now| now).await;
            tick(seconds_between(last, now).min(LONGEST_STEP_SECONDS));
            last = now;
        }
    });
}

pub(crate) fn seconds_between(earlier_nanos: u64, later_nanos: u64) -> f32 {
    later_nanos.saturating_sub(earlier_nanos) as f32 * 1.0e-9
}
