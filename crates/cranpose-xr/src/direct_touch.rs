//! A hand's direct touch of the panels: the tip of its index finger hovers over a panel's face when
//! it comes near, and presses the face where it touches it.

use crate::{
    Hand as Side,
    hit::{Hit, reachable},
    reach::Scene,
    space::{FacePlace, PanelSlot},
    vector::lerp,
};

// The fingertip hovers over a panel while its skin stands this far in front of the face or nearer,
// in meters.
const HOVER_HEIGHT: f32 = 0.06;
// A press ends when the skin comes back this far in front of the face, in meters. Hand tracking
// moves a still fingertip by a few millimeters, and that jitter must not end a press and start
// another one.
const RELEASE_HEIGHT: f32 = 0.005;
// A fingertip that stands up to this far behind a panel's face, within its edges, has pushed
// through the panel, in meters. No panel takes its touch then, so a finger that pushes through a
// panel presses nothing behind it.
const THROUGH_DEPTH: f32 = 0.3;
// A fingertip moves at most this far toward a face between two frames, in meters. A longer step is
// a jump of the tracking, and it presses nothing.
const LONGEST_STEP: f32 = 0.1;

// The tip of a hand's index finger in a frame: the place of its joint in the room, and the radius
// of the finger there, in meters. The skin stands the radius from the joint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fingertip {
    pub(crate) position: [f32; 3],
    pub(crate) radius: f32,
}

// What a hand's fingertip does to the panels in a frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Contact {
    // No panel is near the fingertip, or the hand may not touch now: the hand's ray points.
    Away,
    // The fingertip hovers in front of a panel, over the hit.
    Near(Hit),
    // The fingertip went into a panel's face in this frame, at the hit.
    Down(Hit),
    // The fingertip stays on the panel that it pressed, over the hit.
    On(Hit),
    // The fingertip left the panel that it pressed. `near` is where it hovers now.
    Up { near: Option<Hit> },
}

impl Contact {
    // The place that the fingertip is over in this frame, or `None` while the hand's ray points.
    pub(crate) fn hit(self) -> Option<Hit> {
        match self {
            Self::Away => None,
            Self::Near(hit) | Self::Down(hit) | Self::On(hit) => Some(hit),
            Self::Up { near } => near,
        }
    }
}

// One hand's fingertip across frames.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DirectTouch {
    // The fingertip in the frame before
    last: Option<Fingertip>,
    // The panel that the fingertip presses
    pressed: Option<u64>,
}

impl DirectTouch {
    // What `tip` does to the panels of `scene` in this frame. `tip` is `None` while the hand has
    // no fingertip that may touch. A touch begins only where the fingertip goes into a face from the
    // front, so a fingertip that the cameras first see behind a face presses nothing.
    pub(crate) fn update(
        &mut self,
        tip: Option<Fingertip>,
        scene: Scene<'_>,
        side: Side,
    ) -> Contact {
        let last = std::mem::replace(&mut self.last, tip);
        let Some(tip) = tip else {
            return match self.pressed.take() {
                Some(_) => Contact::Up { near: None },
                None => Contact::Away,
            };
        };
        if let Some(panel) = self.pressed {
            if let Some(hit) = touched(panel, tip, scene, side) {
                return Contact::On(hit);
            }
            self.pressed = None;
            return Contact::Up {
                near: survey(None, tip, scene, side).near,
            };
        }
        let survey = survey(last, tip, scene, side);
        if let Some(hit) = survey.entered {
            self.pressed = Some(hit.slot.id);
            return Contact::Down(hit);
        }
        survey.near.map_or(Contact::Away, Contact::Near)
    }
}

// Where a fingertip stands against the panels in a frame.
struct Survey {
    // The panel whose face the fingertip went into since the frame before, and where
    entered: Option<Hit>,
    // The panel that the fingertip hovers over
    near: Option<Hit>,
}

// One pass over the faces of `scene` that `side` reaches, with the fingertip at `tip` now and at
// `last` in the frame before.
//
// The fingertip hovers over the nearest face that it stands in front of, within HOVER_HEIGHT and
// within the face's edges. It enters a face that it went into from the front since the frame before,
// where it crossed the face within its edges. Of two such faces the one that it crossed first takes
// the press. A fingertip that has pushed through a face hovers over nothing, and a crossing behind a
// face that it had pushed through presses nothing.
fn survey(last: Option<Fingertip>, tip: Fingertip, scene: Scene<'_>, side: Side) -> Survey {
    let mut near: Option<(&PanelSlot, FacePlace)> = None;
    let mut through = false;
    // The share of the step at which the fingertip crossed the face, and the place of the crossing
    let mut entered: Option<(f32, &PanelSlot, [f32; 3])> = None;
    for slot in scene.panels.iter().filter(|slot| reachable(slot, side)) {
        let Some(place) = slot.face_place(tip.position) else {
            continue;
        };
        let skin = place.height - tip.radius;
        if skin > 0.0 {
            if skin <= HOVER_HEIGHT
                && place.within(slot)
                && near.is_none_or(|(_, nearest)| place.height < nearest.height)
            {
                near = Some((slot, place));
            }
            continue;
        }
        through |= skin > -THROUGH_DEPTH && place.within(slot);
        let Some(crossing) = last.and_then(|last| crossing(slot, last, tip, skin)) else {
            continue;
        };
        if entered.is_none_or(|(first, ..)| crossing.0 < first) {
            entered = Some((crossing.0, slot, crossing.1));
        }
    }
    let entered = entered.and_then(|(_, slot, point)| {
        let place = slot.face_place(point)?;
        let behind_another = scene.panels.iter().any(|other| {
            other.id != slot.id
                && reachable(other, side)
                && pushed_through(other, point, tip.radius)
        });
        (!behind_another).then(|| hit_of(slot, place))
    });
    Survey {
        entered,
        near: near
            .filter(|_| !through)
            .map(|(slot, place)| hit_of(slot, place)),
    }
}

// Where the fingertip crossed `slot`'s face from the front between `last` and `tip`, whose skin
// stands `skin` meters from the face now, at or behind it: the share of the step at the crossing
// and the crossing's place in the room. A crossing outside the face's edges is none.
fn crossing(
    slot: &PanelSlot,
    last: Fingertip,
    tip: Fingertip,
    skin: f32,
) -> Option<(f32, [f32; 3])> {
    let before = slot.face_place(last.position)?.height - last.radius;
    if !(before > 0.0 && before <= LONGEST_STEP) {
        return None;
    }
    let share = before / (before - skin);
    let point = lerp(last.position, tip.position, share);
    slot.face_place(point)
        .is_some_and(|place| place.within(slot))
        .then_some((share, point))
}

// Whether a fingertip of `radius` at `point` has pushed through `slot`'s face.
fn pushed_through(slot: &PanelSlot, point: [f32; 3], radius: f32) -> bool {
    slot.face_place(point).is_some_and(|place| {
        let skin = place.height - radius;
        skin <= 0.0 && skin > -THROUGH_DEPTH && place.within(slot)
    })
}

// Where the fingertip stands over the panel `panel` that it presses, while it still presses it:
// within the face's edges and no more than RELEASE_HEIGHT in front of the face.
fn touched(panel: u64, tip: Fingertip, scene: Scene<'_>, side: Side) -> Option<Hit> {
    let slot = scene
        .panels
        .iter()
        .find(|slot| slot.id == panel && reachable(slot, side))?;
    let place = slot.face_place(tip.position)?;
    (place.within(slot) && place.height - tip.radius <= RELEASE_HEIGHT).then(|| hit_of(slot, place))
}

// The fingertip's hit on `slot` at `place`. Its distance is the joint's height above the face.
fn hit_of(slot: &PanelSlot, place: FacePlace) -> Hit {
    Hit {
        slot: *slot,
        x: place.x,
        y: place.y,
        distance: place.height,
        pick: None,
    }
}
