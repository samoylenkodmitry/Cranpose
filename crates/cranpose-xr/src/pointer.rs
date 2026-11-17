//! One pointer over the panels, from the rays and triggers of two controllers.

use std::time::Duration;

use crate::{
    Pose, Ray,
    aim_filter::{AimFilter, angle_between},
    carry::{Carry, handle_hit},
    ray::Beam,
    space::{Cursor, Mobility, PanelKind, PanelSlot},
};

// A trigger counts as down above PRESS and as up below RELEASE, so a trigger held near one value
// does not flicker.
const PRESS: f32 = 0.6;
const RELEASE: f32 = 0.4;
// A trigger below REST is untouched. The pull of a trigger turns the controller, so a press
// points where the ray pointed in the last frame of rest, unless the pull took longer than
// PULL_TIME.
const REST: f32 = 0.1;
const PULL_TIME: Duration = Duration::from_millis(300);
// A press holds its point while the hand shakes: until the ray turns CLICK_ANGLE degrees in the
// press's first CLICK_TIME, or DRAG_ANGLE degrees later. Then the press drags.
const CLICK_TIME: Duration = Duration::from_millis(350);
const CLICK_ANGLE: f32 = 10.0;
const DRAG_ANGLE: f32 = 4.0;
// A ray that meets no panel shows this long, in meters.
const RAY_LENGTH: f32 = 1.5;
// A thumbstick pushed all the way moves a carried panel by this share of its distance each
// second. A thumbstick within DEAD_ZONE of its middle moves nothing.
const PUSH_SPEED: f32 = 1.2;
const DEAD_ZONE: f32 = 0.15;

// What a controller does in a frame: where it points, how far its trigger is pulled from 0 to
// 1, and how far its thumbstick is pushed forward from -1 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Aim {
    pub(crate) ray: Ray,
    pub(crate) pose: Pose,
    pub(crate) trigger: f32,
    pub(crate) stick: f32,
}

// What the pointer does to the panels' composition, in the atlas's dp.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PointerEvent {
    Move { x: f32, y: f32 },
    Press,
    Release,
    // The pointer left every panel.
    Leave,
}

// A trigger in a frame, compared with the frame before.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Trigger {
    Up,
    // Down now, up in the frame before.
    Pulled,
    Held,
}

// Where a ray meets a panel.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Hit {
    slot: PanelSlot,
    // The place on the panel in dp from its top left corner
    x: f32,
    y: f32,
    // How far along the ray, in meters
    distance: f32,
}

impl Hit {
    // Whether the panel has Cranpose content that the pointer can move over and press.
    fn on_content(&self) -> bool {
        self.slot.kind == PanelKind::Content
    }

    fn position(&self) -> (f32, f32) {
        (self.slot.atlas[0] + self.x, self.slot.atlas[1] + self.y)
    }

    fn cursor(&self, pressed: bool) -> Cursor {
        Cursor {
            panel: self.slot.id,
            x: self.x,
            y: self.y,
            pressed,
        }
    }
}

// One controller across frames.
#[derive(Clone, Copy, Debug, Default)]
struct Hand {
    filter: AimFilter,
    down: bool,
    // The ray in the last frame where the trigger rested, and that frame's time
    rest: Option<(Duration, Ray)>,
    // This frame's ray, the panel it meets and the movable panel whose handle it meets. A panel
    // that holds no content, such as a video surface, stops the ray as a panel does.
    ray: Option<Ray>,
    hit: Option<Hit>,
    // The hit, if the panel has content
    content_hit: Option<Hit>,
    handle: Option<(PanelSlot, f32)>,
    // The controller's own pose and its thumbstick, which carry a panel
    pose: Option<Pose>,
    stick: f32,
}

impl Hand {
    fn update(&mut self, aim: Option<Aim>, time: Duration, panels: &[PanelSlot]) -> Trigger {
        let Some(aim) = aim else {
            // A hand the runtime stops tracking lets its press go
            *self = Self::default();
            return Trigger::Up;
        };
        let ray = self.filter.apply(aim.ray, time);
        self.ray = Some(ray);
        self.hit = nearest_hit(&ray, panels);
        self.content_hit = self.hit.filter(Hit::on_content);
        self.handle = nearest_handle(&ray, panels);
        self.pose = Some(aim.pose);
        self.stick = if aim.stick.abs() > DEAD_ZONE {
            aim.stick
        } else {
            0.0
        };
        let was_down = self.down;
        self.down = if was_down {
            aim.trigger > RELEASE
        } else {
            aim.trigger >= PRESS
        };
        if aim.trigger < REST {
            self.rest = Some((time, ray));
        }
        match (self.down, was_down) {
            (false, _) => Trigger::Up,
            (true, false) => Trigger::Pulled,
            (true, true) => Trigger::Held,
        }
    }

    // The ray and the hit that a press in this frame takes: those from just before the pull.
    fn press_hit(&self, time: Duration, panels: &[PanelSlot]) -> Option<(Ray, Hit)> {
        let before = self
            .rest
            .filter(|(rest, _)| time.saturating_sub(*rest) <= PULL_TIME)
            .and_then(|(_, ray)| Some((ray, nearest_hit(&ray, panels)?)));
        before
            .or_else(|| Some((self.ray?, self.hit?)))
            .filter(|(_, hit)| hit.on_content())
    }

    // Whether the ray meets a panel or a handle.
    fn points(&self) -> bool {
        self.hit.is_some() || self.handle.is_some()
    }

    // The movable panel that a pull in this frame carries: one whose handle is nearer than any
    // panel.
    fn grab(&self) -> Option<PanelSlot> {
        let (slot, distance) = self.handle?;
        self.hit
            .is_none_or(|hit| distance < hit.distance)
            .then_some(slot)
    }

    fn beam(&self) -> Option<Beam> {
        let ray = self.ray?;
        let hit = self.hit.map_or(RAY_LENGTH, |hit| hit.distance);
        let handle = self.handle.map_or(RAY_LENGTH, |(_, distance)| distance);
        Some(Beam {
            ray,
            length: hit.min(handle),
        })
    }
}

// A press in progress.
#[derive(Clone, Copy, Debug)]
struct Press {
    start: Duration,
    // The direction that the press pointed in
    direction: [f32; 3],
    // The press's point while it holds; None once it drags
    held: Option<Hit>,
}

// A panel the user carries.
#[derive(Clone, Copy, Debug)]
struct Carried {
    panel: u64,
    carry: Carry,
}

// The pointer. One controller drives it at a time: the one whose trigger went down last, or
// the other one while only that one points at a panel or a handle.
#[derive(Debug, Default)]
pub(crate) struct Pointer {
    // The controller that drives the pointer: 0 for the left hand, 1 for the right
    hand: usize,
    hands: [Hand; 2],
    press: Option<Press>,
    carried: Option<Carried>,
    // The carried panel's pose in this frame
    moved: Option<(u64, Pose)>,
    // The time of the frame before
    time: Option<Duration>,
    position: Option<(f32, f32)>,
    // The frame's events, kept so a frame does not allocate
    events: Vec<PointerEvent>,
}

impl Pointer {
    // The events of a frame at `time` with `aims` over `panels`, and the cursor to show.
    pub(crate) fn update(
        &mut self,
        aims: [Option<Aim>; 2],
        time: Duration,
        panels: &[PanelSlot],
    ) -> (&[PointerEvent], Option<Cursor>) {
        self.events.clear();
        self.moved = None;
        let dt = self
            .time
            .map_or(0.0, |last| time.saturating_sub(last).as_secs_f32());
        self.time = Some(time);
        let triggers = [0, 1].map(|hand| self.hands[hand].update(aims[hand], time, panels));
        self.choose_hand(triggers);
        let hand = self.hands[self.hand];
        match triggers[self.hand] {
            Trigger::Pulled => match hand.grab() {
                Some(slot) => self.grab(&hand, &slot),
                None => self.pull(&hand, time, panels),
            },
            Trigger::Up if self.carried.take().is_some() => self.hover(hand.content_hit),
            Trigger::Up if self.press.is_some() => {
                self.events.push(PointerEvent::Release);
                self.press = None;
                self.hover(hand.content_hit);
            }
            _ if self.carried.is_some() => self.carry(&hand, dt),
            _ => self.follow(&hand, time),
        }
        let cursor = match self.press {
            Some(Press {
                held: Some(held), ..
            }) => Some(held.cursor(true)),
            _ if self.carried.is_some() => None,
            press => hand.content_hit.map(|hit| hit.cursor(press.is_some())),
        };
        (&self.events, cursor)
    }

    // The panel that the user carries in this frame and its new pose.
    pub(crate) fn moved(&self) -> Option<(u64, Pose)> {
        self.moved
    }

    // The panel whose handle the pointer carries or would grab.
    pub(crate) fn handle(&self) -> Option<u64> {
        self.carried
            .map(|carried| carried.panel)
            .or_else(|| self.hands[self.hand].grab().map(|slot| slot.id))
    }

    // The rays to show, left then right.
    pub(crate) fn beams(&self) -> [Option<Beam>; 2] {
        self.hands.map(|hand| hand.beam())
    }

    // A trigger that goes down takes the pointer to its controller. While no press is on, a
    // controller that points at a panel or a handle takes it from one that points at neither. A press keeps
    // its controller until the trigger goes up.
    fn choose_hand(&mut self, triggers: [Trigger; 2]) {
        if self.press.is_some() || self.carried.is_some() {
            return;
        }
        let pulled = (0..2).rev().find(|&hand| triggers[hand] == Trigger::Pulled);
        if let Some(hand) = pulled {
            self.hand = hand;
        } else if !self.hands[self.hand].points() && self.hands[1 - self.hand].points() {
            self.hand = 1 - self.hand;
        }
    }

    // A press starts where the trigger goes down over a panel. A trigger that was down before
    // the ray reached a panel does not press.
    fn pull(&mut self, hand: &Hand, time: Duration, panels: &[PanelSlot]) {
        match hand.press_hit(time, panels) {
            Some((ray, hit)) => {
                self.move_to(hit.position());
                self.events.push(PointerEvent::Press);
                self.press = Some(Press {
                    start: time,
                    direction: ray.direction,
                    held: Some(hit),
                });
            }
            None => self.hover(hand.content_hit),
        }
    }

    // A pull on a handle takes the panel along with the controller. The panel's composition sees
    // the pointer leave.
    fn grab(&mut self, hand: &Hand, slot: &PanelSlot) {
        let Some(aim) = hand.pose else {
            return;
        };
        self.carried = Some(Carried {
            panel: slot.id,
            carry: Carry::grab(&aim, &slot.pose),
        });
        self.hover(None);
    }

    // The carried panel follows the controller. The thumbstick pushes it away or pulls it near.
    fn carry(&mut self, hand: &Hand, dt: f32) {
        let (Some(carried), Some(aim)) = (&mut self.carried, hand.pose) else {
            return;
        };
        let carry = &mut carried.carry;
        carry.push(hand.stick * PUSH_SPEED * carry.distance() * dt);
        self.moved = Some((carried.panel, carry.pose(&aim)));
    }

    // Moves with the ray: while no press is on, or once a press drags.
    fn follow(&mut self, hand: &Hand, time: Duration) {
        let Some(press) = &mut self.press else {
            self.hover(hand.content_hit);
            return;
        };
        if press.held.is_some()
            && let Some(ray) = hand.ray
        {
            let turn = angle_between(press.direction, ray.direction).to_degrees();
            let limit = if time.saturating_sub(press.start) < CLICK_TIME {
                CLICK_ANGLE
            } else {
                DRAG_ANGLE
            };
            if turn > limit {
                press.held = None;
            }
        }
        if press.held.is_none()
            && let Some(hit) = hand.content_hit
        {
            self.move_to(hit.position());
        }
    }

    fn hover(&mut self, hit: Option<Hit>) {
        match hit {
            Some(hit) => self.move_to(hit.position()),
            None => {
                if self.position.take().is_some() {
                    self.events.push(PointerEvent::Leave);
                }
            }
        }
    }

    fn move_to(&mut self, position: (f32, f32)) {
        if self.position != Some(position) {
            self.events.push(PointerEvent::Move {
                x: position.0,
                y: position.1,
            });
            self.position = Some(position);
        }
    }
}

// The movable panel whose handle the ray meets first, and how far along the ray.
fn nearest_handle(ray: &Ray, panels: &[PanelSlot]) -> Option<(PanelSlot, f32)> {
    panels
        .iter()
        .filter(|slot| slot.mobility == Mobility::Movable)
        .filter_map(|slot| Some((*slot, handle_hit(ray, slot)?)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

// The panel the ray meets first and the place on it.
fn nearest_hit(ray: &Ray, panels: &[PanelSlot]) -> Option<Hit> {
    panels
        .iter()
        .filter_map(|slot| {
            let size = slot.size();
            let hit = slot.hit(ray)?;
            Some(Hit {
                slot: *slot,
                x: hit.u * size[0],
                y: hit.v * size[1],
                distance: hit.distance,
            })
        })
        .min_by(|a, b| a.distance.total_cmp(&b.distance))
}

#[cfg(test)]
#[path = "tests/pointer_tests.rs"]
mod tests;
