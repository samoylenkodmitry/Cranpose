// The eye that walks the backrooms. Keys walk and turn it, a drag looks
// around, a turn of a watch's crown or of a mouse wheel steps it ahead or
// back, and on a touch screen a finger held down on the left half walks it
// the way the finger moves.

use cranpose_core::{mutableStateOf, remember, MutableState};
use cranpose_ui::{
    composable,
    space::{Camera, EyePose, Vec3},
    FocusRequester, KeyEvent, Point, PointerEvent, PointerEventKind, PointerId, PointerInputScope,
    RotaryScrollEvent,
};

use super::maze::{self, Cell, Spot, Toward, CELL};
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

#[derive(Clone)]
pub(super) struct Walker {
    // Where the eye stands and looks, measured from the corner of `cell`.
    pose: MutableState<EyePose>,
    cell: MutableState<Cell>,
    keys: MutableState<WalkKeys>,
    stick: MutableState<Option<Stick>>,
    // How far the eye has walked, which its steps bob by.
    walked: MutableState<f32>,
    pub(super) focus: FocusRequester,
}

// The eye, standing first in the middle of a lit cell and looking down the
// longest straight way out of it.
#[composable]
pub(super) fn rememberWalker() -> Walker {
    remember(|| {
        let cell = maze::spawn();
        let toward = Toward::ALL
            .into_iter()
            .max_by_key(|&toward| cell.open_line(toward, 12))
            .unwrap_or(Toward::North);
        let at = Vec3::new(CELL * 0.5, -EYE_HEIGHT, CELL * 0.5);
        Walker {
            pose: mutableStateOf(EyePose::standing(at, toward.yaw(), 0.0)),
            cell: mutableStateOf(cell),
            keys: mutableStateOf(WalkKeys::default()),
            stick: mutableStateOf(None),
            walked: mutableStateOf(0.0),
            focus: FocusRequester::new(),
        }
    })
    .with(Clone::clone)
}

impl Walker {
    pub(super) fn pose(&self) -> EyePose {
        self.pose.get()
    }

    pub(super) fn cell(&self) -> Cell {
        self.cell.get()
    }

    // Whether a key or a finger walks or turns the eye.
    pub(super) fn is_moving(&self) -> bool {
        !self.keys.get().is_empty() || self.stick.get().is_some()
    }

    // The camera the eye sees through, lowered a little with each step.
    pub(super) fn camera(&self, focal_length: f32) -> Camera {
        let mut pose = self.pose.get();
        let phase = self.walked.get() / STRIDE * std::f32::consts::PI;
        pose.position.y += phase.sin().abs() * BOB;
        pose.camera(focal_length)
    }

    // Takes a walking key.
    pub(super) fn key(&self, event: &KeyEvent) -> bool {
        let Some(keys) = self.keys.get().after(event) else {
            return false;
        };
        self.keys.set(keys);
        true
    }

    // Steps the eye ahead, or back for a negative turn, as a crown or a
    // wheel turns.
    pub(super) fn scroll(&self, event: RotaryScrollEvent) -> bool {
        let (ahead, _) = self.pose.get().walking_axes();
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
            self.pose
                .update(|pose| pose.turn(turn * TURN_SPEED * seconds, 0.0));
        }
        let (ahead_axis, right_axis) = self.pose.get().walking_axes();
        self.walk((ahead_axis * ahead + right_axis * right) * (WALK_SPEED * seconds / length));
    }

    fn walk(&self, step: Vec3) {
        let from = Spot {
            cell: self.cell.get(),
            at: self.pose.get().position,
        };
        let to = maze::walk(from, step, EYE_RADIUS);
        if to == from {
            return;
        }
        if to.cell != from.cell {
            self.cell.set(to.cell);
        }
        self.pose.update(|pose| pose.position = to.at);
        let moved = Vec3::new(
            (to.cell.x - from.cell.x) as f32 * CELL + to.at.x - from.at.x,
            0.0,
            (to.cell.z - from.cell.z) as f32 * CELL + to.at.z - from.at.z,
        );
        self.walked.update(|walked| *walked += moved.length());
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

    // A press takes the keys back. A finger on the left half of a touch
    // screen of `width` starts walking; any other press starts looking.
    fn press(&self, event: &PointerEvent, width: f32, looking: &mut Option<(PointerId, Point)>) {
        if let Err(error) = self.focus.request_focus() {
            log::warn!("the backrooms could not take focus: {error:?}");
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
            self.pose.update(|pose| {
                pose.turn(
                    (now.x - last.x) * LOOK_PER_PIXEL,
                    (last.y - now.y) * LOOK_PER_PIXEL,
                );
            });
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
