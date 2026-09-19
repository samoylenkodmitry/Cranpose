use std::{
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
    time::Duration,
};

use web_time::Instant;

use super::{FramePacingMode, FrameSchedule, FrameScheduler, PlatformFrameDriver};

#[derive(Clone, Copy, Debug, PartialEq)]
enum DriverCall {
    RequestFrame,
    RequestWakeAt(Instant),
    ClearWake,
}

#[derive(Default)]
struct RecordingFrameDriver {
    calls: RefCell<Vec<DriverCall>>,
}

impl RecordingFrameDriver {
    fn calls(&self) -> Vec<DriverCall> {
        self.calls.borrow().clone()
    }
}

impl PlatformFrameDriver for RecordingFrameDriver {
    fn request_frame(&self) {
        self.calls.borrow_mut().push(DriverCall::RequestFrame);
    }

    fn request_wake_at(&self, deadline: Instant) {
        self.calls
            .borrow_mut()
            .push(DriverCall::RequestWakeAt(deadline));
    }

    fn clear_wake(&self) {
        self.calls.borrow_mut().push(DriverCall::ClearWake);
    }
}

#[test]
fn frame_pacing_labels_match_overlay_modes() {
    assert_eq!(FramePacingMode::Vsync.label(), "VSync");
    assert_eq!(FramePacingMode::Hard60.label(), "60fps");
    assert_eq!(FramePacingMode::Hard120.label(), "120fps");
    assert_eq!(FramePacingMode::NoVsync.label(), "NoVSync");
}

#[test]
fn only_hard_modes_have_fixed_targets() {
    assert_eq!(FramePacingMode::Vsync.target_fps(), None);
    assert_eq!(FramePacingMode::Hard60.target_fps(), Some(60));
    assert_eq!(FramePacingMode::Hard120.target_fps(), Some(120));
    assert_eq!(FramePacingMode::NoVsync.target_fps(), None);
}

#[test]
fn frame_schedule_requests_immediate_frame_and_clears_deadline() {
    let driver = RecordingFrameDriver::default();
    let deadline = Instant::now() + Duration::from_millis(25);

    FrameSchedule {
        needs_update: true,
        needs_frame: true,
        next_deadline: Some(deadline),
    }
    .apply_to(&driver);

    assert_eq!(
        driver.calls(),
        vec![DriverCall::ClearWake, DriverCall::RequestFrame]
    );
}

#[test]
fn frame_schedule_requests_deadline_when_idle_until_timer() {
    let driver = RecordingFrameDriver::default();
    let deadline = Instant::now() + Duration::from_millis(25);

    FrameSchedule {
        needs_update: false,
        needs_frame: false,
        next_deadline: Some(deadline),
    }
    .apply_to(&driver);

    assert_eq!(driver.calls(), vec![DriverCall::RequestWakeAt(deadline)]);
}

#[test]
fn frame_schedule_wakes_without_requesting_frame_for_update_only_work() {
    let driver = RecordingFrameDriver::default();
    let before = Instant::now();

    FrameSchedule {
        needs_update: true,
        needs_frame: false,
        next_deadline: None,
    }
    .apply_to(&driver);

    let calls = driver.calls();
    assert_eq!(calls.len(), 1);
    match calls[0] {
        DriverCall::RequestWakeAt(deadline) => {
            assert!(deadline >= before);
        }
        other => panic!("update-only work must wake without requesting a frame: {other:?}"),
    }
}

#[test]
fn frame_schedule_clears_wake_when_fully_idle() {
    let driver = RecordingFrameDriver::default();

    FrameSchedule {
        needs_update: false,
        needs_frame: false,
        next_deadline: None,
    }
    .apply_to(&driver);

    assert_eq!(driver.calls(), vec![DriverCall::ClearWake]);
}

#[test]
fn frame_scheduler_records_latest_schedule_and_applies_driver() {
    let scheduler = FrameScheduler::default();
    let driver = RecordingFrameDriver::default();
    let deadline = Instant::now() + Duration::from_millis(25);

    scheduler.schedule(
        FrameSchedule {
            needs_update: false,
            needs_frame: false,
            next_deadline: Some(deadline),
        },
        &driver,
    );

    assert_eq!(
        scheduler.snapshot(),
        FrameSchedule {
            needs_update: false,
            needs_frame: false,
            next_deadline: Some(deadline),
        }
    );
    assert_eq!(driver.calls(), vec![DriverCall::RequestWakeAt(deadline)]);
}

#[test]
fn frame_scheduler_clears_deadline_for_immediate_frame() {
    let scheduler = FrameScheduler::default();
    let driver = RecordingFrameDriver::default();
    let deadline = Instant::now() + Duration::from_millis(25);

    scheduler.schedule(
        FrameSchedule {
            needs_update: true,
            needs_frame: true,
            next_deadline: Some(deadline),
        },
        &driver,
    );

    assert_eq!(
        scheduler.snapshot(),
        FrameSchedule {
            needs_update: true,
            needs_frame: true,
            next_deadline: None,
        }
    );
    assert_eq!(
        driver.calls(),
        vec![DriverCall::ClearWake, DriverCall::RequestFrame]
    );
}

#[test]
fn frame_scheduler_recovers_poisoned_deadline_lock() {
    let scheduler = FrameScheduler::default();
    let deadline = Instant::now() + Duration::from_millis(25);

    let _ = catch_unwind(AssertUnwindSafe(|| {
        let _guard = scheduler.lock_deadline();
        panic!("poison frame scheduler deadline lock");
    }));

    scheduler.record(FrameSchedule {
        needs_update: false,
        needs_frame: false,
        next_deadline: Some(deadline),
    });

    assert_eq!(
        scheduler.snapshot(),
        FrameSchedule {
            needs_update: false,
            needs_frame: false,
            next_deadline: Some(deadline),
        }
    );
}
