//! Where following subspaces stand in the room: each at its target, as its mode says.

use std::time::Duration;

use crate::{
    FollowMode, FollowTarget, Hand, Pose, Recenter,
    pose::{heading_degrees, turn_degrees},
    space::SUBSPACE_ORIGIN,
};

// The origin of one following subspace.
struct Anchor {
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
}

impl Anchor {
    fn new(id: u64, target: FollowTarget) -> Self {
        Self {
            id,
            target,
            // Until the headset has tracked the head, a view-following subspace stands where a
            // `Subspace` does
            at: matches!(target, FollowTarget::View(_)).then_some(SUBSPACE_ORIGIN),
            owes: true,
            heading: 0.0,
            moved: false,
        }
    }

    fn mode(&self) -> FollowMode {
        match self.target {
            FollowTarget::View(mode) | FollowTarget::Controller(_, mode) => mode,
        }
    }

    // Where the origin stands when the target is at `head` or at the pose that `controllers`
    // holds for its hand, or `None` while the runtime does not track the target. The origin has the target's
    // place and its turn about the vertical axis, so the content stands upright.
    fn aligned(&mut self, head: Option<&Pose>, controllers: &[Option<Pose>; 2]) -> Option<Pose> {
        let (pose, offset) = match self.target {
            FollowTarget::View(_) => (head?, SUBSPACE_ORIGIN),
            FollowTarget::Controller(hand, _) => {
                (controllers[hand.index()].as_ref()?, Pose::IDENTITY)
            }
        };
        if let Some(heading) = heading_degrees(pose) {
            self.heading = heading;
        }
        Some(
            Pose {
                position: pose.position,
                orientation: turn_degrees(0.0, self.heading, 0.0),
            }
            .then(&offset),
        )
    }

    // Moves the origin to where the target puts it now. A `Tight` subspace follows in every
    // frame. A `Snap` subspace takes a place once, then again after each recenter.
    fn follow(&mut self, head: Option<&Pose>, controllers: &[Option<Pose>; 2]) {
        let at = match self.mode() {
            FollowMode::Tight => match (self.aligned(head, controllers), self.target) {
                // A head that the headset lost leaves the content where it was
                (None, FollowTarget::View(_)) => self.at,
                (aligned, _) => aligned,
            },
            FollowMode::Snap if self.owes => {
                let aligned = self.aligned(head, controllers);
                self.owes = aligned.is_none();
                aligned.or(self.at)
            }
            FollowMode::Snap => self.at,
        };
        self.moved = at != self.at;
        self.at = at;
    }

    fn standing(&self) -> Standing {
        let hand = match self.target {
            FollowTarget::View(_) => None,
            FollowTarget::Controller(hand, _) => Some(hand),
        };
        Standing { at: self.at, hand }
    }
}

// Where a following subspace stands.
#[derive(Clone, Copy)]
pub(crate) struct Standing {
    // The origin's place in the room, or `None` while the subspace has none
    pub(crate) at: Option<Pose>,
    // The hand whose controller the subspace follows
    pub(crate) hand: Option<Hand>,
}

// The following subspaces of an app, and what places them.
#[derive(Default)]
pub(crate) struct FollowOrigins {
    anchors: Vec<Anchor>,
    head: Option<Pose>,
    controllers: [Option<Pose>; 2],
    // The time at which the runtime's recenter takes effect, until a frame at or after it came
    recentering: Option<Duration>,
}

impl FollowOrigins {
    // Adds the following subspace `id`. A target that the headset has tracked already places it
    // at once. Otherwise the first frame that tracks it does.
    pub(crate) fn add(&mut self, id: u64, target: FollowTarget) {
        let mut anchor = Anchor::new(id, target);
        anchor.follow(self.head.as_ref(), &self.controllers);
        self.anchors.push(anchor);
    }

    pub(crate) fn remove(&mut self, id: u64) {
        self.anchors.retain(|anchor| anchor.id != id);
    }

    // Where the subspace `id` stands, or `None` for a subspace that is gone.
    pub(crate) fn standing(&self, id: u64) -> Option<Standing> {
        self.anchors
            .iter()
            .find(|anchor| anchor.id == id)
            .map(Anchor::standing)
    }

    // Where the subspace `id` stands now, if it moved in the last frame tracked.
    pub(crate) fn moved(&self, id: u64) -> Option<Standing> {
        self.anchors
            .iter()
            .find(|anchor| anchor.id == id && anchor.moved)
            .map(Anchor::standing)
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
        if let Some(change) = recenter {
            self.recentering = Some(change.change_time);
        }
        let recentered = self.recentering.is_some_and(|at| at <= time);
        if recentered {
            self.recentering = None;
        }
        for anchor in &mut self.anchors {
            anchor.owes |= recentered;
            anchor.follow(self.head.as_ref(), &self.controllers);
        }
        self.anchors.iter().any(|anchor| anchor.moved)
    }
}
