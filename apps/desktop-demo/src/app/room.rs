//! A tab that is an ordinary screen until it steps back into a room: the
//! screen becomes the far wall, other tabs hang on the side walls, and a
//! globe and an imported model stand in the room, each a composable drawn on
//! a mesh.

mod space;

use std::sync::Arc;

use cranpose::liquid::prelude::{LiquidCheckbox, LiquidTheme, LiquidThemeSpec, SchemeMode};
use cranpose_core::{key, remember, rememberMutableStateOf, LaunchedEffectAsync, MutableState};
use cranpose_ui::{
    composable,
    space::{Mat4, Mesh, MeshSurface, Vec3},
    text::TextUnit,
    Alignment, Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Brush, Button,
    ButtonSpec, Color, Column, ColumnSpec, Modifier, Rect, Row, RowSpec, Size, Text, TextStyle,
};

use self::space::{
    rememberRoomState, RoomShape, RoomState, Wall, LONGEST_STEP_SECONDS, TRANSITION_SECONDS,
};
use super::{mineswapper2::mineswapper2_tab, text_showcase::TextShowcaseTab};

const TILE: f32 = 160.0;
const TORUS_OBJ: &str = include_str!("../../assets/torus.obj");
const GLOBE_CONTENT: Size = Size {
    width: 1024.0,
    height: 512.0,
};
/// The torus's content, as long around the ring as the ring is around its
/// tube's middle against the tube's own girth (radius 1 against 0.4), so
/// its check boxes keep their shape.
const TORUS_CONTENT: Size = Size {
    width: TORUS_COLUMNS as f32 * TORUS_CELL,
    height: TORUS_ROWS as f32 * TORUS_CELL,
};
const TORUS_COLUMNS: usize = 20;
const TORUS_ROWS: usize = 8;
const TORUS_CELL: f32 = 32.0;
const SPIN_PER_SECOND: f32 = 0.35;

fn text_style(size: f32, color: Color) -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.font_size = TextUnit::Sp(size);
    style.span_style.color = Some(color);
    style
}

/// The meshes the room's surfaces are drawn on, made once.
#[derive(Clone)]
struct RoomMeshes {
    plane: Arc<Mesh>,
    globe: Arc<Mesh>,
    torus: Option<Arc<Mesh>>,
}

#[composable]
pub fn RoomTab() {
    let room = rememberRoomState();
    let meshes = remember(|| RoomMeshes {
        plane: Arc::new(Mesh::plane()),
        globe: Arc::new(Mesh::sphere(48, 24)),
        torus: Mesh::from_obj(TORUS_OBJ).ok().map(Arc::new),
    })
    .with(Clone::clone);
    let keys = room.clone();
    let look = room.clone();
    BoxWithConstraints(
        Modifier::empty()
            .fill_max_size()
            .background(Color(0.04, 0.04, 0.06, 1.0))
            .focus_requester(&room.focus)
            .focus_target()
            .on_key_event(move |event| keys.key(event))
            .pointer_input((), move |scope| {
                let look = look.clone();
                async move { look.look(scope).await }
            }),
        move |scope| {
            let constraints = scope.constraints();
            let finite = |extent: f32| if extent.is_finite() { extent } else { 1.0 };
            let shape = RoomShape::for_viewport(Size {
                width: finite(constraints.max_width),
                height: finite(constraints.max_height),
            });
            RoomMotion(room.clone(), shape);
            Scene(room.clone(), shape, meshes.clone());
        },
    );
}

/// The room's scene: flat, the north wall alone as the screen; in space,
/// every wall and the objects in the room, through the camera.
#[composable(no_skip)]
fn Scene(room: RoomState, shape: RoomShape, meshes: RoomMeshes) {
    let spatial = room.is_spatial();
    let camera = room.clone();
    Box(
        Modifier::empty()
            .fill_max_size()
            .scene3d(move || camera.scene(&shape)),
        BoxSpec::default(),
        move || {
            let walls = if spatial {
                &Wall::ALL[..]
            } else {
                &Wall::ALL[..1]
            };
            for &wall in walls {
                let room = room.clone();
                let plane = Arc::clone(&meshes.plane);
                key(wall, move || {
                    WallSurface(room, shape, wall, plane);
                });
            }
            let globe = Arc::clone(&meshes.globe);
            key("globe", move || Globe(shape, globe));
            if let Some(torus) = meshes.torus.clone().filter(|_| spatial) {
                key("torus", move || Torus(shape, torus));
            }
        },
    );
}

#[composable(no_skip)]
fn RoomMotion(room: RoomState, shape: RoomShape) {
    let entered = room.is_entered();
    let mover = room.clone();
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
                mover.flatten();
            }
        })
    });
    let moving = room.held.get() != 0;
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
                if !room.walk(&shape, seconds) {
                    break;
                }
            }
        })
    });
}

#[composable(no_skip)]
fn WallSurface(room: RoomState, shape: RoomShape, wall: Wall, plane: Arc<Mesh>) {
    Box(
        Modifier::empty()
            .required_size(shape.wall_size(wall))
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&plane),
                transform: shape.wall_transform(wall),
                lit: false,
                casts_shadow: false,
            }),
        BoxSpec::default(),
        move || match wall {
            Wall::North => Controls(room.clone()),
            Wall::East => Panel(Color(0.10, 0.11, 0.15, 1.0), mineswapper2_tab),
            Wall::West => Panel(Color(0.12, 0.10, 0.12, 1.0), TextShowcaseTab),
            Wall::South => BackWall(),
            Wall::Floor => Floor(),
            Wall::Ceiling => Ceiling(),
        },
    );
}

/// A spinning globe whose surface is a live composable. It stands in front
/// of the far wall, so the flat screen shows it over the controls, with its
/// shadow on them.
#[composable(no_skip)]
fn Globe(shape: RoomShape, mesh: Arc<Mesh>) {
    let spin = rememberMutableStateOf(|| 0.0f32);
    let spinning = rememberMutableStateOf(|| true);
    Spin(spin, spinning);
    let radius = shape.width.min(shape.height) * 0.14;
    let center = Vec3::new(
        shape.width * 0.68,
        shape.height * 0.58,
        shape.depth - radius * 1.4,
    );
    Box(
        Modifier::empty()
            .required_size(GLOBE_CONTENT)
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&mesh),
                transform: Mat4::scaling(Vec3::new(radius, radius, radius))
                    .then(&Mat4::rotation_y(spin.get()))
                    .then(&Mat4::translation(center)),
                lit: true,
                casts_shadow: true,
            }),
        BoxSpec::default(),
        move || GlobeFace(spinning),
    );
}

#[composable(no_skip)]
fn Spin(spin: MutableState<f32>, spinning: MutableState<bool>) {
    let on = spinning.get();
    LaunchedEffectAsync(on, move |scope| {
        Box::pin(async move {
            if !on {
                return;
            }
            let clock = scope.runtime().frame_clock();
            let mut last = clock.next_frame().await;
            while scope.is_active() {
                let now = clock.next_frame().await;
                let seconds = seconds_between(last, now).min(LONGEST_STEP_SECONDS);
                last = now;
                spin.update(|angle| {
                    *angle = (*angle + seconds * SPIN_PER_SECOND) % std::f32::consts::TAU;
                });
            }
        })
    });
}

#[composable(no_skip)]
fn GlobeFace(spinning: MutableState<bool>) {
    Box(
        Modifier::empty().fill_max_size().draw_behind(|scope| {
            let size = scope.size();
            scope.draw_rect(Brush::solid(Color(0.10, 0.32, 0.62, 1.0)));
            for band in 0..8 {
                scope.draw_rect_at(
                    Rect {
                        x: size.width * band as f32 / 8.0,
                        y: 0.0,
                        width: size.width / 16.0,
                        height: size.height,
                    },
                    Brush::solid(Color(0.14, 0.50, 0.36, 1.0)),
                );
            }
        }),
        BoxSpec {
            content_alignment: Alignment::CENTER,
            ..BoxSpec::default()
        },
        move || {
            Column(
                Modifier::empty()
                    .rounded_corners(16.0)
                    .background(Color(0.05, 0.07, 0.12, 0.85))
                    .padding(18.0),
                ColumnSpec::default(),
                move || {
                    Text(
                        "A sphere mesh",
                        Modifier::empty().padding(4.0),
                        text_style(30.0, Color::WHITE),
                    );
                    Button(
                        Modifier::empty()
                            .padding(4.0)
                            .rounded_corners(10.0)
                            .background(Color(0.25, 0.45, 0.95, 1.0))
                            .padding(12.0),
                        ButtonSpec::default(),
                        move || spinning.set(!spinning.get()),
                        move || {
                            Text(
                                if spinning.get() {
                                    "Stop the spin"
                                } else {
                                    "Spin the globe"
                                },
                                Modifier::empty(),
                                text_style(18.0, Color::WHITE),
                            );
                        },
                    );
                },
            );
        },
    );
}

/// A torus read from an OBJ file, wrapped in a striped composable.
#[composable(no_skip)]
fn Torus(shape: RoomShape, mesh: Arc<Mesh>) {
    let scale = shape.width.min(shape.height) * 0.14;
    let center = Vec3::new(shape.width * 0.3, shape.height * 0.6, shape.depth * 0.62);
    // OBJ files point y up; the scene points it down.
    let transform = Mat4::scaling(Vec3::new(scale, -scale, scale))
        .then(&Mat4::rotation_x(1.1))
        .then(&Mat4::translation(center));
    Box(
        Modifier::empty()
            .required_size(TORUS_CONTENT)
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&mesh),
                transform,
                lit: true,
                casts_shadow: true,
            })
            .background(Color(0.10, 0.07, 0.20, 1.0)),
        BoxSpec::default(),
        || TorusChecks(),
    );
}

/// A grid of check boxes laid around the torus: each row runs once around
/// the ring, and the rows go around the tube.
#[composable]
fn TorusChecks() {
    let checked = rememberMutableStateOf(|| {
        let mut rows = [0u32; TORUS_ROWS];
        for (row, bits) in rows.iter_mut().enumerate() {
            for column in (row % 3..TORUS_COLUMNS).step_by(3) {
                *bits |= 1 << column;
            }
        }
        rows
    });
    LiquidTheme(
        LiquidThemeSpec {
            scheme: SchemeMode::Dark,
            accent: Color(1.0, 0.45, 0.85, 1.0),
            ..LiquidThemeSpec::default()
        },
        move || {
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                for row in 0..TORUS_ROWS {
                    Row(Modifier::empty(), RowSpec::default(), move || {
                        for column in 0..TORUS_COLUMNS {
                            let bit = 1u32 << column;
                            Box(
                                Modifier::empty().size(Size {
                                    width: TORUS_CELL,
                                    height: TORUS_CELL,
                                }),
                                BoxSpec {
                                    content_alignment: Alignment::CENTER,
                                    ..BoxSpec::default()
                                },
                                move || {
                                    LiquidCheckbox(
                                        Modifier::empty(),
                                        checked.get()[row] & bit != 0,
                                        move |on| {
                                            checked.update(|rows| {
                                                rows[row] = if on {
                                                    rows[row] | bit
                                                } else {
                                                    rows[row] & !bit
                                                };
                                            });
                                        },
                                    );
                                },
                            );
                        }
                    });
                }
            });
        },
    );
}

#[composable(no_skip)]
fn Controls(room: RoomState) {
    let presses = rememberMutableStateOf(|| 0u32);
    let entered = room.is_entered();
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(Color(0.09, 0.10, 0.14, 1.0))
            .padding(40.0),
        ColumnSpec::default(),
        move || {
            Text(
                "A room of screens",
                Modifier::empty().padding(6.0),
                text_style(30.0, Color(0.95, 0.95, 1.0, 1.0)),
            );
            Text(
                "This screen is the far wall of a room, seen from one focal length away, so it \
                 shows as a flat screen. The globe stands in front of it and casts its shadow \
                 on it. Step into the room: the other demo tabs hang on the walls around you, \
                 a torus from an OBJ file stands in the middle, covered in check boxes, and \
                 all of them stay live.",
                Modifier::empty().padding(6.0),
                text_style(16.0, Color(0.75, 0.77, 0.86, 1.0)),
            );
            Text(
                "W A S D or the up and down arrows walk. Q and E, the left and right arrows \
                 or a drag turn. Escape takes the keys back from a focused control.",
                Modifier::empty().padding(6.0),
                text_style(14.0, Color(0.60, 0.63, 0.74, 1.0)),
            );
            let room = room.clone();
            Row(
                Modifier::empty().padding(6.0),
                RowSpec::default(),
                move || {
                    let room = room.clone();
                    Button(
                        Modifier::empty()
                            .rounded_corners(10.0)
                            .background(Color(0.25, 0.45, 0.95, 1.0))
                            .padding(14.0),
                        ButtonSpec::default(),
                        move || {
                            if room.is_entered() {
                                room.leave();
                            } else {
                                room.enter();
                            }
                        },
                        move || {
                            Text(
                                if entered {
                                    "Back to the screen"
                                } else {
                                    "Step into the room"
                                },
                                Modifier::empty(),
                                text_style(16.0, Color::WHITE),
                            );
                        },
                    );
                    Button(
                        Modifier::empty()
                            .padding(6.0)
                            .rounded_corners(10.0)
                            .background(Color(0.20, 0.22, 0.30, 1.0))
                            .padding(14.0),
                        ButtonSpec::default(),
                        move || presses.set(presses.get() + 1),
                        move || {
                            Text(
                                format!("Pressed {} times", presses.get()),
                                Modifier::empty(),
                                text_style(16.0, Color::WHITE),
                            );
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn Panel(color: Color, content: fn()) {
    Box(
        Modifier::empty()
            .fill_max_size()
            .background(color)
            .padding(32.0),
        BoxSpec::default(),
        content,
    );
}

#[composable]
fn BackWall() {
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(Color(0.13, 0.12, 0.10, 1.0))
            .padding(48.0),
        ColumnSpec::default(),
        || {
            Text(
                "Behind you",
                Modifier::empty().padding(6.0),
                text_style(30.0, Color(0.96, 0.92, 0.84, 1.0)),
            );
            Text(
                "The screen you came from is the far wall. Turn around with Q or E.",
                Modifier::empty().padding(6.0),
                text_style(16.0, Color(0.78, 0.74, 0.66, 1.0)),
            );
        },
    );
}

#[composable]
fn Floor() {
    Box(
        Modifier::empty().fill_max_size().draw_behind(|scope| {
            let size = scope.size();
            scope.draw_rect(Brush::solid(Color(0.16, 0.13, 0.11, 1.0)));
            let columns = (size.width / TILE).ceil() as u32;
            let rows = (size.height / TILE).ceil() as u32;
            for row in 0..rows {
                for column in (row % 2..columns).step_by(2) {
                    scope.draw_rect_at(
                        Rect {
                            x: column as f32 * TILE,
                            y: row as f32 * TILE,
                            width: TILE,
                            height: TILE,
                        },
                        Brush::solid(Color(0.22, 0.18, 0.15, 1.0)),
                    );
                }
            }
        }),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn Ceiling() {
    Box(
        Modifier::empty().fill_max_size().draw_behind(|scope| {
            let size = scope.size();
            scope.draw_rect(Brush::solid(Color(0.20, 0.20, 0.22, 1.0)));
            let lamp = Rect {
                x: size.width * 0.5 - 120.0,
                y: 0.0,
                width: 240.0,
                height: 80.0,
            };
            for share in [0.25, 0.55, 0.85] {
                scope.draw_rect_at(
                    Rect {
                        y: size.height * share - lamp.height * 0.5,
                        ..lamp
                    },
                    Brush::solid(Color(0.95, 0.93, 0.85, 1.0)),
                );
            }
        }),
        BoxSpec::default(),
        || {},
    );
}

fn seconds_between(earlier_nanos: u64, later_nanos: u64) -> f32 {
    later_nanos.saturating_sub(earlier_nanos) as f32 * 1.0e-9
}
