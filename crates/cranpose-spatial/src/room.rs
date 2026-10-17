use std::f32::consts::FRAC_PI_3;

use cranpose_core::{LaunchedEffectAsync, MutableState, key, mutableStateOf, remember};
use cranpose_macros::composable;
use cranpose_ui::{
    Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, FocusRequester, GraphicsLayer,
    KeyCode, KeyEvent, KeyEventType, Modifier, Point, PointerEventKind, PointerInputScope, Size,
};

use crate::{Camera, RoomShape, Vec3, Wall, focal_length_for};

const FIELD_OF_VIEW: f32 = FRAC_PI_3;
const TRANSITION_SECONDS: f32 = 0.9;
const WALK_SPEED: f32 = 900.0;
const TURN_SPEED: f32 = 1.8;
const LOOK_PER_PIXEL: f32 = 0.005;
const PITCH_LIMIT: f32 = 1.2;
const WALL_MARGIN: f32 = 80.0;
const LONGEST_STEP_SECONDS: f32 = 0.1;

const FORWARD: u8 = 1;
const BACK: u8 = 1 << 1;
const LEFT: u8 = 1 << 2;
const RIGHT: u8 = 1 << 3;
const TURN_LEFT: u8 = 1 << 4;
const TURN_RIGHT: u8 = 1 << 5;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Pose {
    position: Option<Vec3>,
    yaw: f32,
    pitch: f32,
}

/// What a [`Room`] remembers: whether it is entered, where its camera stands
/// and which movement keys are held.
#[derive(Clone)]
pub struct RoomState {
    entered: MutableState<bool>,
    spatial: MutableState<bool>,
    progress: MutableState<f32>,
    pose: MutableState<Pose>,
    held: MutableState<u8>,
    focus: FocusRequester,
}

impl RoomState {
    /// Whether the room shows in space, or is on its way there.
    pub fn is_entered(&self) -> bool {
        self.entered.get()
    }

    /// Turns the flat screen into the room's north wall and steps back into
    /// the room. The room takes the keyboard, so the movement keys walk.
    pub fn enter(&self) {
        self.spatial.set(true);
        self.entered.set(true);
        self.focus.request_focus().ok();
    }

    /// Walks back to the north wall until it fills the screen again.
    pub fn leave(&self) {
        self.entered.set(false);
        self.held.set(0);
    }

    fn camera(&self, shape: &RoomShape, focal_length: f32) -> Camera {
        let pose = self.pose.get();
        let standing = shape.standing_camera(focal_length);
        let standing = Camera {
            position: shape.keep_inside(pose.position.unwrap_or(standing.position), WALL_MARGIN),
            yaw: pose.yaw,
            pitch: pose.pitch,
            ..standing
        };
        let t = self.progress.get();
        shape
            .screen_camera(focal_length)
            .lerp(&standing, t * t * (3.0 - 2.0 * t))
    }

    fn turn(&self, yaw: f32, pitch: f32) {
        self.pose.update(|pose| {
            pose.yaw += yaw;
            pose.pitch = (pose.pitch + pitch).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        });
    }

    fn key(&self, event: &KeyEvent) -> bool {
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

    fn walk(&self, shape: &RoomShape, focal_length: f32, seconds: f32) -> bool {
        let held = self.held.get();
        if held == 0 {
            return false;
        }
        let axis = |positive: u8, negative: u8| {
            f32::from(u8::from(held & positive != 0)) - f32::from(u8::from(held & negative != 0))
        };
        self.turn(axis(TURN_RIGHT, TURN_LEFT) * TURN_SPEED * seconds, 0.0);
        let camera = self.camera(shape, focal_length);
        let (sin_yaw, cos_yaw) = camera.yaw.sin_cos();
        let ahead = Vec3::new(sin_yaw, 0.0, cos_yaw);
        let step = (ahead * axis(FORWARD, BACK) + camera.right() * axis(RIGHT, LEFT))
            * (WALK_SPEED * seconds);
        let position = shape.keep_inside(camera.position + step, WALL_MARGIN);
        self.pose.update(|pose| pose.position = Some(position));
        true
    }

    async fn look(&self, scope: PointerInputScope) {
        scope
            .await_pointer_event_scope(|events| async move {
                let mut last: Option<Point> = None;
                loop {
                    let event = events.await_pointer_event().await;
                    match event.kind {
                        PointerEventKind::Down => {
                            last = (!event.is_consumed() && self.entered.get())
                                .then_some(event.global_position);
                        }
                        PointerEventKind::Move => {
                            let Some(previous) = last.filter(|_| !event.is_consumed()) else {
                                last = None;
                                continue;
                            };
                            let now = event.global_position;
                            self.turn(
                                (previous.x - now.x) * LOOK_PER_PIXEL,
                                (now.y - previous.y) * LOOK_PER_PIXEL,
                            );
                            last = Some(now);
                        }
                        PointerEventKind::Up | PointerEventKind::Cancel => last = None,
                        _ => {}
                    }
                }
            })
            .await;
    }
}

/// Remembers a [`RoomState`] that starts as the flat screen.
#[composable]
pub fn rememberRoomState() -> RoomState {
    remember(|| RoomState {
        entered: mutableStateOf(false),
        spatial: mutableStateOf(false),
        progress: mutableStateOf(0.0),
        pose: mutableStateOf(Pose::default()),
        held: mutableStateOf(0),
        focus: FocusRequester::new(),
    })
    .with(Clone::clone)
}

/// A box-shaped room seen through a camera, filling its bounds.
///
/// Flat, the north wall's content fills the bounds as an ordinary screen.
/// Entered, the camera steps back into the room and every wall shows
/// `wall`'s content for it in perspective, live and taking input where it
/// lies on screen. W, A, S and D or the up and down arrows walk; Q and E or
/// the left and right arrows turn the camera, and a drag turns it as if the
/// room were held and pulled along. Escape takes the keys back from a wall's
/// focused control.
#[composable(no_skip)]
pub fn Room<F>(state: RoomState, modifier: Modifier, wall: F)
where
    F: Fn(Wall) + Clone + 'static,
{
    let keys = state.clone();
    let look = state.clone();
    BoxWithConstraints(
        modifier
            .focus_requester(&state.focus)
            .focus_target()
            .on_key_event(move |event| keys.key(event))
            .pointer_input((), move |scope| {
                let look = look.clone();
                async move { look.look(scope).await }
            }),
        move |scope| {
            let constraints = scope.constraints();
            let finite = |extent: f32| if extent.is_finite() { extent } else { 1.0 };
            let viewport = Size {
                width: finite(constraints.max_width),
                height: finite(constraints.max_height),
            };
            let focal_length = focal_length_for(viewport.height, FIELD_OF_VIEW);
            let shape = RoomShape::for_viewport(viewport, focal_length);
            RoomMotion(state.clone(), shape, focal_length);
            let walls = if state.spatial.get() {
                &Wall::ALL[..]
            } else {
                &Wall::ALL[..1]
            };
            for &side in walls {
                let state = state.clone();
                let wall = wall.clone();
                key(side, move || {
                    WallSurface(state, (shape, focal_length, viewport), side, wall);
                });
            }
        },
    );
}

#[composable(no_skip)]
fn RoomMotion(state: RoomState, shape: RoomShape, focal_length: f32) {
    let entered = state.entered.get();
    let mover = state.clone();
    LaunchedEffectAsync(entered, move |scope| {
        Box::pin(async move {
            let target = if entered { 1.0 } else { 0.0 };
            let clock = scope.runtime().frame_clock();
            let mut last = clock.next_frame().await;
            while scope.is_active() && mover.progress.get() != target {
                let now = clock.next_frame().await;
                let step = seconds_between(last, now) / TRANSITION_SECONDS;
                last = now;
                mover.progress.update(|progress| {
                    *progress = if target > *progress {
                        (*progress + step).min(target)
                    } else {
                        (*progress - step).max(target)
                    };
                });
            }
            if scope.is_active() && !entered {
                mover.spatial.set(false);
            }
        })
    });
    let moving = state.held.get() != 0;
    let walker = state;
    LaunchedEffectAsync((moving, shape), move |scope| {
        Box::pin(async move {
            if !moving {
                return;
            }
            let clock = scope.runtime().frame_clock();
            let mut last = clock.next_frame().await;
            while scope.is_active() {
                let now = clock.next_frame().await;
                let seconds = seconds_between(last, now).min(LONGEST_STEP_SECONDS);
                last = now;
                if !walker.walk(&shape, focal_length, seconds) {
                    break;
                }
            }
        })
    });
}

#[composable(no_skip)]
fn WallSurface<F>(state: RoomState, room: (RoomShape, f32, Size), side: Wall, wall: F)
where
    F: Fn(Wall) + 'static,
{
    let (shape, focal_length, viewport) = room;
    let plane = shape.plane(side);
    Box(
        Modifier::empty()
            .required_size(shape.wall_size(side))
            .graphics_layer(move || {
                if side == Wall::North && state.progress.get() == 0.0 {
                    return GraphicsLayer::DEFAULT;
                }
                GraphicsLayer {
                    projection: Some(
                        state
                            .camera(&shape, focal_length)
                            .projection(&plane, viewport),
                    ),
                    clip: true,
                    ..GraphicsLayer::DEFAULT
                }
            }),
        BoxSpec::default(),
        move || wall(side),
    );
}

fn seconds_between(earlier_nanos: u64, later_nanos: u64) -> f32 {
    later_nanos.saturating_sub(earlier_nanos) as f32 * 1.0e-9
}
