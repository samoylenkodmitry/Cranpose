// The headset's state as the demo's panel shows it: the head's height, the right trigger and the
// rate of the frame effect. The effect runs in each frame of the headset. It sets the panel's state
// only when a rounded value changes, so a still headset redraws nothing.

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use cranpose::prelude::*;
use cranpose_xr::{
    Hand, Haptics, SpatialFrameEffect, StreamCounts, StreamStats, Vibration, XrFrame, local_haptics,
};

// How long one measure of the frame rate takes.
const WINDOW: Duration = Duration::from_secs(1);
// The vibration of a counter press: the amplitude and the duration.
const PRESS_AMPLITUDE: f32 = 0.6;
const PRESS_DURATION: Duration = Duration::from_millis(40);
// A trigger pulled further than this is a press.
const PRESSED: f32 = 0.5;

// The headset's monotonic clock as the frame effect last read it, which a stream's player reads
// when it starts, to stamp its frames with display times on that clock.
#[derive(Clone, Default)]
pub(crate) struct HeadsetClock(Arc<AtomicU64>);

impl PartialEq for HeadsetClock {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl HeadsetClock {
    fn set(&self, now: Duration) {
        self.0.store(
            u64::try_from(now.as_nanos()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
    }

    pub(crate) fn read(&self) -> Duration {
        Duration::from_nanos(self.0.load(Ordering::Relaxed))
    }
}

// The counts of the demo's two streamed scenes
#[derive(Clone, Default, PartialEq)]
pub(crate) struct StreamLogs {
    pub(crate) clip: StreamStats,
    pub(crate) foveated: StreamStats,
}

// What the panel shows, rounded: the head's height above where it started in centimeters, the
// right trigger in percent and the frames that the effect saw in the last second.
#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) struct Readout {
    // Whole centimeters and whole percent, rounded so a change below them does not redraw
    pub(crate) head_cm: Option<f32>,
    pub(crate) trigger_percent: f32,
    pub(crate) frames_per_second: u32,
}

// What the effect keeps between frames.
struct Tracker {
    window_start: Duration,
    frames: u32,
    frames_per_second: u32,
    presses: u32,
    // The hand whose trigger was pulled last, which feels the vibration of a press
    hand: Hand,
    clock: HeadsetClock,
    logs: StreamLogs,
    // The streams' counts at the last log, to log what each second added
    logged: [StreamCounts; 2],
}

impl Tracker {
    fn on_frame(
        &mut self,
        frame: &XrFrame,
        presses: u32,
        haptics: Option<&Haptics>,
        readout: MutableState<Readout>,
    ) {
        self.count(frame);
        for hand in [Hand::Left, Hand::Right] {
            if frame.controller(hand).trigger.value > PRESSED {
                self.hand = hand;
            }
        }
        if presses > self.presses
            && let Some(haptics) = haptics
        {
            haptics.vibrate(self.hand, Vibration::new(PRESS_AMPLITUDE, PRESS_DURATION));
        }
        self.presses = presses;
        let next = Readout {
            head_cm: frame
                .device_pose
                .map(|pose| (pose.position[1] * 100.0).round()),
            trigger_percent: (frame.right_controller.trigger.value * 100.0).round(),
            frames_per_second: self.frames_per_second,
        };
        if readout.get_non_reactive() != next {
            readout.set(next);
        }
    }

    // Counts the frame, and once a second takes the rate and logs it with the head's place.
    fn count(&mut self, frame: &XrFrame) {
        self.clock.set(frame.now);
        self.frames += 1;
        let elapsed = frame
            .predicted_display_time
            .saturating_sub(self.window_start);
        if elapsed < WINDOW {
            return;
        }
        self.frames_per_second = self.frames;
        self.frames = 0;
        self.window_start = frame.predicted_display_time;
        self.log_streams();
        log::info!(
            "xr-demo: {} frame effects in {:?}, period {:?}, predicts {:?} ahead of now, head {:?}, head velocity {:?} {:?}, floor {:?}, stage {:?}, interpupillary distance {:?}, left eye at {:?} from the head, field of view {:?}, profiles {:?} {:?}, recenter {:?}",
            self.frames_per_second,
            elapsed,
            frame.period,
            frame.predicted_display_time.saturating_sub(frame.now),
            frame
                .device_pose
                .map(|pose| pose.position.map(|v| format!("{v:.5}"))),
            frame
                .device_linear_velocity
                .map(|v| v.map(|v| format!("{v:.3}"))),
            frame
                .device_angular_velocity
                .map(|v| v.map(|v| format!("{v:.3}"))),
            frame.floor_height(),
            frame
                .stage_pose
                .map(|pose| pose.orientation.map(|v| format!("{v:.3}"))),
            frame.interpupillary_distance(),
            frame.left_eye.map(|eye| eye.local_pose.position),
            frame.left_eye.map(|eye| eye.field_of_view),
            frame.left_controller.profile,
            frame.right_controller.profile,
            frame.recenter,
        );
    }

    // Logs what each streamed scene did with its frames in the last second.
    fn log_streams(&mut self) {
        let now = [self.logs.clip.counts(), self.logs.foveated.counts()];
        for ((name, now), before) in ["clip", "foveated"].into_iter().zip(now).zip(self.logged) {
            log::info!(
                "xr-demo: stream {name}: {} shown, {} skipped, {} repeated in the last second ({} shown in all)",
                now.shown - before.shown,
                now.skipped - before.skipped,
                now.repeated - before.repeated,
                now.shown
            );
        }
        self.logged = now;
    }
}

// Runs the frame effect that fills `readout`, and plays a short vibration on the hand that pulled
// its trigger last when `presses` grows.
#[composable]
pub(crate) fn HeadsetReadout(
    presses: MutableState<u32>,
    readout: MutableState<Readout>,
    clock: HeadsetClock,
    logs: StreamLogs,
) {
    let haptics = local_haptics().current();
    let tracker = remember(|| {
        Rc::new(RefCell::new(Tracker {
            window_start: Duration::ZERO,
            frames: 0,
            frames_per_second: 0,
            presses: 0,
            hand: Hand::Right,
            clock,
            logs,
            logged: [StreamCounts::default(); 2],
        }))
    })
    .with(Clone::clone);
    SpatialFrameEffect(move |frame| {
        tracker
            .borrow_mut()
            .on_frame(frame, presses.get_non_reactive(), haptics.as_ref(), readout);
    });
}
