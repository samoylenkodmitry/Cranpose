// Where the walk has taken the eye: the room it stands in and the way back,
// what it has done on the way, which the entity talks about, and whether
// the entity haunts it. The eye stands in the window scene's room; when it
// walks through a doorway, the room behind becomes the window scene's room,
// so the rooms never end.

use std::sync::Arc;

use cranpose_core::{mutableStateOf, remember, MutableState};
use cranpose_ui::{
    composable,
    space::{EyePose, Vec3},
    Eye,
};

use super::plan::{Plan, Visit};

// How far the lamps burn while the lights are out.
const EMERGENCY_POWER: f32 = 0.06;
// How many rooms the entity haunts the eye in after it says to turn around.
const HAUNTED_ROOMS: u32 = 5;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Journey {
    pub(super) visit: MutableState<Visit>,
    // The rooms the eye has walked into, counting the first.
    pub(super) rooms: MutableState<u32>,
    // How many times W was pressed.
    pub(super) presses: MutableState<u32>,
    // How many times a lever was pulled.
    pub(super) pulls: MutableState<u32>,
    // Whether a lever put the lights out, in every room.
    pub(super) lights_out: MutableState<bool>,
    // How many more rooms the entity haunts the eye in, behind its back.
    pub(super) haunted: MutableState<u32>,
}

#[composable]
pub(super) fn rememberJourney() -> Journey {
    remember(|| Journey {
        visit: mutableStateOf(Visit::START),
        rooms: mutableStateOf(1),
        presses: mutableStateOf(0),
        pulls: mutableStateOf(0),
        lights_out: mutableStateOf(false),
        haunted: mutableStateOf(0),
    })
    .with(|journey| *journey)
}

impl Journey {
    // How far the lamps burn, from 0 to 1.
    pub(super) fn power(&self) -> f32 {
        if self.lights_out.get() {
            EMERGENCY_POWER
        } else {
            1.0
        }
    }

    // A lever was pulled: the lights go out, or come back.
    pub(super) fn pull(&self) {
        self.pulls.update(|pulls| *pulls += 1);
        self.lights_out.update(|out| *out = !*out);
    }

    // The entity told the eye to turn around: it stands behind the eye's
    // back for the next rooms.
    pub(super) fn haunt(&self) {
        self.haunted.set(HAUNTED_ROOMS);
    }

    // After `eye` walked from `from` in its room: if it walked through a
    // doorway, the room behind becomes its room, the room it left lies
    // behind the door it came in by, and the eye stands in the new room's
    // coordinates, where the renderer already put it.
    pub(super) fn follow(&self, eye: &Eye, from: Vec3) {
        let pose = eye.pose();
        if pose.doorways.is_empty() {
            return;
        }
        let visit = self.visit.get();
        let Some(door) = Plan::of(visit.room).nearest_door(from) else {
            return;
        };
        self.visit.set(visit.through(door));
        self.rooms.update(|rooms| *rooms += 1);
        self.haunted
            .update(|rooms| *rooms = rooms.saturating_sub(1));
        eye.place(EyePose {
            doorways: Arc::from([]),
            ..pose
        });
    }
}
