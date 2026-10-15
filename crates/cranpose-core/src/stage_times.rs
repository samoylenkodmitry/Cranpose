//! Stage times for telemetry that is off unless an environment variable
//! turns it on.

use web_time::Instant;

/// When a pass reached each of its `N` stages, the first being its start. A
/// pass whose telemetry is off reads no clock: on some targets a clock read
/// is a system call followed by two 64-bit divisions.
#[doc(hidden)]
pub struct StageTimes<const N: usize> {
    times: Option<[Instant; N]>,
    reached: usize,
}

impl<const N: usize> StageTimes<N> {
    /// Starts timing a pass when `on`.
    pub fn start(on: bool) -> Self {
        Self {
            times: on.then(|| [Instant::now(); N]),
            reached: 1,
        }
    }

    /// Records that the pass reached its next stage.
    pub fn reached(&mut self) {
        if let Some(time) = self
            .times
            .as_mut()
            .and_then(|times| times.get_mut(self.reached))
        {
            *time = Instant::now();
            self.reached += 1;
        }
    }

    /// The stage times, while telemetry is on.
    pub fn times(&self) -> Option<[Instant; N]> {
        self.times
    }
}

/// The milliseconds from `from` to `to`.
#[doc(hidden)]
pub fn stage_ms(from: Instant, to: Instant) -> f64 {
    to.duration_since(from).as_secs_f64() * 1000.0
}
