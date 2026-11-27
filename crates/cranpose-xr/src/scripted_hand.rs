//! A hand that a build plays in place of the cameras' hand.

use std::{sync::OnceLock, time::Duration};

use crate::{
    Hand, HandAim, HandJoint, HandJointType, HandState, Pinch, Pose, TrackingState, XrFrame,
    key_layout::{self, KeyKind, PlacedKey},
    keyboard_state::KeyboardState,
    pose::turn_of_axes,
    space::field_dock::{below, point_on},
    vector::{cross, dot, lerp, normalize, sub},
};

// A pinch above this counts as made, as the runtime's own flag does.
const PINCHING: f32 = 0.8;
// The radius of a scripted palm joint, in meters: about an adult's palm
const PALM_RADIUS: f32 = 0.04;
// The radius of a scripted index fingertip, in meters: about an adult's finger at its tip
const FINGERTIP_RADIUS: f32 = 0.007;

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
    fn length(&self) -> Duration {
        self.rest + self.closing + self.held + self.opening
    }

    // How many whole cycles have passed at `time`, and how far into the current one it is. A cycle
    // of no length has passed none.
    fn position(&self, time: Duration) -> (u32, Duration) {
        let total = self.length();
        if total.is_zero() {
            return (0, Duration::ZERO);
        }
        let whole = u32::try_from(time.as_nanos() / total.as_nanos()).unwrap_or(u32::MAX);
        (whole, time.saturating_sub(total.saturating_mul(whole)))
    }

    // The pinch's strength at `time` on the clock of the frames.
    fn strength_at(&self, time: Duration) -> f32 {
        if self.length().is_zero() {
            return 0.0;
        }
        let (_, at) = self.position(time);
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
/// A script that has several places moves its aim to the next place at the start of each cycle,
/// while the fingers are open, and starts again after the last. The script begins at the first
/// frame that plays it.
///
/// A script made with [`touching`](Self::touching) moves the tip of the index finger in steps and
/// never pinches, so the hand presses the panels by direct touch alone.
///
/// It needs `Use::hand_tracking()` in the app's build, as a real hand does, and the crate's
/// `scripted-hand` feature. Jetpack XR has no such API.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptedHand {
    hand: Hand,
    // The aim pose of each cycle, in turn
    aims: Vec<Pose>,
    cycle: PinchCycle,
    // The palm's pose, when the script holds the hand open with its palm there
    palm: Option<Pose>,
    // The path of the index fingertip, when the script moves it
    fingertip: Option<FingertipPath>,
    // The time of the first frame that played the script, on the clock of the frames
    started: OnceLock<Duration>,
}

impl ScriptedHand {
    /// The script with the hand's palm joint at `palm`, valid and tracked, as an open hand holds it.
    /// Content that follows the palm ([`FollowTarget::Hand`](crate::FollowTarget::Hand)) then
    /// stands there. The pose has the OpenXR axes of a hand joint: -z toward the finger tips and +y
    /// out of the back of the hand. The other joints stay at rest.
    #[must_use]
    pub fn with_palm(mut self, palm: Pose) -> Self {
        self.palm = Some(palm);
        self
    }

    /// The hand `hand` with its aim pose at `from` in meters, pointing at the place `at`, and the
    /// tip of its index finger at `fingertip`, valid and tracked. Its fingers never pinch: the hand
    /// presses where its fingertip goes into a panel's face, by the direct touch of
    /// [`PointerSettings::direct_touch`](crate::PointerSettings::direct_touch). The steps of
    /// [`moving_fingertip_to`](Self::moving_fingertip_to) and
    /// [`holding_fingertip`](Self::holding_fingertip) move the fingertip, and it holds the end of
    /// the last step. The fingertip's joint turns as the aim pose does.
    pub fn touching(hand: Hand, from: [f32; 3], at: [f32; 3], fingertip: [f32; 3]) -> Self {
        let open = PinchCycle {
            rest: Duration::ZERO,
            closing: Duration::ZERO,
            held: Duration::ZERO,
            opening: Duration::ZERO,
        };
        Self {
            fingertip: Some(FingertipPath::at(fingertip)),
            ..Self::pointing(hand, from, at, open)
        }
    }

    /// The script with a step that lasts `duration`, in which the tip of the index finger moves in
    /// a straight line to `fingertip`, in meters. A script that had no fingertip gets one that
    /// starts at `fingertip`.
    #[must_use]
    pub fn moving_fingertip_to(mut self, duration: Duration, fingertip: [f32; 3]) -> Self {
        self.fingertip
            .get_or_insert_with(|| FingertipPath::at(fingertip))
            .steps
            .push(FingertipStep {
                duration,
                to: fingertip,
            });
        self
    }

    /// The script with a step that lasts `duration` in which the tip of the index finger stays
    /// where it is. A script with no fingertip stays as it is.
    #[must_use]
    pub fn holding_fingertip(self, duration: Duration) -> Self {
        let Some(end) = self.fingertip.as_ref().map(FingertipPath::end) else {
            return self;
        };
        self.moving_fingertip_to(duration, end)
    }

    /// The hand `hand` with its aim pose at `from` in meters, pointing at the place `at`.
    pub fn pointing(hand: Hand, from: [f32; 3], at: [f32; 3], cycle: PinchCycle) -> Self {
        Self::visiting(hand, from, [at], cycle)
    }

    /// The hand `hand` with its aim pose at `from` in meters, which presses a text field and then
    /// the keys that type `text`, one press in each cycle.
    ///
    /// The field lies `field` dp from the top left corner of the panel of `panel_size` dp at
    /// `panel`, in meters. The keyboard that a headset draws opens below that panel, and the hand
    /// presses its shift key, its symbols page and its letters where the keyboard has them. A
    /// character that the keyboard lacks is left out. The press on the field is the first one, so
    /// the keyboard is open when the hand reaches its first key.
    pub fn typing(
        hand: Hand,
        from: [f32; 3],
        panel: Pose,
        panel_size: [f32; 2],
        field: [f32; 2],
        text: &str,
        cycle: PinchCycle,
    ) -> Self {
        let keyboard = below(&panel, panel_size, key_layout::SIZE);
        let keys = key_taps(text)
            .into_iter()
            .map(|key| point_on(&keyboard, key_layout::SIZE, key.center()));
        let at = std::iter::once(point_on(&panel, panel_size, field)).chain(keys);
        Self::visiting(hand, from, at, cycle)
    }

    // The hand with its aim pose at `from`, pointing at each of `places` in turn.
    fn visiting(
        hand: Hand,
        from: [f32; 3],
        places: impl IntoIterator<Item = [f32; 3]>,
        cycle: PinchCycle,
    ) -> Self {
        Self {
            hand,
            aims: places.into_iter().map(|at| aim_pose(from, at)).collect(),
            cycle,
            palm: None,
            fingertip: None,
            started: OnceLock::new(),
        }
    }

    // The tracked hand at `time` after the script's start.
    fn state_at(&self, time: Duration) -> HandState {
        let strength = self.cycle.strength_at(time);
        let (cycles, _) = self.cycle.position(time);
        let index = usize::try_from(cycles).map_or(0, |cycles| cycles % self.aims.len());
        let aim = self.aims[index];
        let mut joints = [HandJoint::default(); HandJointType::COUNT];
        if let Some(pose) = self.palm {
            joints[HandJointType::Palm.index()] = HandJoint {
                pose,
                radius: PALM_RADIUS,
                valid: true,
                tracked: true,
            };
        }
        if let Some(path) = &self.fingertip {
            joints[HandJointType::IndexTip.index()] = HandJoint {
                pose: Pose {
                    position: path.at_time(time),
                    orientation: aim.orientation,
                },
                radius: FINGERTIP_RADIUS,
                valid: true,
                tracked: true,
            };
        }
        HandState {
            tracking_state: TrackingState::Tracking,
            joints,
            aim: Some(HandAim {
                pose: aim,
                index: Pinch {
                    strength,
                    pinching: strength >= PINCHING,
                },
                ..HandAim::default()
            }),
        }
    }
}

// Where a scripted fingertip starts and the steps that move it, in meters.
#[derive(Clone, Debug, PartialEq)]
struct FingertipPath {
    start: [f32; 3],
    steps: Vec<FingertipStep>,
}

// One step of a scripted fingertip: how long it lasts and where the fingertip ends.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FingertipStep {
    duration: Duration,
    to: [f32; 3],
}

impl FingertipPath {
    fn at(start: [f32; 3]) -> Self {
        Self {
            start,
            steps: Vec::new(),
        }
    }

    // Where the fingertip stands after the last step.
    fn end(&self) -> [f32; 3] {
        self.steps.last().map_or(self.start, |step| step.to)
    }

    // Where the fingertip stands at `time` after the script's start. After the last step it holds
    // that step's end.
    fn at_time(&self, time: Duration) -> [f32; 3] {
        let mut from = self.start;
        let mut passed = Duration::ZERO;
        for step in &self.steps {
            let into = time.saturating_sub(passed);
            if into < step.duration {
                return lerp(
                    from,
                    step.to,
                    into.as_secs_f32() / step.duration.as_secs_f32(),
                );
            }
            from = step.to;
            passed += step.duration;
        }
        from
    }
}

// The aim pose at `from` that points at `at`. It points along its -z, and its x axis stays level.
pub(crate) fn aim_pose(from: [f32; 3], at: [f32; 3]) -> Pose {
    let z = normalize(sub(from, at));
    let across = cross([0.0, 1.0, 0.0], z);
    // A ray straight up or down has no side to the vertical: the room's x axis stands in for it
    let x = if dot(across, across) > 1e-12 {
        normalize(across)
    } else {
        [1.0, 0.0, 0.0]
    };
    let y = cross(z, x);
    Pose {
        position: from,
        orientation: turn_of_axes(x, y, z),
    }
}

// The keys that a person presses to type `text` on the keyboard, in order. The keyboard's own
// state says which page shows and whether shift is on, so a key that is not on the shown page
// comes after the key that shows its page.
fn key_taps(text: &str) -> Vec<PlacedKey> {
    let mut state = KeyboardState::default();
    let mut taps = Vec::new();
    let mut tap = |state: &mut KeyboardState, kind: KeyKind| {
        if let Some(key) = key_layout::keys(state.page).find(|key| key.kind == kind) {
            state.press(kind);
            taps.push(key);
        }
    };
    for ch in text.chars() {
        let Some((page, key)) = key_layout::find(ch) else {
            continue;
        };
        if state.page != page {
            tap(&mut state, KeyKind::Page(page));
        }
        if ch.is_ascii_uppercase() && !state.capitals() {
            tap(&mut state, KeyKind::Shift);
        }
        tap(&mut state, key.kind);
    }
    taps
}

// Writes the scripted hand into `frame`, as the hand trackers write the hands they locate.
pub(crate) fn play(script: &ScriptedHand, frame: &mut XrFrame) {
    let started = *script.started.get_or_init(|| frame.predicted_display_time);
    let state = script.state_at(frame.predicted_display_time.saturating_sub(started));
    match script.hand {
        Hand::Left => frame.left_hand = state,
        Hand::Right => frame.right_hand = state,
    }
}

#[cfg(all(feature = "scripted-hand", headset))]
static SCRIPTS: [OnceLock<ScriptedHand>; 2] = [OnceLock::new(), OnceLock::new()];

/// Installs the script that the backend plays for its hand, for the rest of the process. A script
/// for each hand plays at once. A second script for a hand changes nothing.
#[cfg(all(feature = "scripted-hand", headset))]
pub fn script_hand(script: ScriptedHand) {
    let hand = script.hand;
    if SCRIPTS[hand.index()].set(script).is_err() {
        log::warn!("cranpose xr: a scripted hand is installed for the {hand:?} hand already");
    }
}

// Plays the installed scripts, if any, after the trackers wrote the frame's hands.
#[cfg(all(feature = "scripted-hand", headset))]
pub(crate) fn apply(frame: &mut XrFrame) {
    for script in SCRIPTS.iter().filter_map(OnceLock::get) {
        play(script, frame);
    }
}

#[cfg(test)]
#[path = "tests/scripted_hand_tests.rs"]
mod tests;
