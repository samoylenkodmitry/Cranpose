//! A hand that a build plays in place of the cameras' hand.

use std::{sync::OnceLock, time::Duration};

use crate::{
    Hand, HandAim, HandState, Pinch, Pose, TrackingState, XrFrame,
    key_layout::{self, KeyKind, PlacedKey},
    keyboard_state::KeyboardState,
    pose::turn_of_axes,
    space::field_dock::{below, point_on},
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
/// It needs `Use::hand_tracking()` in the app's build, as a real hand does, and the crate's
/// `scripted-hand` feature. Jetpack XR has no such API.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptedHand {
    hand: Hand,
    // The aim pose of each cycle, in turn
    aims: Vec<Pose>,
    cycle: PinchCycle,
    // The time of the first frame that played the script, on the clock of the frames
    started: OnceLock<Duration>,
}

impl ScriptedHand {
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
            started: OnceLock::new(),
        }
    }

    // The tracked hand at `time` after the script's start.
    fn state_at(&self, time: Duration) -> HandState {
        let strength = self.cycle.strength_at(time);
        let (cycles, _) = self.cycle.position(time);
        let index = usize::try_from(cycles).map_or(0, |cycles| cycles % self.aims.len());
        let aim = self.aims[index];
        HandState {
            tracking_state: TrackingState::Tracking,
            aim: Some(HandAim {
                pose: aim,
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

// The aim pose at `from` that points at `at`. It points along its -z, and its x axis stays level.
fn aim_pose(from: [f32; 3], at: [f32; 3]) -> Pose {
    let z = normalize(sub(from, at));
    let x = normalize(cross([0.0, 1.0, 0.0], z));
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
