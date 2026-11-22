//! Where following subspaces stand in the room: each at its target, as its mode says.

use std::time::Duration;

use crate::{
    FollowMode, FollowTarget, FollowThresholds, Hand, Pose, Recenter, SoftFollowMode,
    TrackedDimensions,
    pose::{heading_degrees, pitch_yaw_roll_degrees, toward, turn_degrees},
    space::{SUBSPACE_ORIGIN, TrackedSpace},
};

// A soft follow has reached its target when it is this close: 1 mm and a tenth of a degree.
const ARRIVED_METERS: f32 = 0.001;
const ARRIVED_DEGREES: f32 = 0.1;

// Where the origin of a `Soft` subspace is in its follow.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Chase {
    // The origin holds still. `since` is the time from which the target stood past a threshold.
    Resting { since: Option<Duration> },
    // The origin glides toward the target.
    Moving,
}

// The origin of one following subspace.
struct Follower {
    id: u64,
    target: FollowTarget,
    // Where the subspace's origin stands in the room. `None` while nothing has placed a subspace
    // that follows a controller: it has no place to show its panels.
    at: Option<Pose>,
    // Whether a `Snap` subspace waits for the target to place it again. A recenter sets it.
    owes: bool,
    // The last heading that the target had, in degrees. A target that points straight up or
    // down has none, so the origin keeps this one.
    heading: f32,
    // Whether the origin moved in the last frame tracked
    moved: bool,
    // How a `Soft` subspace follows its target now
    chase: Chase,
    // The time of the last frame tracked, from which a `Soft` subspace measures its glide
    time: Option<Duration>,
}

impl Follower {
    fn new(id: u64, target: FollowTarget) -> Self {
        // Until the headset has tracked the head, a view-following subspace stands where a
        // `Subspace` does
        let at = matches!(target, FollowTarget::View(_)).then_some(SUBSPACE_ORIGIN);
        Self {
            id,
            target,
            at,
            owes: true,
            heading: 0.0,
            moved: false,
            chase: Chase::Resting { since: None },
            time: None,
        }
    }

    fn mode(&self) -> FollowMode {
        match &self.target {
            FollowTarget::View(mode)
            | FollowTarget::Controller(_, mode)
            | FollowTarget::Anchor(_, mode) => *mode,
        }
    }

    // The space in which the compositor places the subspace's panels with the target's newest
    // pose: the head's or the controller's aim, for a subspace that follows every move of it. A
    // `Snap` subspace stands still in the room, and an upright one does not turn with every move.
    // An anchor holds still in the room, so the room's space places it exactly.
    fn tracked_space(&self) -> Option<TrackedSpace> {
        if self.mode() != FollowMode::Tight(TrackedDimensions::All) {
            return None;
        }
        match &self.target {
            FollowTarget::View(_) => Some(TrackedSpace::View),
            FollowTarget::Controller(hand, _) => Some(TrackedSpace::Aim(*hand)),
            FollowTarget::Anchor(..) => None,
        }
    }

    // Where the origin stands when the target is at `head`, at the pose that `controllers` holds
    // for its hand or at its anchor's pose, or `None` while the runtime does not track the target.
    // The origin has the target's pose in the tracked dimensions. An upright origin has the
    // target's place and its turn about the vertical axis.
    fn aligned(&mut self, head: Option<&Pose>, controllers: &[Option<Pose>; 2]) -> Option<Pose> {
        let (pose, offset) = match &self.target {
            FollowTarget::View(_) => (*head?, SUBSPACE_ORIGIN),
            FollowTarget::Controller(hand, _) => (controllers[hand.index()]?, Pose::IDENTITY),
            FollowTarget::Anchor(anchor, _) => {
                (anchor.is_located().then(|| anchor.pose())?, Pose::IDENTITY)
            }
        };
        let base = match self.mode().dimensions() {
            TrackedDimensions::All => pose,
            TrackedDimensions::TranslationAndYaw => {
                if let Some(heading) = heading_degrees(&pose) {
                    self.heading = heading;
                }
                Pose {
                    position: pose.position,
                    orientation: turn_degrees(0.0, self.heading, 0.0),
                }
            }
        };
        Some(base.then(&offset))
    }

    // Moves the origin to where the target puts it now, in the frame at `time`. A `Tight`
    // subspace follows in every frame. A `Snap` subspace takes a place once, then again after each
    // recenter. A `Soft` subspace takes its first place as a `Snap` one does, then glides after the
    // target.
    fn follow(&mut self, head: Option<&Pose>, controllers: &[Option<Pose>; 2], time: Duration) {
        let at = match self.mode() {
            FollowMode::Tight(_) => match (self.aligned(head, controllers), &self.target) {
                // A head that the headset lost, and an anchor that the runtime does not place now,
                // leave the content where it was
                (None, FollowTarget::View(_) | FollowTarget::Anchor(..)) => self.at,
                (aligned, _) => aligned,
            },
            FollowMode::Snap(_) | FollowMode::Soft(_) if self.owes => {
                let aligned = self.aligned(head, controllers);
                self.owes = aligned.is_none();
                self.chase = Chase::Resting { since: None };
                aligned.or(self.at)
            }
            FollowMode::Snap(_) => self.at,
            FollowMode::Soft(soft) => match (self.aligned(head, controllers), self.at) {
                (Some(target), Some(at)) => Some(self.glide(&soft, &at, &target, time)),
                // A lost controller hides its content, which stands at the controller again when
                // the controller comes back
                (None, _) if matches!(self.target, FollowTarget::Controller(..)) => {
                    self.owes = true;
                    None
                }
                (_, at) => at,
            },
        };
        self.time = Some(time);
        self.moved = at != self.at;
        self.at = at;
    }

    // The next place of a `Soft` origin at `at` whose target is at `target`. It rests until the
    // target stays past a threshold for the start delay, then covers half its distance in each
    // half life until it reaches the target.
    fn glide(&mut self, soft: &SoftFollowMode, at: &Pose, target: &Pose, time: Duration) -> Pose {
        if let Chase::Resting { since } = self.chase {
            if !past(&soft.start_thresholds, at, target) {
                self.chase = Chase::Resting { since: None };
                return *at;
            }
            let since = since.unwrap_or(time);
            if time.saturating_sub(since) < soft.start_delay {
                self.chase = Chase::Resting { since: Some(since) };
                return *at;
            }
            self.chase = Chase::Moving;
        }
        let elapsed = self
            .time
            .map_or(Duration::ZERO, |before| time.saturating_sub(before));
        let half_lives = elapsed.as_secs_f32() / soft.half_life.as_secs_f32().max(f32::EPSILON);
        let next = toward(at, target, 1.0 - 0.5_f32.powf(half_lives));
        if past(&ARRIVED, &next, target) {
            next
        } else {
            self.chase = Chase::Resting { since: None };
            *target
        }
    }

    fn standing(&self) -> Standing {
        let hand = match &self.target {
            FollowTarget::View(_) | FollowTarget::Anchor(..) => None,
            FollowTarget::Controller(hand, _) => Some(*hand),
        };
        Standing {
            at: self.at,
            hand,
            tracked: self.tracked_space(),
        }
    }
}

// How close a soft follow must come to its target to stop.
const ARRIVED: FollowThresholds = FollowThresholds {
    translation_meters: ARRIVED_METERS,
    pitch_degrees: ARRIVED_DEGREES,
    yaw_degrees: ARRIVED_DEGREES,
    roll_degrees: ARRIVED_DEGREES,
};

// Whether `target` stands past one of `thresholds` from `at`: farther away, or turned more in pitch,
// yaw or roll.
fn past(thresholds: &FollowThresholds, at: &Pose, target: &Pose) -> bool {
    let distance = [0, 1, 2]
        .map(|axis| target.position[axis] - at.position[axis])
        .iter()
        .map(|d| d * d)
        .sum::<f32>()
        .sqrt();
    let [from, to] = [at, target].map(pitch_yaw_roll_degrees);
    // An angle that wraps past a half turn counts the short way round
    let turned = |axis: usize| ((to[axis] - from[axis] + 540.0).rem_euclid(360.0) - 180.0).abs();
    distance > thresholds.translation_meters
        || turned(0) > thresholds.pitch_degrees
        || turned(1) > thresholds.yaw_degrees
        || turned(2) > thresholds.roll_degrees
}

// Where a following subspace stands.
#[derive(Clone, Copy)]
pub(crate) struct Standing {
    // The origin's place in the room, or `None` while the subspace has none
    pub(crate) at: Option<Pose>,
    // The hand whose controller the subspace follows
    pub(crate) hand: Option<Hand>,
    // The space in which the compositor places the subspace's panels, or `None` for the room's
    pub(crate) tracked: Option<TrackedSpace>,
}

// The following subspaces of an app, and what places them.
#[derive(Default)]
pub(crate) struct FollowOrigins {
    followers: Vec<Follower>,
    head: Option<Pose>,
    controllers: [Option<Pose>; 2],
    // The time at which the runtime's recenter takes effect, until a frame at or after it came
    recentering: Option<Duration>,
    // The time of the last frame tracked
    time: Duration,
}

impl FollowOrigins {
    // Adds the following subspace `id`. A target that the headset has tracked already places it
    // at once. Otherwise the first frame that tracks it does.
    pub(crate) fn add(&mut self, id: u64, target: FollowTarget) {
        let mut follower = Follower::new(id, target);
        follower.follow(self.head.as_ref(), &self.controllers, self.time);
        self.followers.push(follower);
    }

    pub(crate) fn remove(&mut self, id: u64) {
        self.followers.retain(|follower| follower.id != id);
    }

    // Where the subspace `id` stands, or `None` for a subspace that is gone.
    pub(crate) fn standing(&self, id: u64) -> Option<Standing> {
        self.followers
            .iter()
            .find(|follower| follower.id == id)
            .map(Follower::standing)
    }

    // Where the subspace `id` stands now, if it moved in the last frame tracked.
    pub(crate) fn moved(&self, id: u64) -> Option<Standing> {
        self.followers
            .iter()
            .find(|follower| follower.id == id && follower.moved)
            .map(Follower::standing)
    }

    // Takes the poses of the head and of the controllers in the frame at `time`. A `Tight`
    // subspace follows them. A `Snap` subspace that has no place yet takes one. Every `Snap`
    // subspace takes a new place when the runtime has recentered, in the first frame at or after
    // the time of the change: the poses are in the new room only from then on. Returns whether
    // an origin moved. A frame writes no state and allocates nothing.
    pub(crate) fn track(
        &mut self,
        head: Option<Pose>,
        controllers: [Option<Pose>; 2],
        time: Duration,
        recenter: Option<Recenter>,
    ) -> bool {
        self.head = head;
        self.controllers = controllers;
        self.time = time;
        if let Some(change) = recenter {
            self.recentering = Some(change.change_time);
        }
        let recentered = self.recentering.is_some_and(|at| at <= time);
        if recentered {
            self.recentering = None;
        }
        for follower in &mut self.followers {
            follower.owes |= recentered;
            follower.follow(self.head.as_ref(), &self.controllers, time);
        }
        self.followers.iter().any(|follower| follower.moved)
    }
}
