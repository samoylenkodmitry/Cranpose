//! A controller that a build plays in place of the runtime's controller.

use std::{sync::OnceLock, time::Duration};

use crate::{
    Button, Controller, Hand, InteractionProfile, Pose, Squeeze, Thumbstick, TrackedPose, Trigger,
    XrFrame, carry::handle_center_at, press_threshold::PRESS, scripted_hand::aim_pose,
};

/// What the buttons of a [`ScriptedController`] do during one step. Every field rests at its
/// default: no button down, the trigger and the grip released and the thumbstick at its middle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScriptedButtons {
    /// How far the trigger is pulled, from 0 to 1.
    pub trigger: f32,
    /// How far the grip is squeezed, from 0 to 1.
    pub squeeze: f32,
    /// Whether the first face button is down: A on the right hand, X on the left.
    pub primary: bool,
    /// Whether the second face button is down: B on the right hand, Y on the left.
    pub secondary: bool,
    /// How far the thumbstick leans to the right and forward, each from -1 to 1.
    pub thumbstick: [f32; 2],
    /// Whether the thumbstick is pressed in.
    pub thumbstick_click: bool,
    /// Whether the menu button is down.
    pub menu: bool,
}

// One step of the script: where the controller and its aim end, and what the buttons do. A step
// that is not tracked gives the frame a controller that the runtime does not track.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Step {
    duration: Duration,
    position: [f32; 3],
    at: [f32; 3],
    buttons: ScriptedButtons,
    tracked: bool,
}

/// A controller that the backend plays in place of the runtime's controller, so that a build shows
/// the controllers' pointers and buttons where no controller exists, such as in an emulator. Each
/// step moves the controller and its aim in a straight line and holds its buttons as given. The
/// script is written into the frame's [`Controller`] of its hand, as the runtime's actions would
/// write it, with the profile of the Meta Touch Plus controllers, so the pointer treats it as a
/// real controller. A script for each hand plays at once. Install it with [`script_controller`]
/// before the app starts.
///
/// A script begins at the first frame that plays it. It holds the end of its last step, or starts
/// again with the first step when it [repeats](Self::repeating). It needs the crate's
/// `scripted-hand` feature. Jetpack XR has no such API.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptedController {
    hand: Hand,
    start: ([f32; 3], [f32; 3]),
    steps: Vec<Step>,
    repeats: bool,
    // The time of the first frame that played the script, on the clock of the frames
    started: OnceLock<Duration>,
}

impl ScriptedController {
    /// A script for the controller of `hand` that stands at `position` in meters and points at the
    /// place `at`, with its buttons at rest.
    pub fn new(hand: Hand, position: [f32; 3], at: [f32; 3]) -> Self {
        Self {
            hand,
            start: (position, at),
            steps: Vec::new(),
            repeats: false,
            started: OnceLock::new(),
        }
    }

    /// The script with a step that lasts `duration`, in which the controller moves in a straight
    /// line to `position` while its aim moves in a straight line to the place `at`. The buttons are
    /// as `buttons` says during the whole step.
    #[must_use]
    pub fn moving_to(
        mut self,
        duration: Duration,
        (position, at): ([f32; 3], [f32; 3]),
        buttons: ScriptedButtons,
    ) -> Self {
        self.steps.push(Step {
            duration,
            position,
            at,
            buttons,
            tracked: true,
        });
        self
    }

    /// The script with a step that lasts `duration` where the controller stays where it is, with
    /// `buttons` as they are.
    #[must_use]
    pub fn holding(self, duration: Duration, buttons: ScriptedButtons) -> Self {
        let (position, at) = self.end();
        self.moving_to(duration, (position, at), buttons)
    }

    /// The script with a step that lasts `duration` in which the runtime does not track the
    /// controller, as when its battery is flat or it lies out of the cameras' view: the frame's
    /// [`Controller`] of the hand has no profile, no pose and no button down. A step after it
    /// starts where the controller was before it. A script that ends with such a step keeps the
    /// controller untracked, so a script of one such step plays a controller that is missing.
    #[must_use]
    pub fn untracked(mut self, duration: Duration) -> Self {
        let (position, at) = self.end();
        self.steps.push(Step {
            duration,
            position,
            at,
            buttons: ScriptedButtons::default(),
            tracked: false,
        });
        self
    }

    /// The script that starts again with its first step after its last.
    #[must_use]
    pub fn repeating(mut self) -> Self {
        self.repeats = true;
        self
    }

    /// Where the handle under a panel of `size` dp at `panel`, in meters, stands: the place that a
    /// controller points at to take the panel by that handle and carry it. The panel must be a
    /// movable [`SpatialPanel`](crate::SpatialPanel).
    pub fn handle_of(panel: &Pose, size: [f32; 2]) -> [f32; 3] {
        handle_center_at(panel, size[1])
    }

    // Where the script's controller and its aim are after its last step so far.
    fn end(&self) -> ([f32; 3], [f32; 3]) {
        self.steps
            .last()
            .map_or(self.start, |step| (step.position, step.at))
    }

    fn length(&self) -> Duration {
        self.steps.iter().map(|step| step.duration).sum()
    }

    // The controller `time` after the start of the script.
    fn state_at(&self, time: Duration) -> Controller {
        let total = self.length();
        let time = if self.repeats && !total.is_zero() {
            Duration::from_nanos(
                u64::try_from(time.as_nanos() % total.as_nanos()).unwrap_or(u64::MAX),
            )
        } else {
            time
        };
        let (mut position, mut at) = self.start;
        let mut buttons = ScriptedButtons::default();
        let mut tracked = true;
        let mut passed = Duration::ZERO;
        for step in &self.steps {
            let into = time.saturating_sub(passed);
            tracked = step.tracked;
            if into >= step.duration {
                (position, at, buttons) = (step.position, step.at, step.buttons);
                passed += step.duration;
                continue;
            }
            let share = into.as_secs_f32() / step.duration.as_secs_f32();
            position = lerp(position, step.position, share);
            at = lerp(at, step.at, share);
            buttons = step.buttons;
            break;
        }
        if tracked {
            controller(position, at, &buttons)
        } else {
            Controller::default()
        }
    }
}

fn lerp(from: [f32; 3], to: [f32; 3], share: f32) -> [f32; 3] {
    [0, 1, 2].map(|axis| from[axis] + (to[axis] - from[axis]) * share)
}

// The controller that stands at `position` and points at `at`, with `buttons`.
fn controller(position: [f32; 3], at: [f32; 3], buttons: &ScriptedButtons) -> Controller {
    let pose = aim_pose(position, at);
    let tracked = Some(TrackedPose {
        pose,
        linear_velocity: None,
        angular_velocity: None,
    });
    let down = |pressed| Button {
        pressed,
        touched: pressed,
    };
    Controller {
        profile: Some(InteractionProfile::MetaTouchPlus),
        aim: tracked,
        grip: tracked,
        trigger: Trigger {
            value: buttons.trigger,
            pressed: buttons.trigger >= PRESS,
            touched: buttons.trigger > 0.0,
        },
        squeeze: Squeeze {
            value: buttons.squeeze,
            pressed: buttons.squeeze >= PRESS,
        },
        thumbstick: Thumbstick {
            x: buttons.thumbstick[0],
            y: buttons.thumbstick[1],
            pressed: buttons.thumbstick_click,
            touched: buttons
                .thumbstick
                .iter()
                .any(|lean| lean.abs() > f32::EPSILON)
                || buttons.thumbstick_click,
        },
        primary: down(buttons.primary),
        secondary: down(buttons.secondary),
        menu: buttons.menu,
        thumbrest_touched: false,
    }
}

// Writes the scripted controller into `frame`, as the controllers' actions write the controller of
// a hand.
pub(crate) fn play(script: &ScriptedController, frame: &mut XrFrame) {
    let started = *script.started.get_or_init(|| frame.predicted_display_time);
    let state = script.state_at(frame.predicted_display_time.saturating_sub(started));
    match script.hand {
        Hand::Left => frame.left_controller = state,
        Hand::Right => frame.right_controller = state,
    }
}

#[cfg(all(feature = "scripted-hand", headset))]
static SCRIPTS: [OnceLock<ScriptedController>; 2] = [OnceLock::new(), OnceLock::new()];

/// Installs the script that the backend plays for the controller of its hand, for the rest of the
/// process. A script for each hand plays at once. A second script for a hand changes nothing.
#[cfg(all(feature = "scripted-hand", headset))]
pub fn script_controller(script: ScriptedController) {
    let hand = script.hand;
    if SCRIPTS[hand.index()].set(script).is_err() {
        log::warn!("cranpose xr: a scripted controller is installed for the {hand:?} hand already");
    }
}

// Plays the installed scripts, if any, after the controllers' actions wrote the frame.
#[cfg(all(feature = "scripted-hand", headset))]
pub(crate) fn apply(frame: &mut XrFrame) {
    for script in SCRIPTS.iter().filter_map(OnceLock::get) {
        play(script, frame);
    }
}

#[cfg(test)]
#[path = "tests/scripted_controller_tests.rs"]
mod tests;
