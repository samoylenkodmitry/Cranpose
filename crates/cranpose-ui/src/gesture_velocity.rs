//! The velocity of a finger along one axis. A sample takes the platform's
//! event time where the platform gives one. Desktop, iOS and watchOS give
//! none, so there a sample takes the instant at which the shell received it.

mod arrival_clock;

use arrival_clock::ArrivalClock;
use cranpose_foundation::{PointerEvent, VelocityTracker1D, velocity_tracker::ASSUME_STOPPED_MS};
use web_time::Instant;

// The instant at which the shell received `event`. An event built without the
// shell has none: it counts as received by the call that handles it.
fn received_at(event: &PointerEvent) -> Instant {
    event.received_at.unwrap_or_else(Instant::now)
}

pub(crate) struct GestureVelocity {
    tracker: VelocityTracker1D,
    // When the gesture began, by the clock of the shell's receipt and by the
    // platform's event clock.
    start: Option<Instant>,
    start_event_ms: Option<i64>,
    // The time of the last sample, in milliseconds since the gesture began.
    last_sample_ms: Option<i64>,
    arrival_clock: ArrivalClock,
}

impl GestureVelocity {
    pub(crate) fn new() -> Self {
        Self {
            tracker: VelocityTracker1D::new(),
            start: None,
            start_event_ms: None,
            last_sample_ms: None,
            arrival_clock: ArrivalClock::default(),
        }
    }

    // Starts a gesture at `position`, with the down `event`.
    pub(crate) fn begin(&mut self, event: &PointerEvent, position: f32) {
        self.tracker.reset();
        self.start = Some(received_at(event));
        self.arrival_clock = ArrivalClock::default();
        self.start_event_ms = event.time_ms;
        self.tracker.add_data_point(0, position);
        self.last_sample_ms = Some(0);
    }

    // Adds the sample of the move `event` to `position`.
    pub(crate) fn add(&mut self, event: &PointerEvent, position: f32) {
        let event_sample_ms = self
            .start_event_ms
            .zip(event.time_ms)
            .map(|(start_ms, now_ms)| now_ms - start_ms);
        let sample_ms = if let Some(event_sample_ms) = event_sample_ms {
            Some(match self.last_sample_ms {
                Some(last_sample_ms) => event_sample_ms.max(last_sample_ms),
                None => event_sample_ms.max(0),
            })
        } else {
            self.start.map(|start| {
                self.arrival_clock
                    .sample_ms(received_at(event), start, self.last_sample_ms)
            })
        };
        if let Some(sample_ms) = sample_ms {
            log::trace!(
                target: "cranpose::velocity",
                "sample t={sample_ms}ms pos={position:.2} event_time={:?}",
                event.time_ms
            );
            self.tracker.add_data_point(sample_ms, position);
            self.last_sample_ms = Some(sample_ms);
        }
    }

    // The velocity the gesture ends with at the release `event`, at most
    // `max_velocity`. It is zero when the finger rested before it lifted.
    pub(crate) fn release(&mut self, event: &PointerEvent, max_velocity: f32) -> f32 {
        let Some(start) = self.start else {
            return 0.0;
        };
        let release_sample_ms = event.time_ms.zip(self.start_event_ms).map_or_else(
            || {
                received_at(event)
                    .saturating_duration_since(start)
                    .as_millis() as i64
            },
            |(release_ms, start_ms)| release_ms - start_ms,
        );
        let rested_before_release = self
            .last_sample_ms
            .is_some_and(|last_sample_ms| release_sample_ms - last_sample_ms > ASSUME_STOPPED_MS);
        if rested_before_release {
            return 0.0;
        }
        self.tracker.calculate_velocity_with_max(max_velocity)
    }

    // Forgets the gesture.
    pub(crate) fn clear(&mut self) {
        self.tracker.reset();
        self.start = None;
        self.start_event_ms = None;
        self.last_sample_ms = None;
    }
}
