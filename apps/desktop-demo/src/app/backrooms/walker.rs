// The eye that walks the backrooms: the window scene's eye, which the walls
// stop and the doorways let through. Keys walk and turn it, a drag looks
// around, a turn of a watch's crown or of a mouse wheel steps it ahead or
// back, and on a touch screen a finger held down on the left half walks it
// the way the finger moves.

use std::{cell::Cell, f32::consts::PI, rc::Rc};

use cranpose_core::{mutableStateOf, remember, MutableState, NodeId};
use cranpose_ui::{
    composable,
    space::{Camera, EyePose, Light, Scene3d, Vec3},
    Eye, FocusRequester, KeyEvent, Point, PointerEvent, PointerEventKind, PointerId,
    PointerInputScope, RotaryScrollEvent,
};

use super::{
    journey::Journey,
    plan::{Plan, RoomId},
};
use crate::app::walk_keys::WalkKeys;

const WALK_SPEED: f32 = 260.0;
const TURN_SPEED: f32 = 1.8;
const LOOK_PER_PIXEL: f32 = 0.004;
// How far a crown or wheel turn of one pixel steps the eye.
const STEP_PER_SCROLL_PIXEL: f32 = 1.5;
const EYE_HEIGHT: f32 = 165.0;
const EYE_RADIUS: f32 = 40.0;
// How far a finger moves from where it went down to walk at full speed.
const STICK_REACH: f32 = 70.0;
// How far the eye walks in a step, and how far a step lowers it.
const STRIDE: f32 = 75.0;
const BOB: f32 = 3.5;

// A finger that walks the eye: where it went down and where it is now.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Stick {
    pointer: PointerId,
    from: Point,
    to: Point,
}

impl Stick {
    // How far ahead and to the right the finger walks the eye, each from
    // -1 to 1.
    fn push(self) -> (f32, f32) {
        let (right, ahead) = (
            (self.to.x - self.from.x) / STICK_REACH,
            (self.from.y - self.to.y) / STICK_REACH,
        );
        let length = (right * right + ahead * ahead).sqrt().max(1.0);
        (ahead / length, right / length)
    }
}

// A state holder, remembered once by the tab.
#[derive(Clone)]
pub(super) struct Walker {
    pub(super) eye: Eye,
    pub(super) journey: Journey,
    keys: MutableState<WalkKeys>,
    stick: MutableState<Option<Stick>>,
    // How far the eye has walked, which its steps bob by.
    walked: MutableState<f32>,
    pub(super) focus: FocusRequester,
    // The window scene's node. A walk through a doorway makes the room
    // behind it the window scene's with a new node, and the eye waits for
    // the renderer to place that scene before it walks on among its walls.
    pub(super) scene: Rc<Cell<Option<NodeId>>>,
}

// One walker equals another when both are the one the tab remembered.
impl PartialEq for Walker {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.scene, &other.scene)
    }
}

// The eye, standing first in the first room, in front of the door behind
// it and looking into the room.
#[composable]
pub(super) fn rememberWalker(journey: Journey) -> Walker {
    remember(move || {
        let at = Plan::of(RoomId::FIRST).start() - Vec3::new(0.0, EYE_HEIGHT, 0.0);
        Walker {
            eye: Eye::new(EyePose::standing(at, 0.0, 0.0), EYE_RADIUS),
            journey,
            keys: mutableStateOf(WalkKeys::default()),
            stick: mutableStateOf(None),
            walked: mutableStateOf(0.0),
            focus: FocusRequester::new(),
            scene: Rc::default(),
        }
    })
    .with(Clone::clone)
}

impl Walker {
    // Whether a key or a finger walks or turns the eye.
    pub(super) fn is_moving(&self) -> bool {
        !self.keys.get().is_empty() || self.stick.get().is_some()
    }

    // The scene through the eye, lowered a little with each step.
    pub(super) fn scene(&self, focal_length: f32, light: Option<Light>) -> Scene3d {
        let mut scene = self.eye.scene(focal_length, light);
        let pose = self.eye.pose();
        let phase = self.walked.get() / STRIDE * PI;
        let at = pose.position + Vec3::new(0.0, phase.sin().abs() * BOB, 0.0);
        scene.camera = Camera::look_at(at, at + pose.forward, pose.down, focal_length);
        scene
    }

    // Takes a walking key, and counts the presses of W.
    pub(super) fn key(&self, event: &KeyEvent) -> bool {
        let before = self.keys.get();
        let Some(keys) = before.after(event) else {
            return false;
        };
        if keys.ahead() > before.ahead() {
            self.journey.press();
        }
        self.keys.set(keys);
        true
    }

    // Steps the eye ahead, or back for a negative turn, as a crown or a
    // wheel turns.
    pub(super) fn scroll(&self, event: RotaryScrollEvent) -> bool {
        let (ahead, _) = self.eye.pose().walking_axes();
        self.walk(ahead * (-event.vertical_scroll_pixels * STEP_PER_SCROLL_PIXEL));
        true
    }

    // Turns and walks the eye as the held keys and the finger on the left
    // half ask, over `seconds`.
    pub(super) fn step(&self, seconds: f32) {
        let keys = self.keys.get();
        let (mut ahead, mut right) = (keys.ahead(), keys.right());
        if let Some(stick) = self.stick.get() {
            let (stick_ahead, stick_right) = stick.push();
            ahead += stick_ahead;
            right += stick_right;
        }
        let length = (ahead * ahead + right * right).sqrt().max(1.0);
        let turn = keys.turn();
        if turn != 0.0 {
            self.eye.turn(turn * TURN_SPEED * seconds, 0.0);
        }
        let (ahead_axis, right_axis) = self.eye.pose().walking_axes();
        self.walk((ahead_axis * ahead + right_axis * right) * (WALK_SPEED * seconds / length));
    }

    // Walks the eye `step` once the renderer has placed the window scene,
    // and follows it through a doorway into the room behind.
    fn walk(&self, step: Vec3) {
        let placed = self
            .scene
            .get()
            .is_some_and(|scene| self.eye.pose_in(scene).is_some());
        if !placed {
            return;
        }
        let from = self.eye.pose().position;
        self.eye.walk(step);
        let pose = self.eye.pose();
        let moved = if pose.doorways.is_empty() {
            (pose.position - from).length()
        } else {
            step.length()
        };
        self.walked.update(|walked| *walked += moved);
        self.journey.follow(&self.eye, from);
    }

    // Looks around as a drag moves, and walks while a finger on the left
    // half of a touch screen moves from where it went down.
    pub(super) async fn look(&self, scope: PointerInputScope) {
        scope
            .await_pointer_event_scope(|events| async move {
                // The pointer that looks around, and where it was last.
                let mut looking: Option<(PointerId, Point)> = None;
                loop {
                    let event = events.await_pointer_event().await;
                    match event.kind {
                        PointerEventKind::Down | PointerEventKind::ButtonDown(_) => {
                            self.press(&event, events.size().width, &mut looking);
                        }
                        PointerEventKind::Move => self.drag(&event, &mut looking),
                        PointerEventKind::Up
                        | PointerEventKind::ButtonUp(_)
                        | PointerEventKind::Cancel => self.release(event.id, &mut looking),
                        _ => {}
                    }
                }
            })
            .await;
    }

    // A press takes the keys back. A press that a control in the rooms
    // took does nothing more; of the others, a finger on the left half of a
    // touch screen of `width` starts walking, and any other starts looking.
    fn press(&self, event: &PointerEvent, width: f32, looking: &mut Option<(PointerId, Point)>) {
        if let Err(error) = self.focus.request_focus() {
            log::warn!("the backrooms could not take focus: {error:?}");
        }
        if event.is_consumed() {
            return;
        }
        let walks = event.source.is_touch_like() && event.position.x < width * 0.5;
        if walks && self.stick.get().is_none() {
            self.stick.set(Some(Stick {
                pointer: event.id,
                from: event.position,
                to: event.position,
            }));
        } else if looking.is_none() {
            *looking = Some((event.id, event.global_position));
        }
    }

    fn drag(&self, event: &PointerEvent, looking: &mut Option<(PointerId, Point)>) {
        if let Some(stick) = self.stick.get().filter(|stick| stick.pointer == event.id) {
            self.stick.set(Some(Stick {
                to: event.position,
                ..stick
            }));
        } else if let Some((pointer, last)) = looking.filter(|(pointer, _)| *pointer == event.id) {
            let now = event.global_position;
            self.eye.turn(
                (now.x - last.x) * LOOK_PER_PIXEL,
                (last.y - now.y) * LOOK_PER_PIXEL,
            );
            *looking = Some((pointer, now));
        }
    }

    fn release(&self, pointer: PointerId, looking: &mut Option<(PointerId, Point)>) {
        if self
            .stick
            .get()
            .is_some_and(|stick| stick.pointer == pointer)
        {
            self.stick.set(None);
        }
        if looking.is_some_and(|(looks, _)| looks == pointer) {
            *looking = None;
        }
    }
}
