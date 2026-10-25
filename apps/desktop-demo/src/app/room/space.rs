//! The room the Room tab shows: where its walls lie in the scene, and the
//! eye that walks in it.

use std::{cell::Cell, f32::consts::FRAC_PI_3, rc::Rc};

use cranpose_core::{mutableStateOf, remember, MutableState, NodeId};
use cranpose_ui::{
    composable,
    space::{Camera, EyePose, Light, Mat4, Scene3d, Vec3},
    Eye, FocusRequester, KeyCode, KeyEvent, KeyEventType, Point, PointerButton, PointerEventKind,
    PointerInputScope, Size,
};

pub(super) const FIELD_OF_VIEW: f32 = FRAC_PI_3;
pub(super) const TRANSITION_SECONDS: f32 = 0.9;
pub(super) const LONGEST_STEP_SECONDS: f32 = 0.1;
const DEPTH_PER_WIDTH: f32 = 1.6;
const STANDING_DEPTH_SHARE: f32 = 0.2;
const WALK_SPEED: f32 = 900.0;
const TURN_SPEED: f32 = 1.8;
const LOOK_PER_PIXEL: f32 = 0.005;
/// How close the eye comes to a wall it walks into.
const EYE_RADIUS: f32 = 60.0;
const DOWN: Vec3 = Vec3::new(0.0, 1.0, 0.0);

const FORWARD: u8 = 1;
const BACK: u8 = 1 << 1;
const LEFT: u8 = 1 << 2;
const RIGHT: u8 = 1 << 3;
const TURN_LEFT: u8 = 1 << 4;
const TURN_RIGHT: u8 = 1 << 5;

/// A face of the room.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Wall {
    /// The far wall, which the flat tab shows as its screen.
    North,
    East,
    South,
    West,
    Floor,
    Ceiling,
}

impl Wall {
    pub(super) const ALL: [Self; 6] = [
        Self::North,
        Self::East,
        Self::South,
        Self::West,
        Self::Floor,
        Self::Ceiling,
    ];
}

/// A box-shaped room in logical pixels: `width` along x, `height` along y
/// and `depth` along z. The north wall lies at `z = depth`, the south wall
/// at `z = 0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RoomShape {
    pub(super) width: f32,
    pub(super) height: f32,
    pub(super) depth: f32,
    /// The focal length of the camera that walks in it.
    pub(super) focal_length: f32,
}

impl RoomShape {
    /// The room whose north wall fills `viewport`.
    pub(super) fn for_viewport(viewport: Size) -> Self {
        let focal_length = Camera::focal_length_for(viewport.height.max(1.0), FIELD_OF_VIEW);
        let width = viewport.width.max(1.0);
        Self {
            width,
            height: viewport.height.max(1.0),
            depth: (width * DEPTH_PER_WIDTH).max(focal_length * 1.5),
            focal_length,
        }
    }

    pub(super) fn wall_size(&self, wall: Wall) -> Size {
        let (width, height) = match wall {
            Wall::North | Wall::South => (self.width, self.height),
            Wall::East | Wall::West => (self.depth, self.height),
            Wall::Floor | Wall::Ceiling => (self.width, self.depth),
        };
        Size { width, height }
    }

    /// Places the unit plane mesh as `wall`, its content reading the right
    /// way round from inside the room.
    pub(super) fn wall_transform(&self, wall: Wall) -> Mat4 {
        let (w, h, d) = (self.width, self.height, self.depth);
        let (origin, right, down) = match wall {
            Wall::North => (Vec3::new(0.0, 0.0, d), Vec3::new(1.0, 0.0, 0.0), DOWN),
            Wall::South => (Vec3::new(w, 0.0, 0.0), Vec3::new(-1.0, 0.0, 0.0), DOWN),
            Wall::West => (Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), DOWN),
            Wall::East => (Vec3::new(w, 0.0, d), Vec3::new(0.0, 0.0, -1.0), DOWN),
            Wall::Floor => (
                Vec3::new(0.0, h, d),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, -1.0),
            ),
            Wall::Ceiling => (
                Vec3::ZERO,
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            ),
        };
        let size = self.wall_size(wall);
        Mat4::from_axes(
            right * size.width,
            down * size.height,
            right.cross(down),
            origin,
        )
    }

    /// The camera that shows the north wall at its own size, filling the
    /// viewport: the flat screen.
    pub(super) fn screen_camera(&self) -> Camera {
        let eye = self.screen_eye();
        Camera::look_at(eye, eye + Vec3::new(0.0, 0.0, 1.0), DOWN, self.focal_length)
    }

    /// Where the eye stands to show the north wall at its own size, filling
    /// the viewport: the flat screen.
    fn screen_eye(&self) -> Vec3 {
        Vec3::new(
            self.width * 0.5,
            self.height * 0.5,
            self.depth - self.focal_length,
        )
    }

    /// The camera of a room behind a doorway, once stepped into: in the
    /// middle of the room's south side, which the doorway is, so the whole
    /// room lies behind the doorway.
    pub(super) fn doorway_camera(&self) -> Camera {
        let eye = Vec3::new(self.width * 0.5, self.height * 0.5, 0.0);
        Camera::look_at(eye, eye + Vec3::new(0.0, 0.0, 1.0), DOWN, self.focal_length)
    }

    /// Where the eye stands once it steps into the room.
    fn standing_eye(&self) -> Vec3 {
        Vec3::new(
            self.width * 0.5,
            self.height * 0.5,
            self.depth * STANDING_DEPTH_SHARE,
        )
    }
}

/// Whether the room is entered, the eye that walks in it and which movement
/// keys are held.
#[derive(Clone)]
pub(super) struct RoomState {
    entered: MutableState<bool>,
    spatial: MutableState<bool>,
    pub(super) progress: MutableState<f32>,
    /// The window's eye: this room's own when it is the window's scene, the
    /// window scene's when it lies behind a doorway.
    pub(super) eye: Eye,
    /// Whether the room lies behind a doorway of another.
    nested: bool,
    /// The node of the room's scene.
    pub(super) scene_node: Rc<Cell<Option<NodeId>>>,
    /// Where the eye stood before it stepped into a room behind a doorway.
    came_from: MutableState<Option<EyePose>>,
    pub(super) held: MutableState<u8>,
    pub(super) focus: FocusRequester,
    /// Whether the ball of fire flies and the shape shifts.
    pub(super) motion: MutableState<bool>,
}

/// The state of a room: behind a doorway, the room walks `window_eye`, the
/// eye of the window's scene; otherwise an eye of its own.
#[composable]
pub(super) fn rememberRoomState(window_eye: Option<Eye>) -> RoomState {
    remember(move || RoomState {
        entered: mutableStateOf(false),
        spatial: mutableStateOf(false),
        progress: mutableStateOf(0.0),
        nested: window_eye.is_some(),
        eye: window_eye
            .unwrap_or_else(|| Eye::new(EyePose::standing(Vec3::ZERO, 0.0, 0.0), EYE_RADIUS)),
        scene_node: Rc::default(),
        came_from: mutableStateOf(None),
        held: mutableStateOf(0),
        focus: FocusRequester::new(),
        motion: mutableStateOf(true),
    })
    .with(Clone::clone)
}

impl RoomState {
    /// Whether the room shows in space, or is on its way there.
    pub(super) fn is_entered(&self) -> bool {
        self.entered.get()
    }

    /// Whether the tab shows the scene rather than the flat screen.
    pub(super) fn is_spatial(&self) -> bool {
        self.spatial.get()
    }

    /// Whether the room lies behind a doorway of another.
    pub(super) fn is_nested(&self) -> bool {
        self.nested
    }

    /// Whether the window's eye stands in this room behind a doorway, or
    /// further in, through one of its doorways: a room the eye walked into
    /// shows in space, as one stepped into does. Reads the eye's pose, so a
    /// composable reads it through a derived state.
    pub(super) fn holds_eye(&self) -> bool {
        self.nested
            && self
                .scene_node
                .get()
                .is_some_and(|scene| self.eye.pose().doorways.contains(&scene))
    }

    /// Turns the flat screen into the room's north wall and steps into the
    /// room, to where the eye stands in it. The room takes the keyboard, so
    /// the movement keys walk. The eye steps back from the screen into this
    /// room, or, behind a doorway, through the doorway into it.
    pub(super) fn enter(&self, shape: &RoomShape) {
        let standing = EyePose::standing(shape.standing_eye(), 0.0, 0.0);
        if self.nested {
            let before = self.eye.pose();
            let Some(scene) = self.scene_node.get() else {
                return;
            };
            if !self.eye.enter(scene, standing) {
                return;
            }
            self.came_from.set(Some(before));
            self.progress.set(1.0);
        } else {
            self.eye.place(standing);
        }
        self.spatial.set(true);
        self.entered.set(true);
        self.focus.request_focus().ok();
    }

    /// Walks back to the north wall until it fills the screen again: from
    /// where the eye stands in the room, or from where it steps in when it
    /// stands in a room behind one of the walls. Behind a doorway, the eye
    /// goes back to where it stood before it stepped in, while it still
    /// stands in this room.
    pub(super) fn leave(&self, shape: &RoomShape) {
        if self.nested {
            let came_from = self.came_from.get();
            self.came_from.set(None);
            let inside = self.eye.pose().doorways.last().copied() == self.scene_node.get();
            if let Some(pose) = came_from.filter(|_| inside) {
                self.eye.place(pose);
            }
        } else if !self.eye.pose().doorways.is_empty() {
            self.eye
                .place(EyePose::standing(shape.standing_eye(), 0.0, 0.0));
        }
        self.entered.set(false);
        self.held.set(0);
    }

    /// Back to the flat screen, once the walk back has ended.
    pub(super) fn flatten(&self) {
        self.spatial.set(false);
    }

    /// The scene: in the room, through the eye; on the way between the
    /// screen and the room, through a camera part of the way from the screen
    /// to the eye. Behind a doorway of another room, the scene's camera
    /// stands in the doorway: the screen's camera for the flat screen, and
    /// once stepped into, the camera in the room's open south side, so the
    /// whole room lies behind the doorway; the window's eye walks in.
    pub(super) fn scene(&self, shape: &RoomShape) -> Scene3d {
        let light = Some(Light {
            direction: Vec3::new(0.5, 0.45, 0.75),
            ambient: 0.45,
            shadows: true,
            size: 0.08,
        });
        if self.nested {
            let camera = if self.spatial.get() || self.holds_eye() {
                shape.doorway_camera()
            } else {
                shape.screen_camera()
            };
            return Scene3d {
                camera,
                light,
                eye: None,
            };
        }
        let t = self.progress.get();
        if t >= 1.0 {
            return self.eye.scene(shape.focal_length, light);
        }
        let pose = self.eye.pose();
        let t = t * t * (3.0 - 2.0 * t);
        let at = shape.screen_eye().lerp(pose.position, t);
        let ahead = Vec3::new(0.0, 0.0, 1.0);
        let forward = ahead.lerp(pose.forward, t).normalized().unwrap_or(ahead);
        let down = DOWN.lerp(pose.down, t).normalized().unwrap_or(DOWN);
        Scene3d {
            camera: Camera::look_at(at, at + forward, down, shape.focal_length),
            light,
            eye: None,
        }
    }

    /// The camera of the window's eye in this room's coordinates, which a
    /// plane faces to face the eye: the scene's own camera while the eye
    /// stands in this room's space, or on its way into it from the screen;
    /// from a space a doorway away, the eye's pose there.
    pub(super) fn eye_camera(&self, shape: &RoomShape) -> Camera {
        let own = self.scene(shape).camera;
        if !self.nested && self.eye.pose().doorways.is_empty() {
            return own;
        }
        self.scene_node
            .get()
            .and_then(|scene| self.eye.pose_in(scene))
            .map_or(own, |pose| pose.camera(shape.focal_length))
    }

    /// Takes a movement key while the room is entered.
    pub(super) fn key(&self, event: &KeyEvent) -> bool {
        if !self.entered.get() {
            return false;
        }
        let pressed = event.event_type == KeyEventType::KeyDown;
        let bit = match event.key_code {
            KeyCode::Escape => {
                if pressed {
                    self.focus.request_focus().ok();
                }
                return true;
            }
            KeyCode::W | KeyCode::ArrowUp => FORWARD,
            KeyCode::S | KeyCode::ArrowDown => BACK,
            KeyCode::A => LEFT,
            KeyCode::D => RIGHT,
            KeyCode::Q | KeyCode::ArrowLeft => TURN_LEFT,
            KeyCode::E | KeyCode::ArrowRight => TURN_RIGHT,
            _ => return false,
        };
        self.held.update(|held| {
            if pressed {
                *held |= bit;
            } else {
                *held &= !bit;
            }
        });
        true
    }

    /// Moves the eye by the held keys over `seconds`; false when no key is
    /// held. The walls stop it, but for a doorway in one, which leads into
    /// the room behind it.
    pub(super) fn walk(&self, seconds: f32) -> bool {
        let held = self.held.get();
        if held == 0 {
            return false;
        }
        let axis = |positive: u8, negative: u8| {
            f32::from(u8::from(held & positive != 0)) - f32::from(u8::from(held & negative != 0))
        };
        self.eye
            .turn(axis(TURN_RIGHT, TURN_LEFT) * TURN_SPEED * seconds, 0.0);
        let (ahead, right) = self.eye.pose().walking_axes();
        self.eye.walk(
            (ahead * axis(FORWARD, BACK) + right * axis(RIGHT, LEFT)) * (WALK_SPEED * seconds),
        );
        true
    }

    /// Turns the camera while the right button drags, as a game's mouse look
    /// does: a drag to the right looks right, a drag up looks up. The left
    /// button stays with the walls' content, which it clicks, scrolls and
    /// selects. A finger has no right button, so its drag over nothing that
    /// takes it turns the camera too.
    pub(super) async fn look(&self, scope: PointerInputScope) {
        scope
            .await_pointer_event_scope(|events| async move {
                let mut last: Option<Point> = None;
                loop {
                    let event = events.await_pointer_event().await;
                    let finger = event.source.is_touch_like();
                    match event.kind {
                        PointerEventKind::ButtonDown(PointerButton::Secondary) => {
                            last = self.entered.get().then_some(event.global_position);
                        }
                        PointerEventKind::Down if finger && !event.is_consumed() => {
                            last = self.entered.get().then_some(event.global_position);
                        }
                        PointerEventKind::Move => {
                            let Some(previous) = last.filter(|_| !event.is_consumed()) else {
                                continue;
                            };
                            let now = event.global_position;
                            self.eye.turn(
                                (now.x - previous.x) * LOOK_PER_PIXEL,
                                (previous.y - now.y) * LOOK_PER_PIXEL,
                            );
                            last = Some(now);
                        }
                        PointerEventKind::ButtonUp(PointerButton::Secondary)
                        | PointerEventKind::Cancel => last = None,
                        PointerEventKind::Up if finger => last = None,
                        _ => {}
                    }
                }
            })
            .await;
    }
}
