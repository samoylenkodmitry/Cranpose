//! The room the Room tab shows: where its walls lie in the scene, and the
//! camera that walks in it.

use std::f32::consts::{FRAC_PI_3, PI, TAU};

use cranpose_core::{mutableStateOf, remember, MutableState};
use cranpose_ui::{
    composable,
    space::{Camera, Light, Mat4, Scene3d, Vec3},
    FocusRequester, KeyCode, KeyEvent, KeyEventType, Point, PointerButton, PointerEventKind,
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
const PITCH_LIMIT: f32 = 1.2;
const WALL_MARGIN: f32 = 80.0;
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
    pub(super) const ALL: [Wall; 6] = [
        Wall::North,
        Wall::East,
        Wall::South,
        Wall::West,
        Wall::Floor,
        Wall::Ceiling,
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

    /// Where the eye stands to show the north wall at its own size, filling
    /// the viewport: the flat screen.
    fn screen_eye(&self) -> Vec3 {
        Vec3::new(
            self.width * 0.5,
            self.height * 0.5,
            self.depth - self.focal_length,
        )
    }

    /// Where the eye stands once it steps into the room.
    fn standing_eye(&self) -> Vec3 {
        Vec3::new(
            self.width * 0.5,
            self.height * 0.5,
            self.depth * STANDING_DEPTH_SHARE,
        )
    }

    /// `position` moved inside the room, at least `margin` from each wall.
    fn keep_inside(&self, position: Vec3, margin: f32) -> Vec3 {
        let clamp = |value: f32, extent: f32| {
            let margin = margin.min(extent * 0.5);
            value.clamp(margin, extent - margin)
        };
        Vec3::new(
            clamp(position.x, self.width),
            position.y,
            clamp(position.z, self.depth),
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Pose {
    position: Option<Vec3>,
    yaw: f32,
    pitch: f32,
}

/// Whether the room is entered, where its camera stands and which movement
/// keys are held.
#[derive(Clone)]
pub(super) struct RoomState {
    entered: MutableState<bool>,
    spatial: MutableState<bool>,
    pub(super) progress: MutableState<f32>,
    pose: MutableState<Pose>,
    pub(super) held: MutableState<u8>,
    pub(super) focus: FocusRequester,
    /// Whether the ball of fire flies and the shape shifts.
    pub(super) motion: MutableState<bool>,
}

#[composable]
pub(super) fn rememberRoomState() -> RoomState {
    remember(|| RoomState {
        entered: mutableStateOf(false),
        spatial: mutableStateOf(false),
        progress: mutableStateOf(0.0),
        pose: mutableStateOf(Pose::default()),
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

    /// Turns the flat screen into the room's north wall and steps back into
    /// the room. The room takes the keyboard, so the movement keys walk.
    pub(super) fn enter(&self) {
        self.spatial.set(true);
        self.entered.set(true);
        self.focus.request_focus().ok();
    }

    /// Walks back to the north wall until it fills the screen again.
    pub(super) fn leave(&self) {
        self.entered.set(false);
        self.held.set(0);
    }

    /// Back to the flat screen, once the walk back has ended.
    pub(super) fn flatten(&self) {
        self.spatial.set(false);
    }

    /// The scene through the camera, part of the way from the screen to
    /// where it stands.
    pub(super) fn scene(&self, shape: &RoomShape) -> Scene3d {
        let pose = self.pose.get();
        let standing = shape.keep_inside(
            pose.position.unwrap_or_else(|| shape.standing_eye()),
            WALL_MARGIN,
        );
        let t = self.progress.get();
        let t = t * t * (3.0 - 2.0 * t);
        let eye = shape.screen_eye().lerp(standing, t);
        let yaw = ((pose.yaw + PI).rem_euclid(TAU) - PI) * t;
        let pitch = pose.pitch * t;
        Scene3d {
            camera: Camera::look_at(eye, eye + facing(yaw, pitch), DOWN, shape.focal_length),
            light: Some(Light {
                direction: Vec3::new(0.5, 0.45, 0.75),
                ambient: 0.45,
                shadows: true,
                size: 0.08,
            }),
        }
    }

    fn turn(&self, yaw: f32, pitch: f32) {
        self.pose.update(|pose| {
            pose.yaw += yaw;
            pose.pitch = (pose.pitch + pitch).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        });
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

    /// Moves the camera by the held keys over `seconds`; false when no key
    /// is held.
    pub(super) fn walk(&self, shape: &RoomShape, seconds: f32) -> bool {
        let held = self.held.get();
        if held == 0 {
            return false;
        }
        let axis = |positive: u8, negative: u8| {
            f32::from(u8::from(held & positive != 0)) - f32::from(u8::from(held & negative != 0))
        };
        self.turn(axis(TURN_RIGHT, TURN_LEFT) * TURN_SPEED * seconds, 0.0);
        let pose = self.pose.get();
        let ahead = facing(pose.yaw, 0.0);
        let right = DOWN.cross(ahead);
        let position = pose.position.unwrap_or_else(|| shape.standing_eye());
        let step =
            (ahead * axis(FORWARD, BACK) + right * axis(RIGHT, LEFT)) * (WALK_SPEED * seconds);
        let position = shape.keep_inside(position + step, WALL_MARGIN);
        self.pose.update(|pose| pose.position = Some(position));
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
                            self.turn(
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

/// The direction of a camera turned `yaw` to the right and `pitch` up.
fn facing(yaw: f32, pitch: f32) -> Vec3 {
    let (sin_yaw, cos_yaw) = yaw.sin_cos();
    let (sin_pitch, cos_pitch) = pitch.sin_cos();
    Vec3::new(sin_yaw * cos_pitch, -sin_pitch, cos_yaw * cos_pitch)
}
