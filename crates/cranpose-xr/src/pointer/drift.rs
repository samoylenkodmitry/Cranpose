//! How far a press strays while the trigger holds it: the measure that sizes the slop of a drag.

use std::time::Duration;

use crate::{
    Hand as Side, Ray, aim_filter::angle_between, reach::Scene, resize::plane_dp, space::PanelSlot,
};

// Where a press began: the panel that it pressed, the ray's direction at the pull and where the
// ray met the panel's plane, in dp to the right and up from the panel's center.
#[derive(Clone, Copy, Debug)]
pub(super) struct Drift {
    panel: u64,
    start: Duration,
    direction: [f32; 3],
    place: Option<[f32; 2]>,
    // The most that the ray turned from `direction`, in degrees, and the most that its meeting
    // with the panel's plane moved from `place`, in dp
    degrees: f32,
    dp: f32,
}

impl Drift {
    pub(super) fn begin(slot: &PanelSlot, ray: &Ray, start: Duration) -> Self {
        Self {
            panel: slot.id,
            start,
            direction: ray.direction,
            place: plane_dp(ray, &slot.face()),
            degrees: 0.0,
            dp: 0.0,
        }
    }

    // Takes the ray of a frame of the press. The panel stands where the frame has it, so a panel
    // that rides the other hand measures the ray against the panel.
    pub(super) fn note(&mut self, ray: &Ray, scene: Scene<'_>) {
        self.degrees = self
            .degrees
            .max(angle_between(self.direction, ray.direction).to_degrees());
        let now = scene
            .panels
            .iter()
            .find(|slot| slot.id == self.panel)
            .and_then(|slot| plane_dp(ray, &slot.face()));
        if let (Some(place), Some(now)) = (self.place, now) {
            self.dp = self.dp.max((now[0] - place[0]).hypot(now[1] - place[1]));
        }
    }

    // Logs the press that ended at `end`, and how it ended, for the measure of the slop.
    pub(super) fn report(&self, side: Side, end: Duration, outcome: &str) {
        log::info!(
            "cranpose xr: the {side:?} press ended as {outcome} after {} ms: the ray turned {:.2} degrees and its point moved {:.1} dp at most",
            end.saturating_sub(self.start).as_millis(),
            self.degrees,
            self.dp,
        );
    }
}
