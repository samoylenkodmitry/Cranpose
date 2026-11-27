//! The 3D content of a `SpatialTray`: its base and the bodies of its controls, behind its panel.
//!
//! The volume's box is centered on the panel's plane, so the panel's plane is `z = 0` in the
//! content's axes, which point right, down and away from the viewer. Every mesh stays behind that
//! plane by at least half a millimeter, also while it animates, so the panel's words show over the
//! parts where the compositor tests the panel against the scene's depth. The base's front face lies
//! `BASE_FRONT` behind the plane. A key's face rests `FACE` behind it and sinks `PRESS_DEPTH`
//! farther while pressed. The meshes read the animations while the scene draws, so an animation
//! draws again and composes nothing.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use cranpose::{
    Box, BoxSpec, Color, Modifier, Size, composable,
    space::{Light, Mat4, Material, Mesh, MeshSurface, Vec3},
};
use cranpose_animation::{Easing, animateFloatAsState, spring, tween};
use cranpose_core::{State, key, remember};

use crate::{
    tray_material::{Finish, satin},
    tray_meshes::slab,
    tray_part::{Part, PartShape, TrayParts},
};

// The base: its front face behind the panel's plane, its depth and the radii of its corners and
// of its front edge, in dp
const BASE_FRONT: f32 = 10.0;
const BASE_DEPTH: f32 = 10.0;
const BASE_CORNER: f32 = 24.0;
const BASE_EDGE: f32 = 4.0;
// A face at rest behind the panel's plane, and how far a press and a hover move it
const FACE: f32 = 1.5;
// How far a pressed key or ball sinks, in dp
pub(crate) const PRESS_DEPTH: f32 = 5.0;
const HOVER_LIFT: f32 = 0.8;
// A key reaches back to the base. Its corners and front edge, and the gap to its neighbors
const KEY_DEPTH: f32 = BASE_FRONT - FACE;
const KEY_CORNER: f32 = 12.0;
const KEY_EDGE: f32 = 3.0;
const KEY_INSET: f32 = 1.0;
// A rocker stands in the base, its middle at the base's front face. Its raised end stays this far
// behind the panel's plane
const ROCKER_DEPTH: f32 = 8.0;
const ROCKER_CLEARANCE: f32 = 1.0;
const ROCKER_MOST_TILT: f32 = 0.14;
// A slider's groove, its fill and its knob, and the marks of its values
const GROOVE_HEIGHT: f32 = 12.0;
const GROOVE_DEPTH: f32 = 3.0;
const FILL_HEIGHT: f32 = 6.0;
// The width of a slider's knob, in dp
pub(crate) const KNOB: f32 = 32.0;
const KNOB_DEPTH: f32 = 9.0;
const KNOB_PRESS: f32 = 3.0;
const MARK_RADIUS: f32 = 2.5;
// The segments around a ball: a ball of a few centimeters looks round with 24
const DOME_SEGMENTS: u32 = 24;
// How far the tray stands behind its place and how small it is when it starts to show
const SHOW_DEPTH: f32 = 30.0;
const SHOW_SCALE: f32 = 0.85;
// The colors of a slider's groove and of a rocker's light while off
const GROOVE_COLOR: Color = Color(0.05, 0.06, 0.1, 1.0);
const DARK_LIGHT: Color = Color(0.12, 0.13, 0.18, 1.0);

// The light of every tray: from the top left, ahead of the viewer. Its shadows take four taps of
// the shadow map, which soften their edges by a texel. On a Quest 3 the shadow map of an open menu
// tray takes 0.4 ms of the GPU per frame. A light of some size takes 32 to 48 taps for its
// penumbra: the same tray took 5.2 ms of the GPU per frame with it, against 3.0 ms with four taps.
pub(crate) const TRAY_LIGHT: Light = Light {
    direction: Vec3::new(0.35, 0.55, 0.76),
    ambient: 0.42,
    shadows: true,
    size: 0.0,
};

// The base of `size` dp in `color`, when the tray has one, and the parts on it.
#[composable]
pub(crate) fn TrayScene(tray: TrayParts, base: Option<Color>, size: [f32; 2]) {
    if let Some(color) = base {
        let mesh = rememberSlab([size[0], size[1], BASE_DEPTH], BASE_CORNER, BASE_EDGE);
        Body(
            (mesh, color, true),
            move || {
                Mat4::translation(Vec3::new(0.0, 0.0, BASE_FRONT + BASE_DEPTH / 2.0))
                    .then(&shown_transform(tray.shown))
            },
            || satin(Finish::Matte, 0.0),
        );
    }
    // Parts on no base have nothing to cast a shadow on
    let shadows = base.is_some();
    for part in tray.list.get() {
        key(part.id, move || PartBody(tray, part, (size, shadows)));
    }
}

// The scale and the move back that a tray takes as it shows and hides.
fn shown_transform(shown: State<f32>) -> Mat4 {
    let shown = shown.get();
    let scale = SHOW_SCALE + (1.0 - SHOW_SCALE) * shown;
    Mat4::scaling(Vec3::new(scale, scale, 1.0)).then(&Mat4::translation(Vec3::new(
        0.0,
        0.0,
        (1.0 - shown) * SHOW_DEPTH,
    )))
}

// The animations of one part: the press, the hover, the flip of a rocker and a slider's value.
#[derive(Clone, Copy)]
struct Motion {
    press: State<f32>,
    hover: State<f32>,
    flip: State<f32>,
    value: State<f32>,
}

#[composable]
fn PartBody(tray: TrayParts, part: Part, (size, shadows): ([f32; 2], bool)) {
    let motion = Motion {
        press: animateFloatAsState(
            f32::from(u8::from(part.pressed.get())),
            tween(70, Easing::EaseOut),
            "press",
        ),
        hover: animateFloatAsState(
            f32::from(u8::from(part.hovered.get())),
            tween(120, Easing::EaseOut),
            "hover",
        ),
        flip: animateFloatAsState(
            f32::from(u8::from(part.on.get())),
            spring(0.55, 700.0),
            "flip",
        ),
        value: animateFloatAsState(part.value.get(), tween(60, Easing::EaseOut), "value"),
    };
    let rect = part.rect.get();
    let [face, lit] = part.colors.get();
    // The place of the part's middle on the tray, which the meshes read while they draw
    let middle = move || {
        let [x, y] = tray.center_of(part.rect.get(), size);
        (x, y, (1.0 - tray.shown.get()) * 0.4 * SHOW_DEPTH)
    };
    let place = move |local: Mat4| {
        let (x, y, back) = middle();
        local
            .then(&Mat4::translation(Vec3::new(x, y, back)))
            .then(&shown_transform(tray.shown))
    };
    let glow = move || satin(Finish::Glossy, 0.18 * motion.hover.get());
    match part.shape {
        // The parts of a hologram's controls stand on a tray as the nearest of its own bodies
        PartShape::Key | PartShape::Choice { .. } | PartShape::Arrow { .. } => {
            let mesh = rememberSlab(
                [
                    rect.width - 2.0 * KEY_INSET,
                    rect.height - 2.0 * KEY_INSET,
                    KEY_DEPTH,
                ],
                KEY_CORNER,
                KEY_EDGE,
            );
            Body(
                (mesh, face, shadows),
                move || {
                    place(Mat4::translation(Vec3::new(
                        0.0,
                        0.0,
                        sunk(motion) + KEY_DEPTH / 2.0,
                    )))
                },
                glow,
            );
        }
        PartShape::Dome | PartShape::Crystal | PartShape::Emblem | PartShape::Gem => {
            let radius = rect.width.min(rect.height) / 2.0 - KEY_INSET;
            let mesh = remember(|| Arc::new(Mesh::sphere(DOME_SEGMENTS, DOME_SEGMENTS / 2)))
                .with(Arc::clone);
            Body(
                (mesh, face, shadows),
                move || {
                    place(Mat4::scaling(Vec3::new(radius, radius, radius)).then(
                        &Mat4::translation(Vec3::new(0.0, 0.0, sunk(motion) + radius)),
                    ))
                },
                glow,
            );
        }
        PartShape::Rocker => Rocker(
            [rect.width, rect.height],
            ([face, lit], shadows),
            motion,
            place,
            glow,
        ),
        PartShape::Slider { stops } | PartShape::Track { count: stops } => {
            Slider((rect.width, stops), ([face, lit], shadows), motion, place);
        }
        PartShape::Ribbon | PartShape::Wave => {}
    }
}

// How far behind the panel's plane a key's face stands now.
fn sunk(motion: Motion) -> f32 {
    FACE + PRESS_DEPTH * motion.press.get() - HOVER_LIFT * motion.hover.get()
}

#[composable]
fn Rocker(
    [width, height]: [f32; 2],
    ([face, lit], shadows): ([Color; 2], bool),
    motion: Motion,
    place: impl Fn(Mat4) -> Mat4 + Copy + 'static,
    glow: impl Fn() -> Material + Copy + 'static,
) {
    let half = (width / 2.0).max(1.0);
    // The raised end comes forward by the half width times the tilt's sine
    let room = BASE_FRONT - ROCKER_DEPTH / 2.0 - ROCKER_CLEARANCE;
    let tilt = (room / half).clamp(0.0, 1.0).asin().min(ROCKER_MOST_TILT);
    let mesh = rememberSlab(
        [
            width - 2.0 * KEY_INSET,
            height - 2.0 * KEY_INSET,
            ROCKER_DEPTH,
        ],
        KEY_CORNER,
        KEY_EDGE,
    );
    let (flip, press) = (motion.flip, motion.press);
    // On, the end half lies deeper: a positive turn about y takes +x away from the viewer
    let turn = move || tilt * (2.0 * flip.get() - 1.0) * (1.0 - 0.3 * press.get());
    Body(
        (mesh, face, shadows),
        move || {
            place(
                Mat4::rotation_y(turn()).then(&Mat4::translation(Vec3::new(0.0, 0.0, BASE_FRONT))),
            )
        },
        glow,
    );
    // A light on the end half that shines while on
    let dot = remember(|| Arc::new(Mesh::sphere(16, 8))).with(Arc::clone);
    let radius = (height / 10.0).clamp(2.0, 4.0);
    let at = half * 0.62;
    let on = flip.get() > 0.5;
    Body(
        (dot, if on { lit } else { DARK_LIGHT }, shadows),
        move || {
            // The point of the face at `at` turns with the rocker
            let (sin, cos) = turn().sin_cos();
            let front = BASE_FRONT - ROCKER_DEPTH / 2.0 + at * sin - radius * 0.25;
            place(
                Mat4::scaling(Vec3::new(radius, radius, radius * 0.5))
                    .then(&Mat4::translation(Vec3::new(at * cos, 0.0, front))),
            )
        },
        move || {
            if on {
                Material::Emissive
            } else {
                satin(Finish::Glossy, 0.0)
            }
        },
    );
}

#[composable]
fn Slider(
    (width, stops): (f32, u32),
    ([face, lit], shadows): ([Color; 2], bool),
    motion: Motion,
    place: impl Fn(Mat4) -> Mat4 + Copy + 'static,
) {
    let knob = KNOB;
    let track = (width - knob).max(1.0);
    let start = -width / 2.0 + knob / 2.0;
    let groove = rememberSlab(
        [track + GROOVE_HEIGHT, GROOVE_HEIGHT, GROOVE_DEPTH],
        GROOVE_HEIGHT / 2.0,
        1.0,
    );
    let groove_front = BASE_FRONT - GROOVE_DEPTH;
    Body(
        (groove, GROOVE_COLOR, shadows),
        move || {
            place(Mat4::translation(Vec3::new(
                0.0,
                0.0,
                groove_front + GROOVE_DEPTH / 2.0,
            )))
        },
        || satin(Finish::Matte, 0.0),
    );
    let (value, press, hover) = (motion.value, motion.press, motion.hover);
    // The fill from the start to the knob: a box that the value stretches along x
    let fill = remember(|| Arc::new(Mesh::cuboid())).with(Arc::clone);
    Body(
        (fill, lit, shadows),
        move || {
            let length = (track * value.get()).max(0.5);
            place(
                Mat4::scaling(Vec3::new(length, FILL_HEIGHT, 2.0)).then(&Mat4::translation(
                    Vec3::new(start + length / 2.0, 0.0, groove_front - 0.5),
                )),
            )
        },
        move || satin(Finish::Glossy, 0.1),
    );
    // A mark under the groove at each value that the slider takes
    let mark = remember(|| Arc::new(Mesh::sphere(12, 6))).with(Arc::clone);
    for index in 0..stops.max(2) {
        let at = start + track * index as f32 / (stops.max(2) - 1) as f32;
        let mark = Arc::clone(&mark);
        Body(
            (mark, face, shadows),
            move || {
                place(
                    Mat4::scaling(Vec3::new(MARK_RADIUS, MARK_RADIUS, MARK_RADIUS * 0.6)).then(
                        &Mat4::translation(Vec3::new(
                            at,
                            GROOVE_HEIGHT / 2.0 + 6.0,
                            BASE_FRONT - 0.5,
                        )),
                    ),
                )
            },
            || satin(Finish::Matte, 0.0),
        );
    }
    let puck = rememberSlab([knob, knob, KNOB_DEPTH], knob / 2.0, 3.0);
    Body(
        (puck, face, shadows),
        move || {
            let front = FACE + KNOB_PRESS * press.get() - HOVER_LIFT * hover.get();
            place(Mat4::translation(Vec3::new(
                start + track * value.get(),
                0.0,
                front + KNOB_DEPTH / 2.0,
            )))
        },
        move || satin(Finish::Glossy, 0.18 * hover.get()),
    );
}

// One mesh in `color`, which `transform` places and `material` lights while the scene draws. It
// casts a shadow when `shadows` says so.
#[composable]
fn Body(
    (mesh, color, shadows): (Arc<Mesh>, Color, bool),
    transform: impl Fn() -> Mat4 + 'static,
    material: impl Fn() -> Material + 'static,
) {
    Box(
        Modifier::empty()
            .required_size(Size::new(2.0, 2.0))
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&mesh),
                transform: transform(),
                material: material(),
                casts_shadow: shadows,
                solid: false,
            })
            .background(color),
        BoxSpec::default(),
        || {},
    );
}

// A slab of `size` dp, made again only when its size changes by half a dp or more.
#[composable]
fn rememberSlab(size: [f32; 3], corner: f32, edge: f32) -> Arc<Mesh> {
    rememberShape(size, move |size| slab(size, corner, edge))
}

// A mesh that `make` builds from `size`, made again only when a side of the size changes by half a
// unit or more.
#[composable]
pub(crate) fn rememberShape<const N: usize>(
    size: [f32; N],
    make: impl Fn([f32; N]) -> Mesh,
) -> Arc<Mesh> {
    let made = remember(|| Rc::new(RefCell::new(None::<([f32; N], Arc<Mesh>)>))).with(Rc::clone);
    let mut made = made.borrow_mut();
    match made.as_ref() {
        Some((known, mesh))
            if known
                .iter()
                .zip(size)
                .all(|(known, side)| (known - side).abs() < 0.5) =>
        {
            Arc::clone(mesh)
        }
        _ => {
            let mesh = Arc::new(make(size));
            *made = Some((size, Arc::clone(&mesh)));
            mesh
        }
    }
}
