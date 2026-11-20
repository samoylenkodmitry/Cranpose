//! A panel that a hand holds by one of its handles: the carry that follows the controller, and the
//! resize that follows the ray.

use super::{Frame, LET_GO, Pointer};
use crate::{
    Pose, SpatialMoveEventType, SpatialResizeEventType,
    carry::Carry,
    gestures::Gestures,
    handle::HandleKind,
    reach::{Handle, Reach},
    resize::Resize,
    space::{PanelMove, PanelResize},
};

// A thumbstick pushed all the way moves a carried panel by this share of its distance each
// second.
const PUSH_SPEED: f32 = 1.2;

// A panel the user carries.
#[derive(Clone, Copy, Debug)]
pub(super) struct Carried {
    panel: u64,
    carry: Carry,
    // The panel's pose in the last step, which a step that ends the carry repeats when the
    // controller is lost
    pose: Pose,
}

// A panel the user resizes.
#[derive(Clone, Copy, Debug)]
pub(super) struct Resized {
    panel: u64,
    resize: Resize,
    // The panel's size in dp in the last step, which a step that ends the resize repeats when the
    // controller is lost
    size: [f32; 2],
    // The highest pull of the trigger since the grab
    peak: f32,
}

// What the user holds by a handle.
#[derive(Clone, Copy, Debug)]
pub(super) enum Held {
    Carry(Carried),
    Resize(Resized),
}

impl Held {
    pub(super) fn handle(&self) -> (u64, HandleKind) {
        match self {
            Self::Carry(carried) => (carried.panel, HandleKind::Move),
            Self::Resize(resized) => (resized.panel, HandleKind::Resize),
        }
    }
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the held panel in its own file"
)]
impl Pointer {
    // A pull on a handle takes hold of its panel: a move handle carries the panel along with the
    // controller, and a corner handle resizes it. The panel's composition sees the pointer leave.
    pub(super) fn grab(&mut self, reach: &Reach, handle: &Handle, out: &mut Gestures) {
        let slot = &handle.slot;
        let grabbed = match handle.kind {
            HandleKind::Move => reach.pose.map(|aim| {
                step(out, slot.id, SpatialMoveEventType::Start, slot.pose);
                Held::Carry(Carried {
                    panel: slot.id,
                    carry: Carry::grab(&aim, &slot.pose),
                    pose: slot.pose,
                })
            }),
            HandleKind::Resize => {
                reach
                    .ray
                    .and_then(|ray| Resize::grab(&ray, slot))
                    .map(|resize| {
                        let size = slot.size();
                        resize_step(out, slot.id, SpatialResizeEventType::Start, size);
                        Held::Resize(Resized {
                            panel: slot.id,
                            resize,
                            size,
                            peak: reach.trigger,
                        })
                    })
            }
        };
        if grabbed.is_some() {
            self.held = grabbed;
            self.hover(None, out);
        }
    }

    // The held panel follows the controller.
    pub(super) fn hold(&mut self, reach: &Reach, frame: &Frame<'_>, out: &mut Gestures) {
        match self.held {
            Some(Held::Carry(_)) => self.carry(reach, frame.dt, out),
            Some(Held::Resize(_)) => self.resize(reach, frame, out),
            None => {}
        }
    }

    // The carried panel follows the controller. The thumbstick pushes it away or pulls it near.
    fn carry(&mut self, reach: &Reach, dt: f32, out: &mut Gestures) {
        let (Some(Held::Carry(carried)), Some(aim)) = (&mut self.held, reach.pose) else {
            return;
        };
        let carry = &mut carried.carry;
        carry.push(reach.stick[1] * PUSH_SPEED * carry.distance() * dt);
        let pose = carry.pose(&aim);
        if pose != carried.pose {
            carried.pose = pose;
            step(out, carried.panel, SpatialMoveEventType::Moving, pose);
        }
    }

    // The resized panel takes the size that the ray gives it. A ray that meets the panel's plane
    // nowhere leaves the size as it was. So does a trigger that is on its way up: letting go
    // turns the controller as a pull does.
    fn resize(&mut self, reach: &Reach, frame: &Frame<'_>, out: &mut Gestures) {
        let (Some(Held::Resize(resized)), Some(ray)) = (&mut self.held, reach.ray) else {
            return;
        };
        resized.peak = resized.peak.max(reach.trigger);
        if reach.trigger < resized.peak - LET_GO {
            return;
        }
        let Some(slot) = frame
            .scene
            .panels
            .iter()
            .find(|slot| slot.id == resized.panel)
        else {
            return;
        };
        let Some(size) = resized.resize.size(&ray, &slot.face()) else {
            return;
        };
        // Sizes are whole dp, so two equal sizes compare equal
        #[expect(clippy::float_cmp, reason = "the sizes are rounded to whole dp")]
        let changed = size != resized.size;
        if changed {
            resized.size = size;
            resize_step(out, resized.panel, SpatialResizeEventType::Resizing, size);
        }
    }

    // The trigger went up while the user held a panel by a handle, or the controller was lost.
    pub(super) fn put_down(&mut self, reach: &Reach, out: &mut Gestures) {
        match self.held.take() {
            Some(Held::Carry(carried)) => {
                step(out, carried.panel, SpatialMoveEventType::End, carried.pose);
            }
            Some(Held::Resize(resized)) => {
                resize_step(
                    out,
                    resized.panel,
                    SpatialResizeEventType::End,
                    resized.size,
                );
            }
            None => {}
        }
        self.hover(reach.hit, out);
    }
}

fn step(out: &mut Gestures, panel: u64, event_type: SpatialMoveEventType, pose: Pose) {
    out.moves.push(PanelMove {
        panel,
        event_type,
        pose,
    });
}

fn resize_step(out: &mut Gestures, panel: u64, event_type: SpatialResizeEventType, size: [f32; 2]) {
    out.resizes.push(PanelResize {
        panel,
        event_type,
        size,
    });
}
