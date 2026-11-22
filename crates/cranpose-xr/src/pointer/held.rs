//! A panel that a hand holds by one of its handles: the carry that follows the controller, and the
//! resize that follows the ray.
//!
//! A pull on a handle presses it and moves nothing. The panel moves only once the hand moves it
//! farther than `DRAG_SLOP`, as Compose's touch slop holds a drag back until the finger travels.
//! The panel then takes the whole move since the pull, so it stays where the hand holds it: the
//! carried panel keeps its place against the controller, and the resized corner stays under the
//! ray.

use std::time::Duration;

use super::{DRAG_SLOP, Frame, LET_GO, Pointer};
use crate::{
    METERS_PER_DP, Pose, SpatialMoveEventType, SpatialResizeEventType,
    carry::Carry,
    gestures::Gestures,
    handle::{HandleFocus, HandleKind},
    reach::{Handle, Reach},
    resize::Resize,
    space::{PanelMove, PanelResize, PanelSlot},
    vector::sub,
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
    // Until the panel moves: the handle's place in the room at the pull, and that place in the
    // controller's frame, which the panel's place would follow
    pressed: Option<([f32; 3], [f32; 3])>,
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
    // Whether the size has changed yet
    started: bool,
}

// What the user holds by a handle.
#[derive(Clone, Copy, Debug)]
pub(super) enum Held {
    Carry(Carried),
    Resize(Resized),
}

impl Held {
    pub(super) fn handle(&self) -> HandleFocus {
        let (panel, kind) = match self {
            Self::Carry(carried) => (carried.panel, HandleKind::Move),
            Self::Resize(resized) => (resized.panel, HandleKind::Resize),
        };
        HandleFocus {
            panel,
            kind,
            pressed: true,
        }
    }

    fn started(&self) -> bool {
        match self {
            Self::Carry(carried) => carried.pressed.is_none(),
            Self::Resize(resized) => resized.started,
        }
    }
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the held panel in its own file"
)]
impl Pointer {
    // A pull on a handle takes hold of its panel: a move handle carries the panel along with the
    // controller, and a corner handle resizes it. Nothing moves before the slop. The panel's
    // composition sees the pointer leave, and the controller ticks.
    pub(super) fn grab(
        &mut self,
        reach: &Reach,
        handle: &Handle,
        frame: &Frame<'_>,
        out: &mut Gestures,
    ) {
        let slot = &handle.slot;
        let grabbed = match handle.kind {
            HandleKind::Move => reach.pose.map(|aim| {
                let center = handle.kind.center(slot);
                Held::Carry(Carried {
                    panel: slot.id,
                    carry: Carry::grab(&aim, &slot.pose),
                    pose: slot.pose,
                    pressed: Some((center, aim.inverse().then(&Pose::at(center)).position)),
                })
            }),
            HandleKind::Resize => {
                reach
                    .ray
                    .and_then(|ray| Resize::grab(&ray, slot))
                    .map(|resize| {
                        Held::Resize(Resized {
                            panel: slot.id,
                            resize,
                            size: slot.size(),
                            peak: reach.trigger,
                            started: false,
                        })
                    })
            }
        };
        if grabbed.is_some() {
            self.held = grabbed;
            self.grabbed_at = frame.time;
            self.hover(None, out);
            self.tick(out);
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

    // The carried panel follows the controller once the hand moved it past the slop. The
    // thumbstick pushes it away or pulls it near.
    fn carry(&mut self, reach: &Reach, dt: f32, out: &mut Gestures) {
        let (Some(Held::Carry(carried)), Some(aim)) = (&mut self.held, reach.pose) else {
            return;
        };
        if let Some((place, in_hand)) = carried.pressed {
            let now = aim.then(&Pose::at(in_hand)).position;
            let [x, y, z] = sub(now, place);
            if (x * x + y * y + z * z).sqrt() / METERS_PER_DP <= DRAG_SLOP {
                return;
            }
            carried.pressed = None;
            step(
                out,
                carried.panel,
                SpatialMoveEventType::Start,
                carried.pose,
            );
            out.ticks[self.side.index()] = true;
        }
        let carry = &mut carried.carry;
        carry.push(reach.stick[1] * PUSH_SPEED * carry.distance() * dt);
        let pose = carry.pose(&aim);
        if pose != carried.pose {
            carried.pose = pose;
            step(out, carried.panel, SpatialMoveEventType::Moving, pose);
        }
    }

    // The resized panel takes the size that the ray gives it, once that size differs from the
    // panel's by more than the slop. A ray that meets the panel's plane nowhere leaves the size as
    // it was. So does a trigger that is on its way up: letting go turns the controller as a pull
    // does.
    fn resize(&mut self, reach: &Reach, frame: &Frame<'_>, out: &mut Gestures) {
        let (Some(Held::Resize(resized)), Some(ray)) = (&mut self.held, reach.ray) else {
            return;
        };
        resized.peak = resized.peak.max(reach.trigger);
        if reach.trigger < resized.peak - LET_GO {
            return;
        }
        let Some(slot) = panel(frame, resized.panel) else {
            return;
        };
        let Some(size) = resized.resize.size(&ray, &slot.face()) else {
            return;
        };
        if !resized.started {
            let grown = [0, 1].map(|axis| (size[axis] - resized.size[axis]).abs());
            if grown[0].max(grown[1]) <= DRAG_SLOP {
                return;
            }
            resized.started = true;
            resize_step(
                out,
                resized.panel,
                SpatialResizeEventType::Start,
                resized.size,
            );
            out.ticks[self.side.index()] = true;
        }
        // Sizes are whole dp, so two equal sizes compare equal
        #[expect(clippy::float_cmp, reason = "the sizes are rounded to whole dp")]
        let changed = size != resized.size;
        if changed {
            resized.size = size;
            resize_step(out, resized.panel, SpatialResizeEventType::Resizing, size);
        }
    }

    // The trigger went up while the user held a panel by a handle, or the controller was lost. A
    // panel that moved reports its end, and the controller ticks. A press that never moved the
    // panel ends with nothing to report.
    pub(super) fn put_down(&mut self, reach: &Reach, time: Duration, out: &mut Gestures) {
        let held = self.held.take();
        if let Some(held) = held {
            log::info!(
                "cranpose xr: the {:?} press on a handle ended after {} ms, {}",
                self.side,
                time.saturating_sub(self.grabbed_at).as_millis(),
                if held.started() {
                    "after a drag"
                } else {
                    "with no drag"
                },
            );
        }
        match held {
            Some(Held::Carry(carried)) if carried.pressed.is_none() => {
                step(out, carried.panel, SpatialMoveEventType::End, carried.pose);
                self.tick(out);
            }
            Some(Held::Resize(resized)) if resized.started => {
                resize_step(
                    out,
                    resized.panel,
                    SpatialResizeEventType::End,
                    resized.size,
                );
                self.tick(out);
            }
            _ => {}
        }
        self.hover(reach.hit, out);
    }
}

// The panel `id` as the frame has it.
fn panel<'a>(frame: &Frame<'a>, id: u64) -> Option<&'a PanelSlot> {
    frame.scene.panels.iter().find(|slot| slot.id == id)
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
