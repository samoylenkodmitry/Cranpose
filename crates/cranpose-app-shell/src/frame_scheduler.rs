use std::cell::Cell;

use crate::{FrameSchedule, PlatformFrameDriver};

/// Stores the latest [`FrameSchedule`] of a surface for the platform code that drives it. A
/// surface lives on its shell's thread, so the schedule sits in a `Cell`: on ESP-IDF a `Mutex`
/// is a FreeRTOS mutex, which every frame took.
#[derive(Debug, Default)]
pub struct FrameScheduler {
    latest: Cell<FrameSchedule>,
    // Whether a schedule recorded since the surface's last frame asked for no
    // frame and no update: the app held still between the two frames.
    held_still: Cell<bool>,
}

impl FrameScheduler {
    /// Stores `schedule` as the latest one. A schedule that needs an update drops its deadline.
    pub fn record(&self, schedule: FrameSchedule) {
        if !schedule.needs_frame && !schedule.needs_update {
            self.held_still.set(true);
        }
        self.latest.set(FrameSchedule {
            next_deadline: if schedule.needs_update {
                None
            } else {
                schedule.next_deadline
            },
            ..schedule
        });
    }

    /// Stores `schedule` and asks `driver` for the frame or wake it calls for.
    pub fn schedule<D>(&self, schedule: FrameSchedule, driver: &D)
    where
        D: PlatformFrameDriver + ?Sized,
    {
        self.record(schedule);
        schedule.apply_to(driver);
    }

    /// Returns the latest stored schedule.
    pub fn snapshot(&self) -> FrameSchedule {
        self.latest.get()
    }

    // Whether the app held still since the last call, which each frame of
    // the surface makes.
    pub(crate) fn take_held_still(&self) -> bool {
        self.held_still.take()
    }
}
