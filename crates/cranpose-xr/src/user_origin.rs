//! Where the user's subspaces stand in the room: each in front of the head at the moment it was
//! placed.

use std::time::Duration;

use cranpose::{MutableState, mutableStateOf};

use crate::{Pose, Recenter, pose::turn_degrees, space::SUBSPACE_ORIGIN};

// The origin of one user subspace. The pose is `None` until the headset has tracked the head.
#[derive(Clone, Copy)]
pub(crate) struct UserOrigin {
    pub(crate) id: u64,
    pub(crate) pose: MutableState<Option<Pose>>,
}

struct Anchor {
    origin: UserOrigin,
    // What the origin's state holds, so that a frame needs no read of the state
    at: Option<Pose>,
}

// The user subspaces of an app, and what places them.
#[derive(Default)]
pub(crate) struct UserOrigins {
    anchors: Vec<Anchor>,
    head: Option<Pose>,
    // The time at which the runtime's recenter takes effect, until a frame at or after it came
    recentering: Option<Duration>,
}

impl UserOrigins {
    // Adds the user subspace `id`. A head that the headset has tracked already places it at
    // once. Otherwise the first tracked frame does.
    pub(crate) fn add(&mut self, id: u64) -> UserOrigin {
        let at = self.head.as_ref().map(origin_in_front_of);
        let origin = UserOrigin {
            id,
            pose: mutableStateOf(at),
        };
        self.anchors.push(Anchor { origin, at });
        origin
    }

    pub(crate) fn remove(&mut self, id: u64) {
        self.anchors.retain(|anchor| anchor.origin.id != id);
    }

    // Takes the head's pose in the frame at `time`. A subspace that has no place yet takes one
    // in front of the head. Every subspace takes a new one when the runtime has recentered, in
    // the first frame at or after the time of the change: the head's pose is in the new room only
    // from then on. A frame writes no state otherwise.
    pub(crate) fn track(&mut self, head: Option<Pose>, time: Duration, recenter: Option<Recenter>) {
        self.head = head;
        if let Some(change) = recenter {
            self.recentering = Some(change.change_time);
        }
        let Some(head) = head else {
            return;
        };
        let recentered = self.recentering.is_some_and(|at| at <= time);
        if recentered {
            self.recentering = None;
        }
        let origin = origin_in_front_of(&head);
        for anchor in &mut self.anchors {
            if recentered || anchor.at.is_none() {
                anchor.at = Some(origin);
                anchor.origin.pose.set(Some(origin));
            }
        }
    }
}

// The origin of a subspace that stands in front of the head at `head`: a subspace's usual
// distance ahead of the head along the head's heading, at the head's height, facing it. Only the
// head's turn about the vertical axis counts, so the panels stand upright.
fn origin_in_front_of(head: &Pose) -> Pose {
    let ahead = head.rotate([0.0, 0.0, -1.0]);
    let level = (ahead[0] * ahead[0] + ahead[2] * ahead[2]).sqrt();
    // A head that looks straight up or down has no heading
    let yaw = if level < 1e-3 {
        0.0
    } else {
        (-ahead[0]).atan2(-ahead[2]).to_degrees()
    };
    Pose {
        position: head.position,
        orientation: turn_degrees(0.0, yaw, 0.0),
    }
    .then(&SUBSPACE_ORIGIN)
}
