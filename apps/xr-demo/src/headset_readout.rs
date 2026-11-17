// The headset's state as the demo's panel shows it: the head's height, the right trigger and the
// rate of the frame effect. The effect runs in each frame of the headset. It sets the panel's state
// only when a rounded value changes, so a still headset redraws nothing.

use std::{cell::RefCell, rc::Rc, time::Duration};

use cranpose::prelude::*;
use cranpose_xr::{Hand, Haptics, SpatialFrameEffect, Vibration, XrFrame, local_haptics};

// How long one measure of the frame rate takes.
const WINDOW: Duration = Duration::from_secs(1);
// The vibration of a counter press: the amplitude and the duration.
const PRESS_AMPLITUDE: f32 = 0.6;
const PRESS_DURATION: Duration = Duration::from_millis(40);
// A trigger pulled further than this is a press.
const PRESSED: f32 = 0.5;

// What the panel shows, rounded: the head's height above where it started in centimeters, the
// right trigger in percent and the frames that the effect saw in the last second.
#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) struct Readout {
    pub(crate) head_cm: Option<i32>,
    pub(crate) trigger_percent: u32,
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
                .map(|pose| (pose.position[1] * 100.0).round() as i32),
            trigger_percent: (frame.right_controller.trigger.value * 100.0).round() as u32,
            frames_per_second: self.frames_per_second,
        };
        if readout.get_non_reactive() != next {
            readout.set(next);
        }
    }

    // Counts the frame, and once a second takes the rate and logs it with the head's place.
    fn count(&mut self, frame: &XrFrame) {
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
        log::info!(
            "xr-demo: {} frame effects in {:?}, period {:?}, predicts {:?} ahead of now, head {:?}, interpupillary distance {:?}, left eye at {:?} from the head, field of view {:?}, profiles {:?} {:?}, recenter {:?}",
            self.frames_per_second,
            elapsed,
            frame.period,
            frame.predicted_display_time.saturating_sub(frame.now),
            frame
                .device_pose
                .map(|pose| pose.position.map(|v| format!("{v:.5}"))),
            frame.interpupillary_distance(),
            frame.left_eye.map(|eye| eye.local_pose.position),
            frame.left_eye.map(|eye| eye.field_of_view),
            frame.left_controller.profile,
            frame.right_controller.profile,
            frame.recenter,
        );
    }
}

// Runs the frame effect that fills `readout`, and plays a short vibration on the hand that pulled
// its trigger last when `presses` grows.
#[composable]
pub(crate) fn HeadsetReadout(presses: MutableState<u32>, readout: MutableState<Readout>) {
    let haptics = local_haptics().current();
    let tracker = remember(|| {
        Rc::new(RefCell::new(Tracker {
            window_start: Duration::ZERO,
            frames: 0,
            frames_per_second: 0,
            presses: 0,
            hand: Hand::Right,
        }))
    })
    .with(Clone::clone);
    SpatialFrameEffect(move |frame| {
        tracker
            .borrow_mut()
            .on_frame(frame, presses.get_non_reactive(), haptics.as_ref(), readout);
    });
}
