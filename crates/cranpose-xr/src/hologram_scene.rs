//! The 3D content of a `SpatialHologram`: an object of glowing glass for each control, and the
//! projector, the ribbons and the sparks around the page.
//!
//! The volume's box is centered on the panel's plane, in the content's axes, which point right,
//! down and away from the viewer. Each object stands at the middle of its control's place on the
//! panel. The objects glow and leave no depth (`hologram_material.rs`), so they may stand in front
//! of the panel's plane: the words show over them. The meshes read the animations and the clock
//! while the scene draws, so the motion composes nothing. The clock runs only while the page holds
//! something that moves.

use std::{
    f32::consts::{FRAC_PI_2, PI},
    sync::{Arc, OnceLock},
};

use cranpose::{
    Box, BoxSpec, Color, Modifier, Size, composable,
    space::{Mat4, Mesh, MeshSurface, Vec3},
};
use cranpose_animation::{Easing, animateFloatAsState, spring, tween};
use cranpose_core::{LaunchedEffect, State, key, remember, rememberMutableStateOf};

use crate::{
    ChoiceForm, HologramColors, HologramStage,
    hologram_material::{Edges, Glow},
    hologram_meshes::{
        arc, arrow, capsule, cone, crystal, flower, helix, ring, rings, sheet, sparks,
    },
    spatial_choice_group::WORDS_UNDER,
    tray_meshes::slab,
    tray_part::{Part, PartShape, TrayParts},
    tray_scene::rememberShape,
};

// How long a hologram takes to rise out of its projector and to sink back, in milliseconds
pub(crate) const HOLOGRAM_SHOW_MILLIS: u64 = 420;
pub(crate) const HOLOGRAM_HIDE_MILLIS: u64 = 260;
// How small the page is as it leaves the projector
const SHOW_FROM: f32 = 0.12;
// How far under the page the projector stands, in dp
const PROJECTOR_DROP: f32 = 80.0;
// How far a press moves an object away from the viewer, in dp
const PRESS_DEPTH: f32 = 6.0;
// The sweep of the arc of `ChoiceForm::Arc`, in radians
const ARC_SWEEP: f32 = 1.0;
// How far the arc leans back, so it shows as a curve that rises at its ends
const ARC_LEAN: f32 = 0.45;
// How far the arc's knob floats over it, in dp
const KNOB_FLOAT: f32 = 16.0;
// The radius of a ribbon's turns as a share of the page's width
const RIBBON_RADIUS: f32 = 0.1;
// How far a chosen object grows
const BLOOM: f32 = 0.22;

// The meshes that every hologram's objects share. Each draws at every size, so the process makes
// them once.
struct Meshes {
    crystal: Arc<Mesh>,
    gem: Arc<Mesh>,
    stone: Arc<Mesh>,
    cube: Arc<Mesh>,
    ring: Arc<Mesh>,
    halo: Arc<Mesh>,
    beam: Arc<Mesh>,
    arc: Arc<Mesh>,
    arrow: Arc<Mesh>,
    disc: Arc<Mesh>,
    lens: Arc<Mesh>,
    orbit: Arc<Mesh>,
    cone: Arc<Mesh>,
    helix: Arc<Mesh>,
    sheet: Arc<Mesh>,
}

static MESHES: OnceLock<Meshes> = OnceLock::new();

fn meshes() -> &'static Meshes {
    MESHES.get_or_init(Meshes::new)
}

impl Meshes {
    fn new() -> Self {
        Self {
            crystal: Arc::new(crystal(6, 0.6, 0.6)),
            gem: Arc::new(crystal(8, 0.28, 0.55)),
            stone: Arc::new(crystal(8, 0.42, 0.42)),
            cube: Arc::new(Mesh::cuboid()),
            ring: Arc::new(ring(0.01)),
            halo: Arc::new(Mesh::sphere(20, 10)),
            beam: Arc::new(Mesh::cylinder(20)),
            arc: Arc::new(arc(ARC_SWEEP, 0.018)),
            arrow: Arc::new(arrow(0.35)),
            disc: Arc::new(rings(&[1.0, 0.86, 0.62, 0.36], 0.025)),
            lens: Arc::new(rings(&[1.0, 0.8], 0.08)),
            orbit: Arc::new(ring(0.03)),
            cone: Arc::new(cone(1.0, 2.0)),
            helix: Arc::new(helix(2, 0.22)),
            sheet: Arc::new(sheet(0.12)),
        }
    }
}

// What every object of one hologram reads: its parts, its size in dp, its colors and its clock in
// seconds.
#[derive(Clone, Copy, PartialEq)]
struct Scene {
    tray: TrayParts,
    size: [f32; 2],
    colors: HologramColors,
    time: State<f32>,
}

impl Scene {
    // How far the page has come out of its projector, from 0 to 1.
    fn shown(self) -> f32 {
        self.tray.shown.get()
    }

    fn projector(self) -> Vec3 {
        Vec3::new(0.0, self.size[1] / 2.0 + PROJECTOR_DROP, 0.0)
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
    fn center(self, part: Part) -> Vec3 {
        let [x, y] = self.tray.center_of(part.rect.get(), self.size);
        Vec3::new(x, y, 0.0)
    }

    // `local` moved to `at` on the page, as the page rises.
    fn at(self, at: Vec3, local: Mat4) -> Mat4 {
        local.then(&Mat4::translation(at)).then(&self.rise())
    }

    fn time(self) -> f32 {
        self.time.get()
    }
}

// The animations of one part: the press, the hover, the bloom of a choice and the value.
#[derive(Clone, Copy, PartialEq)]
struct Motion {
    press: State<f32>,
    hover: State<f32>,
    bloom: State<f32>,
    value: State<f32>,
}

impl Motion {
    // How much brighter the part glows now: a ray that rests on it and the flash of a press.
    fn lift(self) -> f32 {
        1.0 + 0.35 * self.hover.get() + 0.9 * self.press.get()
    }

    // How far the press moves the part away from the viewer.
    fn sunk(self) -> Vec3 {
        Vec3::new(0.0, 0.0, PRESS_DEPTH * self.press.get())
    }
}

// The objects of every part of `tray` and the stage around them, on a page of `size` dp.
#[composable]
pub(crate) fn HologramScene(
    tray: TrayParts,
    (colors, stage): (HologramColors, HologramStage),
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
            let mesh = Arc::clone(&meshes().crystal);
            Glowing(
                mesh,
                move || scene.at(scene.center(part) + motion.sunk(), scaled(radius * 2.0)),
                move || crystal_glow(face).times(motion.lift() * scene.shown()),
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
            let option = (form, index, count);
            ChoiceObject(scene, part, option, [rect.width, rect.height], motion);
        }
        PartShape::Track { count } => {
            Track(scene, part, (rect.width, rect.height, count), motion);
        }
        PartShape::Crystal => Stone(scene, part, [rect.width, rect.height], motion),
        PartShape::Arrow { forward } => {
            let side = rect.width.min(rect.height) * 0.55;
            let enabled = part.on.get();
            let mesh = Arc::clone(&meshes().arrow);
            let turn = if forward { 0.0 } else { PI };
            Glowing(
                mesh,
                move || {
                    let swing = Mat4::rotation_y(turn + 0.25 * (scene.time() * 1.3).sin());
                    scene.at(
                        scene.center(part) + motion.sunk(),
                        scaled(side).then(&swing),
                    )
                },
                move || {
                    let strength = if enabled { motion.lift() } else { 0.3 };
                    crystal_glow(scene.colors.glass).times(strength * scene.shown())
                },
            );
        }
        PartShape::Emblem => Emblem(scene, part, rect.width.min(rect.height)),
        PartShape::Ribbon => {
            let mesh = Arc::clone(&meshes().sheet);
            let (width, height) = (rect.width + 24.0, rect.height);
            Glowing(
                mesh,
                move || {
                    let shape = Mat4::scaling(Vec3::new(width, height, width));
                    scene.at(scene.center(part) + Vec3::new(0.0, 0.0, 4.0), shape)
                },
                move || {
                    Glow {
                        rim: 0.0,
                        fill: 0.07,
                        ..Glow::glass(scene.colors.glass)
                    }
                    .edged(Edges::Quads, 0.7, 0.03)
                    .times(scene.shown())
                },
            );
        }
    }
}

// The light of a faceted crystal of `color`: a bright rim and glowing facet edges.
fn crystal_glow(color: Color) -> Glow {
    Glow {
        fill: 0.12,
        ..Glow::glass(color)
    }
    .edged(Edges::Facets, 1.2, 0.07)
}

fn scaled(side: f32) -> Mat4 {
    Mat4::scaling(Vec3::new(side, side, side))
}

fn mix(from: Color, to: Color, share: f32) -> Color {
    let share = share.clamp(0.0, 1.0);
    let at = |a: f32, b: f32| a + (b - a) * share;
    Color(at(from.0, to.0), at(from.1, to.1), at(from.2, to.2), 1.0)
}

// A key of glass of the key's own color, with glowing edges.
#[composable]
fn Key(scene: Scene, part: Part, (width, height, face): (f32, f32, Color), motion: Motion) {
    let mesh = rememberShape([width - 2.0, height - 2.0], |[width, height]| {
        slab([width, height, 10.0], 12.0, 3.0)
    });
    Glowing(
        mesh,
        move || scene.at(scene.center(part) + motion.sunk(), Mat4::IDENTITY),
        move || {
            // A key is a large flat face, whose front and back both add their fill
            Glow {
                rim: 0.5,
                fill: 0.025,
                ..Glow::glass(face)
            }
            .edged(Edges::Quads, 0.9, 0.03)
            .times(motion.lift() * scene.shown())
        },
    );
}

// A capsule of glass whose lit half slides to the end while on and to the start while off.
#[composable]
fn Capsule(scene: Scene, part: Part, (size, [face, lit]): ([f32; 2], [Color; 2]), motion: Motion) {
    let [width, height] = [size[0] - 4.0, size[1] - 6.0];
    let shell = rememberShape([width, height], |[width, height]| {
        capsule(height / (2.0 * width.max(1.0)))
    });
    let half = (width / 2.0).max(height);
    let core = rememberShape([half, height - 8.0], |[length, height]| {
        capsule(height / (2.0 * length.max(1.0)))
    });
    Glowing(
        shell,
        move || scene.at(scene.center(part) + motion.sunk(), scaled(width)),
        move || {
            Glow {
                rim: 1.0,
                fill: 0.025,
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
            scene.at(
                scene.center(part) + motion.sunk() + Vec3::new(x, 0.0, 0.0),
                scaled(half),
            )
        },
        move || {
            let flip = motion.bloom.get();
            Glow {
                rim: 0.8,
                fill: 0.55,
                halo: 0.0,
                ..Glow::glass(mix(face, lit, flip))
            }
            .times((0.55 + 0.45 * flip) * motion.lift() * scene.shown())
        },
    );
}

// A slider's groove of glass with a crystal knob at its value.
#[composable]
fn Groove(scene: Scene, part: Part, (width, lit): (f32, Color), motion: Motion) {
    let groove = rememberShape([width, 12.0], |[width, height]| {
        capsule(height / (2.0 * width.max(1.0)))
    });
    Glowing(
        groove,
        move || scene.at(scene.center(part), scaled(width)),
        move || Glow::glass(scene.colors.glass).times(scene.shown()),
    );
    let knob = Arc::clone(&meshes().crystal);
    let track = (width - 32.0).max(1.0);
    Glowing(
        knob,
        move || {
            let x = -track / 2.0 + track * motion.value.get();
            scene.at(
                scene.center(part) + motion.sunk() + Vec3::new(x, 0.0, -6.0),
                scaled(26.0),
            )
        },
        move || crystal_glow(lit).times(motion.lift() * scene.shown()),
    );
}

// What the object of one option reads: the scene, its part, its animations, its size in dp and
// how far it stands above its place's middle, to leave room for its words.
#[derive(Clone, Copy, PartialEq)]
struct Pick {
    scene: Scene,
    part: Part,
    motion: Motion,
    side: f32,
    lift: f32,
}

impl Pick {
    fn middle(self) -> Vec3 {
        self.scene.center(self.part) + Vec3::new(0.0, self.lift, 0.0) + self.motion.sunk()
    }

    // The glass's color, which turns to the chosen color as the option blooms.
    fn color(self) -> Color {
        mix(
            self.scene.colors.glass,
            self.scene.colors.chosen,
            self.motion.bloom.get(),
        )
    }

    // The turn about y: a chosen object turns, and one that is not stands still at a
    // three-quarter view.
    fn turn(self, speed: f32) -> f32 {
        0.6 + self.motion.bloom.get() * self.scene.time() * speed
    }

    fn strength(self) -> f32 {
        (0.55 + 0.45 * self.motion.bloom.get()) * self.motion.lift() * self.scene.shown()
    }

    fn grow(self) -> f32 {
        1.0 + BLOOM * self.motion.bloom.get()
    }
}

// The object of one option of a choice group, with a halo while it is chosen.
#[composable]
fn ChoiceObject(
    scene: Scene,
    part: Part,
    (form, index, count): (ChoiceForm, u32, u32),
    [width, height]: [f32; 2],
    motion: Motion,
) {
    let step = index as f32 / count.saturating_sub(1).max(1) as f32;
    let room = (height - WORDS_UNDER).max(1.0);
    let pick = Pick {
        scene,
        part,
        motion,
        side: width.min(room) * 0.85,
        lift: -WORDS_UNDER / 2.0,
    };
    match form {
        ChoiceForm::Cubes => Cube(pick, pick.side * (0.42 + 0.58 * step), index),
        ChoiceForm::Gems => Gem(pick),
        ChoiceForm::Petals => Flower(pick, index, step),
        // The arc's track draws its ticks and knob, and a stepper draws its crystal and arrows
        ChoiceForm::Arc | ChoiceForm::Stepper => return,
    }
    if part.on.get() {
        let radius = pick.side * 0.72;
        Halo(scene, move || pick.middle(), radius, motion);
        if form == ChoiceForm::Gems {
            Beam(scene, move || pick.middle(), pick.side, motion);
        }
    }
}

// A cube of glass with an edge of `edge` dp, whose faces carry a finer grid for a later option.
#[composable]
fn Cube(pick: Pick, edge: f32, index: u32) {
    let cells = (1 + index.min(4).div_ceil(2)) as f32;
    Glowing(
        Arc::clone(&meshes().cube),
        move || {
            let shape = scaled(edge * pick.grow())
                .then(&Mat4::rotation_y(pick.turn(0.6)))
                .then(&Mat4::rotation_x(-0.42));
            pick.scene.at(pick.middle(), shape)
        },
        move || {
            Glow {
                rim: 0.35,
                fill: 0.1,
                ..Glow::glass(pick.color())
            }
            .edged(Edges::Quads, 1.4, 0.05)
            .gridded(cells)
            .times(pick.strength())
        },
    );
}

// A faceted gem, larger while chosen.
#[composable]
fn Gem(pick: Pick) {
    Glowing(
        Arc::clone(&meshes().gem),
        move || {
            let grow = 1.1 + 0.35 * pick.motion.bloom.get();
            let shape = scaled(pick.side * grow)
                .then(&Mat4::rotation_y(pick.turn(0.8)))
                .then(&Mat4::rotation_x(-0.25));
            pick.scene.at(pick.middle(), shape)
        },
        move || crystal_glow(pick.color()).times(pick.strength()),
    );
}

// A flower whose petals grow with the option's `step` from a closed bud at the first option.
#[composable]
fn Flower(pick: Pick, index: u32, step: f32) {
    let petals = if index == 0 { 5 } else { 3 + 3 * index };
    let mesh = rememberShape([petals as f32, step], move |[_, step]| {
        if index == 0 {
            flower(petals, 0.22, 2.8)
        } else {
            flower(petals, 0.35 + 0.3 * step, 1.1)
        }
    });
    Glowing(
        mesh,
        move || {
            let shape = scaled(1.15 * pick.side * pick.grow())
                .then(&Mat4::rotation_y(pick.turn(0.35)))
                .then(&Mat4::rotation_x(0.6));
            pick.scene
                .at(pick.middle() + Vec3::new(0.0, pick.side * 0.15, 0.0), shape)
        },
        move || {
            Glow {
                rim: 0.45,
                fill: 0.3,
                ..Glow::glass(pick.color())
            }
            .edged(Edges::Quads, 1.1, 0.16)
            .times(pick.strength())
        },
    );
}

// The beam of light under a chosen gem, and the halo around it.
#[composable]
fn Beam(scene: Scene, middle: impl Fn() -> Vec3 + Copy + 'static, side: f32, motion: Motion) {
    let length = side * 1.1;
    Glowing(
        Arc::clone(&meshes().beam),
        move || {
            let shape = Mat4::scaling(Vec3::new(side * 0.16, length, side * 0.16));
            scene.at(
                middle() + Vec3::new(0.0, length / 2.0 + side * 0.2, 0.0),
                shape,
            )
        },
        move || {
            Glow {
                rim: 0.2,
                fill: 0.5,
                ..Glow::glass(scene.colors.light)
            }
            .faded()
            .times(motion.bloom.get() * scene.shown())
        },
    );
    Halo(scene, middle, side * 0.7, motion);
}

// A soft round glow of the chosen color about the middle of a chosen object.
#[composable]
fn Halo(scene: Scene, middle: impl Fn() -> Vec3 + 'static, radius: f32, motion: Motion) {
    Glowing(
        Arc::clone(&meshes().halo),
        move || scene.at(middle(), scaled(radius)),
        move || {
            Glow::halo(scene.colors.chosen, 2.5).times(0.28 * motion.bloom.get() * scene.shown())
        },
    );
}

// The arc of `ChoiceForm::Arc` across the group: a tick over each option and a crystal knob that
// floats over the chosen one and slides to the next choice.
#[composable]
fn Track(scene: Scene, part: Part, (width, height, count): (f32, f32, u32), motion: Motion) {
    let count = count.max(2);
    // The ticks stand over the middles of the options, which share the width equally
    let first = -width / 2.0 + width / (2.0 * count as f32);
    let half = -first;
    let radius = half / (ARC_SWEEP / 2.0).sin();
    let lift = -WORDS_UNDER / 2.0 + height * 0.26;
    let tilt = Mat4::rotation_x(ARC_LEAN);
    // The point of the arc over `x` across the group, from its middle
    let on_arc = move |x: f32| {
        let angle = (x / radius).clamp(-1.0, 1.0).asin();
        let (sin, cos) = angle.sin_cos();
        tilt.transform_point(Vec3::new(radius * sin, 0.0, radius * (1.0 - cos)))
            + Vec3::new(0.0, lift, -12.0)
    };
    Glowing(
        Arc::clone(&meshes().arc),
        move || {
            let shape = scaled(radius)
                .then(&Mat4::translation(Vec3::new(0.0, 0.0, radius)))
                .then(&tilt);
            scene.at(scene.center(part) + Vec3::new(0.0, lift, -12.0), shape)
        },
        move || {
            Glow {
                rim: 0.8,
                fill: 0.85,
                ..Glow::glass(scene.colors.glass)
            }
            .times(scene.shown())
        },
    );
    for tick in 0..count {
        let x = first + 2.0 * half * tick as f32 / (count - 1) as f32;
        Glowing(
            Arc::clone(&meshes().cube),
            move || {
                let shape = Mat4::scaling(Vec3::new(2.5, 12.0, 2.5));
                scene.at(
                    scene.center(part) + on_arc(x) + Vec3::new(0.0, 4.0, 0.0),
                    shape,
                )
            },
            move || {
                Glow {
                    fill: 0.6,
                    ..Glow::glass(scene.colors.glass)
                }
                .times(scene.shown())
            },
        );
    }
    let knob = move || {
        let x = first + 2.0 * half * motion.value.get();
        scene.center(part) + on_arc(x) + Vec3::new(0.0, -KNOB_FLOAT, 0.0)
    };
    Glowing(
        Arc::clone(&meshes().crystal),
        move || {
            let shape = scaled(30.0).then(&Mat4::rotation_y(scene.time() * 0.9));
            scene.at(knob(), shape)
        },
        move || crystal_glow(scene.colors.chosen).times(motion.lift() * scene.shown()),
    );
    Glowing(
        Arc::clone(&meshes().halo),
        move || scene.at(knob(), scaled(24.0)),
        move || Glow::halo(scene.colors.chosen, 2.5).times(0.3 * scene.shown()),
    );
    // A ring of light under the knob, on the arc
    Glowing(
        Arc::clone(&meshes().ring),
        move || {
            let under = knob() + Vec3::new(0.0, KNOB_FLOAT - 2.0, 0.0);
            let shape = scaled(14.0).then(&Mat4::rotation_y(scene.time() * 0.5));
            scene.at(under, shape)
        },
        move || {
            Glow {
                rim: 0.6,
                fill: 1.0,
                ..Glow::glass(scene.colors.chosen)
            }
            .times(scene.shown())
        },
    );
}

// A stepper's crystal, which holds the chosen option's words, with two rings that orbit it.
#[composable]
fn Stone(scene: Scene, part: Part, [width, height]: [f32; 2], motion: Motion) {
    let side = height.min(width) * 1.3;
    Glowing(
        Arc::clone(&meshes().stone),
        move || {
            let shape = Mat4::scaling(Vec3::new(side * 1.3, side, side * 0.8))
                .then(&Mat4::rotation_y(scene.time() * 0.35));
            scene.at(scene.center(part) + Vec3::new(0.0, 0.0, 10.0), shape)
        },
        move || {
            // A dim body and bright facet edges keep the words over it readable
            Glow {
                fill: 0.03,
                rim: 0.6,
                ..crystal_glow(scene.colors.chosen)
            }
            .times(0.7 * motion.lift() * scene.shown())
        },
    );
    let orbit = (height * 1.15).min(width * 0.46);
    for (tilt, speed) in [(1.18, 0.7), (1.38, -0.5)] {
        let ring = Orbit {
            size: [orbit, orbit, orbit * 0.55],
            tilt,
            speed,
            phase: 0.0,
            swing: [0.18, 0.3],
        };
        Orbiting(scene, part, Arc::clone(&meshes().ring), ring);
    }
}

// A ring that orbits the middle of a part: its size along x, y and z, its tilt toward the viewer,
// and how fast and how far it swings, in radians.
#[derive(Clone, Copy, PartialEq)]
struct Orbit {
    size: [f32; 3],
    tilt: f32,
    speed: f32,
    phase: f32,
    swing: [f32; 2],
}

// A ring of `mesh` that orbits the middle of `part` as `orbit` says, in the theme's chosen colour.
#[composable]
fn Orbiting(scene: Scene, part: Part, mesh: Arc<Mesh>, orbit: Orbit) {
    Glowing(
        mesh,
        move || {
            let time = scene.time() * orbit.speed + orbit.phase;
            let [x, y, z] = orbit.size;
            let shape = Mat4::scaling(Vec3::new(x, y, z))
                .then(&Mat4::rotation_x(orbit.tilt))
                .then(&Mat4::rotation_z(orbit.swing[0] * time.sin()))
                .then(&Mat4::rotation_y(orbit.swing[1] * time.cos()));
            scene.at(scene.center(part), shape)
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

// A round emblem: a disc of rings that faces the viewer, a soft glow in it and two rings that
// orbit it.
#[composable]
fn Emblem(scene: Scene, part: Part, side: f32) {
    let radius = side * 0.46;
    Glowing(
        Arc::clone(&meshes().lens),
        move || {
            let shape = scaled(radius).then(&Mat4::rotation_x(-FRAC_PI_2));
            scene.at(scene.center(part), shape)
        },
        move || {
            Glow {
                fill: 0.9,
                ..Glow::glass(scene.colors.glass)
            }
            .times(scene.shown())
        },
    );
    Glowing(
        Arc::clone(&meshes().halo),
        move || {
            let shape = Mat4::scaling(Vec3::new(radius, radius, radius * 0.3));
            scene.at(scene.center(part) + Vec3::new(0.0, 0.0, 2.0), shape)
        },
        move || Glow::halo(scene.colors.glass, 3.0).times(0.12 * scene.shown()),
    );
    let orbit = radius * 1.3;
    for (tilt, speed, phase) in [(1.25, 0.6, 0.0), (1.05, -0.45, 1.3)] {
        let ring = Orbit {
            size: [orbit; 3],
            tilt,
            speed,
            phase,
            swing: [0.5, 0.35],
        };
        Orbiting(scene, part, Arc::clone(&meshes().orbit), ring);
    }
}

// The projector under the page with its cone of light, the ribbons and the sparks.
#[composable]
fn Stage(scene: Scene, stage: HologramStage) {
    let [width, height] = scene.size;
    if stage.projector {
        Projector(scene, width * 0.22);
    }
    // The ribbons wind about upright lines beside the page, every other one on the left, so none
    // crosses its controls: an object behind the page shows through it. Each turns slowly about
    // its line, so its band seems to climb.
    for ribbon in 0..stage.ribbons {
        let at = ribbon as f32;
        let side = if ribbon % 2 == 0 { 1.0 } else { -1.0 };
        let pair = (ribbon / 2) as f32;
        let radius = width * RIBBON_RADIUS;
        let axis = Vec3::new(
            side * (width / 2.0 + radius * 1.4 + 40.0 * pair),
            height * (0.05 - 0.07 * side) + 60.0 * pair,
            0.0,
        );
        let rise = height * 0.45;
        Glowing(
            Arc::clone(&meshes().helix),
            move || {
                let shape = Mat4::scaling(Vec3::new(radius, rise, radius))
                    .then(&Mat4::rotation_y(scene.time() * 0.35 * side + at));
                scene.at(axis, shape)
            },
            move || {
                Glow {
                    rim: 0.2,
                    fill: 0.06,
                    ..Glow::glass(scene.colors.glass)
                }
                .edged(Edges::Band, 0.9, 0.16)
                .times(scene.shown())
            },
        );
    }
    if stage.particles > 0 {
        let count = stage.particles;
        let field = key(count, || {
            remember(|| Arc::new(sparks(count, (0.56, 0.78), 0.006))).with(Arc::clone)
        });
        Glowing(
            field,
            move || {
                // The field rocks and bobs a little, so no spark drifts over the controls
                let time = scene.time();
                let shape = Mat4::scaling(Vec3::new(width, height + 140.0, width))
                    .then(&Mat4::rotation_y(0.12 * (time * 0.2).sin()));
                scene.at(Vec3::new(0.0, 20.0 + 8.0 * (time * 0.5).sin(), 0.0), shape)
            },
            move || {
                Glow {
                    rim: 0.5,
                    fill: 1.0,
                    ..Glow::glass(scene.colors.light)
                }
                .times(0.8 * scene.shown())
            },
        );
    }
}

// The disc of rings of `radius` under the page, its glow and its cone of light up to the page.
// The disc stays where it stands as the page rises out of it.
#[composable]
fn Projector(scene: Scene, radius: f32) {
    let at = scene.projector();
    Glowing(
        Arc::clone(&meshes().disc),
        move || scaled(radius).then(&Mat4::translation(at)),
        move || {
            Glow {
                rim: 0.2,
                fill: 0.9,
                ..Glow::glass(scene.colors.light)
            }
            .times(0.6 + 0.4 * scene.shown())
        },
    );
    Glowing(
        Arc::clone(&meshes().halo),
        move || {
            Mat4::scaling(Vec3::new(radius * 0.8, 6.0, radius * 0.8)).then(&Mat4::translation(at))
        },
        move || Glow::halo(scene.colors.light, 1.2).times(0.45),
    );
    let reach = PROJECTOR_DROP + 50.0;
    Glowing(
        Arc::clone(&meshes().cone),
        move || {
            let shape = Mat4::scaling(Vec3::new(radius * 0.5, reach * scene.shown(), radius * 0.5));
            shape.then(&Mat4::translation(at))
        },
        move || {
            Glow::halo(scene.colors.light, 2.0)
                .faded()
                .scanned(14.0, scene.time() * 2.0)
                .times(0.45 * scene.shown())
        },
    );
}

// One mesh that `transform` places and `glow` lights while the scene draws.
#[composable]
fn Glowing(
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
            })
            .background(Color::WHITE),
        BoxSpec::default(),
        || {},
    );
}
