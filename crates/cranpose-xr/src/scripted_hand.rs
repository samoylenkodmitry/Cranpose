//! A hand that a build plays in place of the cameras' hand.

#[cfg(all(feature = "scripted-hand", headset))]
use std::sync::OnceLock;
use std::time::Duration;

use crate::{
    Hand, HandAim, HandState, Pinch, Pose, TrackingState, XrFrame,
    pose::turn_of_axes,
    vector::{cross, normalize, sub},
};

// A pinch above this counts as made, as the runtime's own flag does.
const PINCHING: f32 = 0.8;

/// How the index finger of a [`ScriptedHand`] pinches over time, in a cycle that repeats. The
/// strength is 0 while the hand rests, rises to 1 while the fingers close, stays at 1 while they
/// are held and falls to 0 while they open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PinchCycle {
    /// How long the fingers stay open.
    pub rest: Duration,
    /// How long they take to close.
    pub closing: Duration,
    /// How long they stay together.
    pub held: Duration,
    /// How long they take to open.
    pub opening: Duration,
}

impl Default for PinchCycle {
    fn default() -> Self {
        Self {
            rest: Duration::from_secs(2),
            closing: Duration::from_millis(200),
            held: Duration::from_millis(600),
            opening: Duration::from_millis(200),
        }
    }
}

impl PinchCycle {
    // The pinch's strength at `time` on the clock of the frames.
    fn strength_at(&self, time: Duration) -> f32 {
        let total = self.rest + self.closing + self.held + self.opening;
        if total.is_zero() {
            return 0.0;
        }
        let whole = u32::try_from(time.as_nanos() / total.as_nanos()).unwrap_or(u32::MAX);
        let at = time.saturating_sub(total.saturating_mul(whole));
        let closed = self.rest + self.closing;
        let open_again = closed + self.held;
        if at < self.rest {
            0.0
        } else if at < closed {
            share(at - self.rest, self.closing)
        } else if at < open_again {
            1.0
        } else {
            1.0 - share(at - open_again, self.opening)
        }
    }
}

// How far `part` goes of `whole`, from 0 to 1.
fn share(part: Duration, whole: Duration) -> f32 {
    (part.as_secs_f32() / whole.as_secs_f32()).clamp(0.0, 1.0)
}

/// A hand that the backend plays in place of the cameras' hand, so that a build shows the hand
/// pointer where no hand can be tracked, such as in an emulator. Its aim pose points at a place
/// in the app's room, and its index finger pinches in a [`PinchCycle`]. The frame's [`HandState`] of
/// that side holds it as a tracked hand, and the pointer treats it as the cameras' hand: the
/// same path takes it from there. Install it with [`script_hand`] before the app starts.
///
/// It needs `Use::hand_tracking()` in the app's build, as a real hand does, and the crate's
/// `scripted-hand` feature. Jetpack XR has no such API.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScriptedHand {
    hand: Hand,
    aim: Pose,
    cycle: PinchCycle,
}

impl ScriptedHand {
    /// The hand `hand` with its aim pose at `from` in meters, pointing at the place `at`.
    pub fn pointing(hand: Hand, from: [f32; 3], at: [f32; 3], cycle: PinchCycle) -> Self {
        // The aim pose points along its -z. Its x axis stays level
        let z = normalize(sub(from, at));
        let x = normalize(cross([0.0, 1.0, 0.0], z));
        let y = cross(z, x);
        Self {
            hand,
            aim: Pose {
                position: from,
                orientation: turn_of_axes(x, y, z),
            },
            cycle,
        }
    }

    // The tracked hand at `time`.
    fn state_at(&self, time: Duration) -> HandState {
        let strength = self.cycle.strength_at(time);
        HandState {
            tracking_state: TrackingState::Tracking,
            aim: Some(HandAim {
                pose: self.aim,
                index: Pinch {
                    strength,
                    pinching: strength >= PINCHING,
                },
                ..HandAim::default()
            }),
            ..HandState::default()
        }
    }
}

// Writes the scripted hand into `frame`, as the hand trackers write the hands they locate.
pub(crate) fn play(script: &ScriptedHand, frame: &mut XrFrame) {
    let state = script.state_at(frame.predicted_display_time);
    match script.hand {
        Hand::Left => frame.left_hand = state,
        Hand::Right => frame.right_hand = state,
    }
}

#[cfg(all(feature = "scripted-hand", headset))]
static SCRIPT: OnceLock<ScriptedHand> = OnceLock::new();

/// Installs the script that the backend plays for the rest of the process. A second script
/// changes nothing.
#[cfg(all(feature = "scripted-hand", headset))]
pub fn script_hand(script: ScriptedHand) {
    if SCRIPT.set(script).is_err() {
        log::warn!("cranpose xr: a scripted hand is installed already");
    }
}

// Plays the installed script, if any, after the trackers wrote the frame's hands.
#[cfg(all(feature = "scripted-hand", headset))]
pub(crate) fn apply(frame: &mut XrFrame) {
    if let Some(script) = SCRIPT.get() {
        play(script, frame);
    }
}

#[cfg(test)]
#[path = "tests/scripted_hand_tests.rs"]
mod tests;
