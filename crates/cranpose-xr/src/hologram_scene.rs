//! The 3D content of a `SpatialHologram`: an object of glowing glass for each control, and the
//! projector, the ribbons and the sparks around the page (`hologram_stage.rs`).
//!
//! The volume's box is centered on the panel's plane, in the content's axes, which point right,
//! down and away from the viewer. Each object stands at the middle of its control's place on the
//! panel. The objects glow and leave no depth (`hologram_material.rs`), so they may stand in front
//! of the panel's plane: the words show over them. The meshes read the animations and the clock
//! while the scene draws, so the motion composes nothing. The clock runs only while the page holds
//! something that moves.
//!
//! An object is glass filled with light: a bright rim where its surface turns from the eye, a fill,
//! glowing edges and an inner light about its middle. The objects of the controls are here, and
//! `hologram_objects.rs` draws the options of a choice.

use std::{
    f32::consts::{FRAC_PI_2, PI},
    sync::{Arc, OnceLock},
};

use cranpose::{
    Box, BoxSpec, Color, Modifier, Size, composable,
    space::{Mat4, Mesh, MeshSurface, Vec3},
};
use cranpose_animation::{Easing, animateFloatAsState, spring, tween};
use cranpose_core::{LaunchedEffect, State, key, rememberMutableStateOf};

use crate::{
    HologramColors, HologramStage,
    hologram_gem::GemBody,
    hologram_material::{Edges, Glow},
    hologram_meshes::{arc, arrow, capsule, cone, crystal, disc, helix, ring, rings, sheet, wave},
    hologram_objects::{ChoiceObject, STEPPER_LIFT, Stone, Track},
    hologram_stage::Stage,
    tray_part::{Part, PartShape, TrayParts},
    tray_scene::rememberShape,
};

// How long a hologram takes to rise out of its projector and to sink back, in milliseconds
pub(crate) const HOLOGRAM_SHOW_MILLIS: u64 = 420;
pub(crate) const HOLOGRAM_HIDE_MILLIS: u64 = 260;
// How small the page is as it leaves the projector
const SHOW_FROM: f32 = 0.12;
// How far under the page's bottom edge the middle of its handle stands, in dp: the gap and the
// radius of a handle (`handle.rs`). The projector's middle stands there and is the handle.
pub(crate) const HANDLE_DROP: f32 = 40.0;
// How far a press moves an object away from the viewer, in dp
const PRESS_DEPTH: f32 = 6.0;
// The sweep of the arc of `ChoiceForm::Arc`, in radians, and the radius of its tube as a share of
// its radius
pub(crate) const ARC_SWEEP: f32 = 1.0;
const ARC_TUBE: f32 = 0.02;

// The meshes that every hologram's objects share. Each draws at every size, so the process makes
// them once.
pub(crate) struct Meshes {
    pub(crate) crystal: Arc<Mesh>,
    pub(crate) gem: Arc<Mesh>,
    pub(crate) octahedron: Arc<Mesh>,
    pub(crate) stone: Arc<Mesh>,
    pub(crate) cube: Arc<Mesh>,
    pub(crate) ring: Arc<Mesh>,
    pub(crate) halo: Arc<Mesh>,
    pub(crate) beam: Arc<Mesh>,
    pub(crate) arc: Arc<Mesh>,
    pub(crate) arrow: Arc<Mesh>,
    pub(crate) disc: Arc<Mesh>,
    pub(crate) plate: Arc<Mesh>,
    pub(crate) lens: Arc<Mesh>,
    pub(crate) orbit: Arc<Mesh>,
    pub(crate) cone: Arc<Mesh>,
    pub(crate) helix: Arc<Mesh>,
    pub(crate) sheet: Arc<Mesh>,
    pub(crate) wave: Arc<Mesh>,
}

static MESHES: OnceLock<Meshes> = OnceLock::new();

pub(crate) fn meshes() -> &'static Meshes {
    MESHES.get_or_init(Meshes::new)
}

impl Meshes {
    fn new() -> Self {
        Self {
            crystal: Arc::new(crystal(6, 0.6, 0.6)),
            gem: Arc::new(crystal(8, 0.28, 0.55)),
            octahedron: Arc::new(crystal(4, 0.5, 0.5)),
            stone: Arc::new(crystal(8, 0.42, 0.42)),
            cube: Arc::new(Mesh::cuboid()),
            ring: Arc::new(ring(0.012)),
            halo: Arc::new(Mesh::sphere(20, 10)),
            beam: Arc::new(Mesh::cylinder(20)),
            arc: Arc::new(arc(ARC_SWEEP, ARC_TUBE)),
            arrow: Arc::new(arrow(0.35)),
            disc: Arc::new(rings(&[1.0, 0.82, 0.55], 0.03)),
            plate: Arc::new(disc()),
            lens: Arc::new(rings(&[1.0, 0.8], 0.08)),
            orbit: Arc::new(ring(0.03)),
            cone: Arc::new(cone(1.0, 1.6)),
            helix: Arc::new(helix(2, 0.22)),
            sheet: Arc::new(sheet(0.12)),
            wave: Arc::new(wave(40, 10, 0.016)),
        }
    }
}

// What every object of one hologram reads: its parts, its size in dp, its colors, its clock in
// seconds and whether the user carries it by a handle under it.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Scene {
    pub(crate) tray: TrayParts,
    pub(crate) size: [f32; 2],
    pub(crate) colors: HologramColors,
    pub(crate) time: State<f32>,
    pub(crate) movable: bool,
}

impl Scene {
    // How far the page has come out of its projector, from 0 to 1.
    pub(crate) fn shown(self) -> f32 {
        self.tray.shown.get()
    }

    // The middle of the projector under the page, where the handle stands.
    pub(crate) fn projector(self) -> Vec3 {
        Vec3::new(0.0, self.size[1] / 2.0 + HANDLE_DROP, 0.0)
    }

    // The move of the page out of its projector: it grows from the projector's middle.
    fn rise(self) -> Mat4 {
        let grow = SHOW_FROM + (1.0 - SHOW_FROM) * self.shown();
        let at = self.projector();
        Mat4::translation(-at)
            .then(&Mat4::scaling(Vec3::new(grow, grow, grow)))
            .then(&Mat4::translation(at))
    }

    // The middle of `part`'s place on the page.
    pub(crate) fn center(self, part: Part) -> Vec3 {
        let [x, y] = self.tray.center_of(part.rect.get(), self.size);
        Vec3::new(x, y, 0.0)
    }

    // `local` moved to `at` on the page, as the page rises.
    pub(crate) fn at(self, at: Vec3, local: Mat4) -> Mat4 {
        local.then(&Mat4::translation(at)).then(&self.rise())
    }

    pub(crate) fn time(self) -> f32 {
        self.time.get()
    }
}

// The animations of one part: the press, the hover, the bloom of a choice and the value.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Motion {
    pub(crate) press: State<f32>,
    pub(crate) hover: State<f32>,
    pub(crate) bloom: State<f32>,
    pub(crate) value: State<f32>,
}

impl Motion {
    // How much brighter the part glows now: a ray that rests on it and the flash of a press.
    pub(crate) fn lift(self) -> f32 {
        1.0 + 0.35 * self.hover.get() + 0.9 * self.press.get()
    }

    // How far the press moves the part away from the viewer.
    pub(crate) fn sunk(self) -> Vec3 {
        Vec3::new(0.0, 0.0, PRESS_DEPTH * self.press.get())
    }
}

// The objects of every part of `tray` and the stage around them, on a page of `size` dp, which
// the user carries by a handle under it while `movable`.
#[composable]
pub(crate) fn HologramScene(
    tray: TrayParts,
    (colors, stage, movable): (HologramColors, HologramStage, bool),
    size: [f32; 2],
) {
    let parts = tray.list.get();
    let moving = stage.moves() || parts.iter().any(|part| part.shape.moves());
    let time = rememberMutableStateOf(|| 0.0_f32);
    // The clock runs while the page holds something that moves, and ends with the page
    LaunchedEffect(moving, async move |scope| {
        if !moving {
            return;
        }
        let start = scope.with_frame_nanos(|now| now).await;
        loop {
            let now = scope.with_frame_nanos(|now| now).await;
            time.set(now.saturating_sub(start) as f32 * 1.0e-9);
        }
    });
    let scene = Scene {
        tray,
        size,
        colors,
        time: time.as_state(),
        movable,
    };
    Stage(scene, stage);
    for part in parts {
        key(part.id, move || PartObject(scene, part));
    }
}

#[composable]
fn PartObject(scene: Scene, part: Part) {
    let motion = Motion {
        press: animateFloatAsState(
            f32::from(u8::from(part.pressed.get())),
            tween(80, Easing::EaseOut),
            "press",
        ),
        hover: animateFloatAsState(
            f32::from(u8::from(part.hovered.get())),
            tween(120, Easing::EaseOut),
            "hover",
        ),
        bloom: animateFloatAsState(
            f32::from(u8::from(part.on.get())),
            spring(0.5, 260.0),
            "bloom",
        ),
        value: animateFloatAsState(part.value.get(), tween(240, Easing::EaseOut), "value"),
    };
    let rect = part.rect.get();
    let [face, lit] = part.colors.get();
    match part.shape {
        PartShape::Key => Key(scene, part, (rect.width, rect.height, face), motion),
        PartShape::Dome => {
            let radius = rect.width.min(rect.height) * 0.42;
            Glowing(
                Arc::clone(&meshes().crystal),
                move || {
                    let shape = scaled(radius * 2.0).then(&Mat4::rotation_y(0.4));
                    scene.at(scene.center(part) + motion.sunk(), shape)
                },
                move || glass_body(face, Edges::Facets).times(motion.lift() * scene.shown()),
            );
        }
        PartShape::Rocker => Capsule(
            scene,
            part,
            ([rect.width, rect.height], [face, lit]),
            motion,
        ),
        PartShape::Slider { .. } => Groove(scene, part, (rect.width, lit), motion),
        PartShape::Choice { form, index, count } => {
            ChoiceObject(scene, part, (form, index, count), motion);
        }
        PartShape::Track { count } => Track(scene, part, count, motion),
        PartShape::Crystal => Stone(scene, part, motion),
        PartShape::Arrow { forward } => Arrow(scene, part, forward, motion),
        PartShape::Emblem => Emblem(scene, part, rect.width.min(rect.height)),
        PartShape::Ribbon => Ribbon(scene, part, [rect.width, rect.height]),
        PartShape::Wave => Wave(scene, part, [rect.width, rect.height]),
        PartShape::Gem => GemBody(scene, part, (rect.width.min(rect.height), face), motion),
    }
}

// Glass of `color` filled with light: a bright rim, a fill, glowing `edges` and an inner light
// about the mesh's middle.
pub(crate) fn glass_body(color: Color, edges: Edges) -> Glow {
    Glow {
        rim: 1.25,
        fill: 0.16,
        ..Glow::glass(color)
    }
    .edged(edges, 1.0, 0.06)
    .lit_inside(0.6, 0.5)
}

pub(crate) fn scaled(side: f32) -> Mat4 {
    Mat4::scaling(Vec3::new(side, side, side))
}

pub(crate) fn mix(from: Color, to: Color, share: f32) -> Color {
    let share = share.clamp(0.0, 1.0);
    let at = |a: f32, b: f32| a + (b - a) * share;
    Color(at(from.0, to.0), at(from.1, to.1), at(from.2, to.2), 1.0)
}

// A capsule of `length` and `height` dp, as a mesh of length 1 that a transform scales by `length`.
fn pill([length, height]: [f32; 2]) -> Mesh {
    capsule(height / (2.0 * length.max(1.0)))
}

// A key: a flat pill of glass of the key's own color, lit from inside.
#[composable]
fn Key(scene: Scene, part: Part, (width, height, face): (f32, f32, Color), motion: Motion) {
    let length = (width - 4.0).max(1.0);
    let mesh = rememberShape([length, height - 4.0], pill);
    Glowing(
        mesh,
        move || {
            let shape = Mat4::scaling(Vec3::new(length, length, length * 0.3));
            scene.at(scene.center(part) + motion.sunk(), shape)
        },
        move || {
            Glow {
                rim: 1.0,
                fill: 0.05,
                ..Glow::glass(face)
            }
            .lit_inside(0.45, 0.4)
            .times(motion.lift() * scene.shown())
        },
    );
}

// A capsule of glass whose lit half slides to the end while on and to the start while off.
#[composable]
fn Capsule(scene: Scene, part: Part, (size, [face, lit]): ([f32; 2], [Color; 2]), motion: Motion) {
    let [width, height] = [size[0] - 4.0, size[1] - 6.0];
    let shell = rememberShape([width, height], pill);
    let half = (width / 2.0).max(height);
    let core = rememberShape([half, height - 8.0], pill);
    Glowing(
        shell,
        move || {
            let shape = Mat4::scaling(Vec3::new(width, width, width * 0.6));
            scene.at(scene.center(part) + motion.sunk(), shape)
        },
        move || {
            Glow {
                rim: 1.1,
                fill: 0.035,
                ..Glow::glass(face)
            }
            .times(motion.lift() * scene.shown())
        },
    );
    let travel = (width - half) / 2.0;
    Glowing(
        core,
        move || {
            let flip = motion.bloom.get();
            let x = -travel + 2.0 * travel * flip;
            let shape = Mat4::scaling(Vec3::new(half, half, half * 0.6));
            scene.at(
                scene.center(part) + motion.sunk() + Vec3::new(x, 0.0, 0.0),
                shape,
            )
        },
        move || {
            let flip = motion.bloom.get();
            Glow {
                rim: 0.9,
                fill: 0.36,
                ..Glow::glass(mix(face, lit, flip))
            }
            .lit_inside(0.45, 0.45)
            .times((0.45 + 0.55 * flip) * motion.lift() * scene.shown())
        },
    );
}

// A slider's groove of glass with a crystal knob at its value.
#[composable]
fn Groove(scene: Scene, part: Part, (width, lit): (f32, Color), motion: Motion) {
    let groove = rememberShape([width, 12.0], pill);
    Glowing(
        groove,
        move || scene.at(scene.center(part), scaled(width)),
        move || {
            Glow::glass(scene.colors.glass)
                .lit_inside(0.2, 0.5)
                .times(scene.shown())
        },
    );
    let track = (width - 32.0).max(1.0);
    Glowing(
        Arc::clone(&meshes().crystal),
        move || {
            let x = -track / 2.0 + track * motion.value.get();
            scene.at(
                scene.center(part) + motion.sunk() + Vec3::new(x, 0.0, -6.0),
                scaled(26.0).then(&Mat4::rotation_y(0.4)),
            )
        },
        move || glass_body(lit, Edges::Facets).times(motion.lift() * scene.shown()),
    );
}

// A stepper's arrow toward the next option or the one before. It swings a little, and it dims
// while it has no option to go to. It stands as high as the stepper's crystal.
#[composable]
fn Arrow(scene: Scene, part: Part, forward: bool, motion: Motion) {
    let rect = part.rect.get();
    let side = rect.width.min(rect.height) * 0.45;
    let enabled = part.on.get();
    let turn = if forward { 0.0 } else { PI };
    let lift = Vec3::new(0.0, STEPPER_LIFT, 0.0);
    Glowing(
        Arc::clone(&meshes().arrow),
        move || {
            let swing = Mat4::rotation_y(turn + 0.25 * (scene.time() * 1.3).sin());
            scene.at(
                scene.center(part) + lift + motion.sunk(),
                scaled(side).then(&swing),
            )
        },
        move || {
            let strength = if enabled { motion.lift() } else { 0.3 };
            glass_body(scene.colors.glass, Edges::Facets).times(strength * scene.shown())
        },
    );
}

// A ring that orbits a point: its size along x, y and z, its tilt toward the viewer, and how fast
// and how far it swings, in radians.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Orbit {
    pub(crate) size: [f32; 3],
    pub(crate) tilt: f32,
    pub(crate) speed: f32,
    pub(crate) phase: f32,
    pub(crate) swing: [f32; 2],
}

// A ring of `mesh` that orbits `middle` on the page as `orbit` says, in the theme's chosen color.
#[composable]
pub(crate) fn Orbiting(scene: Scene, middle: Vec3, mesh: Arc<Mesh>, orbit: Orbit) {
    Glowing(
        mesh,
        move || {
            let time = scene.time() * orbit.speed + orbit.phase;
            let [x, y, z] = orbit.size;
            let shape = Mat4::scaling(Vec3::new(x, y, z))
                .then(&Mat4::rotation_x(orbit.tilt))
                .then(&Mat4::rotation_z(orbit.swing[0] * time.sin()))
                .then(&Mat4::rotation_y(orbit.swing[1] * time.cos()));
            scene.at(middle, shape)
        },
        move || {
            Glow {
                rim: 0.6,
                fill: 0.9,
                ..Glow::glass(scene.colors.chosen)
            }
            .times(scene.shown())
        },
    );
}

// A round emblem: a disc of glass lit from inside that faces the viewer, a rim of two rings and
// two rings that orbit it.
#[composable]
fn Emblem(scene: Scene, part: Part, side: f32) {
    let radius = side * 0.4;
    let facing = Mat4::rotation_x(-FRAC_PI_2);
    let middle = scene.center(part);
    Glowing(
        Arc::clone(&meshes().plate),
        move || scene.at(middle, scaled(radius).then(&facing)),
        move || {
            Glow {
                rim: 0.0,
                fill: 0.22,
                ..Glow::glass(scene.colors.glass)
            }
            .lit_inside(0.9, 0.9)
            .times(scene.shown())
        },
    );
    Glowing(
        Arc::clone(&meshes().lens),
        move || {
            let shape = scaled(radius * 1.04).then(&facing);
            scene.at(middle + Vec3::new(0.0, 0.0, -1.0), shape)
        },
        move || {
            Glow {
                fill: 1.0,
                ..Glow::glass(scene.colors.glass)
            }
            .times(scene.shown())
        },
    );
    let orbit = radius * 1.35;
    for (tilt, speed, phase) in [(1.25, 0.6, 0.0), (1.05, -0.45, 1.3)] {
        let ring = Orbit {
            size: [orbit; 3],
            tilt,
            speed,
            phase,
            swing: [0.5, 0.35],
        };
        Orbiting(scene, middle, Arc::clone(&meshes().orbit), ring);
    }
}

// A band of glass behind words: a bent sheet a little wider than its place, with glowing edges
// and a light inside that is brightest behind its middle.
#[composable]
fn Ribbon(scene: Scene, part: Part, [width, height]: [f32; 2]) {
    let (width, height) = (width + 36.0, height + 6.0);
    Glowing(
        Arc::clone(&meshes().sheet),
        move || {
            let shape = Mat4::scaling(Vec3::new(width, height, width));
            scene.at(scene.center(part) + Vec3::new(0.0, 0.0, 4.0), shape)
        },
        move || {
            Glow {
                rim: 0.0,
                fill: 0.13,
                ..Glow::glass(scene.colors.glass)
            }
            .edged(Edges::Quads, 1.1, 0.025)
            .lit_inside(0.4, 0.6)
            .times(scene.shown())
        },
    );
}

// A wave of points over the part's place, seen a little from above, with bands of light that run
// over it.
#[composable]
fn Wave(scene: Scene, part: Part, [width, height]: [f32; 2]) {
    Glowing(
        Arc::clone(&meshes().wave),
        move || {
            let shape =
                Mat4::scaling(Vec3::new(width, height, height * 1.4)).then(&Mat4::rotation_x(0.55));
            scene.at(
                scene.center(part) + Vec3::new(0.0, height * 0.35, 0.0),
                shape,
            )
        },
        move || {
            Glow {
                rim: 0.4,
                fill: 1.2,
                ..Glow::glass(scene.colors.chosen)
            }
            .scanned(9.0, -scene.time() * 2.2)
            .times(scene.shown())
        },
    );
}

// One mesh that `transform` places and `glow` lights while the scene draws. Its node draws no
// content, so the mesh costs no pass of its own.
#[composable]
pub(crate) fn Glowing(
    mesh: Arc<Mesh>,
    transform: impl Fn() -> Mat4 + 'static,
    glow: impl Fn() -> Glow + 'static,
) {
    Box(
        Modifier::empty()
            .required_size(Size::new(2.0, 2.0))
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&mesh),
                transform: transform(),
                material: glow().material(),
                casts_shadow: false,
                solid: false,
            }),
        BoxSpec::default(),
        || {},
    );
}
