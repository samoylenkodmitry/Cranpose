//! The objects of a choice on a hologram: an object of each option's form, the arc of a choice of
//! `ChoiceForm::Arc` and the crystal of a stepper.
//!
//! Each object stands in its option's cell above the band of the option's words, and at its largest,
//! in full bloom, it stays inside that part of the cell with a margin (`fit`). The options of one
//! group take the scale of the group's largest option, so their sizes keep their order. A chosen
//! object blooms: it grows, turns, glows in the chosen color and stands over a pool of light.

use std::sync::Arc;

use cranpose::{
    Color, composable,
    space::{Mat4, Vec3},
};

use crate::{
    ChoiceForm,
    hologram_material::{Edges, Glow},
    hologram_meshes::flower,
    hologram_scene::{
        ARC_SWEEP, Glowing, Motion, Orbit, Orbiting, Scene, glass_body, meshes, mix, scaled,
    },
    spatial_choice_group::WORDS_UNDER,
    tray_part::Part,
    tray_scene::rememberShape,
};

// How far a chosen object grows
const BLOOM: f32 = 0.18;
// The share of the room above the words that an object takes at most, across and down
const ROOM: f32 = 0.9;
// How far the arc leans back, so it shows as a curve that rises at its ends
const ARC_LEAN: f32 = 0.25;
// How far the arc's knob floats over it, and the knob's side, in dp
const KNOB_FLOAT: f32 = 18.0;
const KNOB: f32 = 34.0;
// How far over the band of words the arc stands, in dp
const ARC_OVER_WORDS: f32 = 8.0;

// How far an object reaches from its middle at a scale of one, at its group's largest option, in
// full bloom and turned any way it turns: half its width, and its reach up and down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Reach {
    pub(crate) half: f32,
    pub(crate) up: f32,
    pub(crate) down: f32,
}

impl Reach {
    // The reach of the objects of `form`. A cube that turns about y shows at most half its
    // diagonal across, and its tilt toward the viewer adds a share of its depth up and down. A gem
    // is wider than high and points down. A flower's far petals rise above it as it tilts.
    pub(crate) fn of(form: ChoiceForm) -> Self {
        let grown = |half: f32, up: f32, down: f32| Self {
            half: half * (1.0 + BLOOM),
            up: up * (1.0 + BLOOM),
            down: down * (1.0 + BLOOM),
        };
        match form {
            ChoiceForm::Cubes => grown(0.71, 0.75, 0.75),
            ChoiceForm::Gems => grown(0.5, 0.4, 0.66),
            ChoiceForm::Petals => grown(0.75, 1.01, 0.2),
            ChoiceForm::Stepper => Self {
                half: 0.8,
                up: 0.45,
                down: 0.45,
            },
            ChoiceForm::Arc => Self {
                half: 0.5,
                up: 0.5,
                down: 0.5,
            },
        }
    }
}

// Where an object stands in its cell and how large it draws: the offset of its middle from the
// cell's middle along y, in dp, and its scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fit {
    pub(crate) lift: f32,
    pub(crate) scale: f32,
}

// The fit of an object of `reach` in a cell of `width` by `height` dp: as large as fits the room
// above the words with a margin, with its reach centered in that room.
pub(crate) fn fit(reach: Reach, [width, height]: [f32; 2]) -> Fit {
    let room = (height - WORDS_UNDER).max(1.0);
    let scale = (ROOM * width / (2.0 * reach.half)).min(ROOM * room / (reach.up + reach.down));
    Fit {
        lift: -WORDS_UNDER / 2.0 + (reach.up - reach.down) / 2.0 * scale,
        scale,
    }
}

// How far below its cell's middle a stepper's arrows stand, in dp: as high as its crystal, the
// middle of the room above its words.
pub(crate) const STEPPER_LIFT: f32 = -WORDS_UNDER / 2.0;

// What the object of one option reads: the scene, its part, its animations and its fit.
#[derive(Clone, Copy, PartialEq)]
struct Pick {
    scene: Scene,
    part: Part,
    motion: Motion,
    fit: Fit,
}

impl Pick {
    fn middle(self) -> Vec3 {
        self.scene.center(self.part) + Vec3::new(0.0, self.fit.lift, 0.0) + self.motion.sunk()
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
        (0.6 + 0.4 * self.motion.bloom.get()) * self.motion.lift() * self.scene.shown()
    }

    fn grow(self) -> f32 {
        1.0 + BLOOM * self.motion.bloom.get()
    }
}

// The object of one option of a choice group, with a pool of light under it while it is chosen.
#[composable]
pub(crate) fn ChoiceObject(
    scene: Scene,
    part: Part,
    (form, index, count): (ChoiceForm, u32, u32),
    motion: Motion,
) {
    let rect = part.rect.get();
    let step = index as f32 / count.saturating_sub(1).max(1) as f32;
    let reach = Reach::of(form);
    let fit = fit(reach, [rect.width, rect.height]);
    let pick = Pick {
        scene,
        part,
        motion,
        fit,
    };
    match form {
        ChoiceForm::Cubes => Cube(pick, 0.45 + 0.55 * step, index),
        ChoiceForm::Gems => Gem(pick),
        ChoiceForm::Petals => Flower(pick, index, step),
        // The arc's track draws its ticks and knob, and a stepper draws its crystal and arrows
        ChoiceForm::Arc | ChoiceForm::Stepper => return,
    }
    if part.on.get() {
        let below = reach.down * fit.scale * 0.85;
        let radius = reach.half * fit.scale * 0.7;
        Pool(
            scene,
            move || pick.middle() + Vec3::new(0.0, below, 0.0),
            radius,
            motion,
        );
        Halo(scene, move || pick.middle(), reach.half * fit.scale, motion);
        if form == ChoiceForm::Gems {
            Beam(scene, move || pick.middle(), (below, radius), motion);
        }
    }
}

// A cube of glass whose edge is `share` of the largest, whose faces carry a finer grid for a later
// option.
#[composable]
fn Cube(pick: Pick, share: f32, index: u32) {
    let cells = (1 + index.min(4).div_ceil(2)) as f32;
    Glowing(
        Arc::clone(&meshes().cube),
        move || {
            let shape = scaled(pick.fit.scale * share * pick.grow())
                .then(&Mat4::rotation_y(pick.turn(0.6)))
                .then(&Mat4::rotation_x(-0.42));
            pick.scene.at(pick.middle(), shape)
        },
        move || {
            Glow {
                rim: 0.5,
                fill: 0.14,
                ..Glow::glass(pick.color())
            }
            .edged(Edges::Quads, 1.3, 0.05)
            .gridded(cells)
            .lit_inside(0.65, 0.6)
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
            let shape = scaled(pick.fit.scale * pick.grow())
                .then(&Mat4::rotation_y(pick.turn(0.8)))
                .then(&Mat4::rotation_x(-0.25));
            pick.scene.at(pick.middle(), shape)
        },
        move || glass_body(pick.color(), Edges::Facets).times(pick.strength()),
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
            let shape = scaled(pick.fit.scale * pick.grow())
                .then(&Mat4::rotation_y(pick.turn(0.35)))
                .then(&Mat4::rotation_x(0.6));
            pick.scene.at(pick.middle(), shape)
        },
        move || {
            Glow {
                rim: 0.55,
                fill: 0.34,
                ..Glow::glass(pick.color())
            }
            .edged(Edges::Quads, 1.1, 0.16)
            .lit_inside(0.5, 0.35)
            .times(pick.strength())
        },
    );
}

// A pool of light of `radius` dp under a chosen object: a flat disc that faces up a little toward
// the viewer, brightest in its middle, with a ring at its edge.
#[composable]
fn Pool(scene: Scene, at: impl Fn() -> Vec3 + Copy + 'static, radius: f32, motion: Motion) {
    let lying = Mat4::rotation_x(-0.35);
    Glowing(
        Arc::clone(&meshes().plate),
        move || {
            let shape = Mat4::scaling(Vec3::new(radius, 1.0, radius)).then(&lying);
            scene.at(at(), shape)
        },
        move || {
            Glow {
                rim: 0.0,
                fill: 0.1,
                ..Glow::glass(scene.colors.chosen)
            }
            .lit_inside(0.6, 0.8)
            .times(motion.bloom.get() * scene.shown())
        },
    );
    Glowing(
        Arc::clone(&meshes().ring),
        move || {
            let shape = Mat4::scaling(Vec3::new(radius, radius, radius))
                .then(&Mat4::rotation_y(scene.time() * 0.5))
                .then(&lying);
            scene.at(at(), shape)
        },
        move || {
            Glow {
                rim: 0.6,
                fill: 1.0,
                ..Glow::glass(scene.colors.chosen)
            }
            .times(0.8 * motion.bloom.get() * scene.shown())
        },
    );
}

// The beam of light that rises from the pool under a chosen gem, `rise` dp under its middle, to
// the gem.
#[composable]
fn Beam(
    scene: Scene,
    middle: impl Fn() -> Vec3 + Copy + 'static,
    (rise, radius): (f32, f32),
    motion: Motion,
) {
    Glowing(
        Arc::clone(&meshes().beam),
        move || {
            let shape = Mat4::scaling(Vec3::new(radius * 0.35, rise, radius * 0.35));
            scene.at(middle() + Vec3::new(0.0, rise / 2.0, 0.0), shape)
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
}

// A soft round glow of the chosen color about the middle of a chosen object.
#[composable]
fn Halo(scene: Scene, middle: impl Fn() -> Vec3 + 'static, radius: f32, motion: Motion) {
    Glowing(
        Arc::clone(&meshes().halo),
        move || scene.at(middle(), scaled(radius)),
        move || {
            Glow::halo(scene.colors.chosen, 2.5).times(0.24 * motion.bloom.get() * scene.shown())
        },
    );
}

// The arc of `ChoiceForm::Arc` across the group: a tick over each option and a crystal knob that
// floats over the chosen one and slides to the next choice, over a ring of light.
#[composable]
pub(crate) fn Track(scene: Scene, part: Part, count: u32, motion: Motion) {
    let rect = part.rect.get();
    let (width, height) = (rect.width, rect.height);
    let count = count.max(2);
    // The ticks stand over the middles of the options, which share the width equally
    let first = -width / 2.0 + width / (2.0 * count as f32);
    let half = -first;
    let radius = half / (ARC_SWEEP / 2.0).sin();
    let lift = height / 2.0 - WORDS_UNDER - ARC_OVER_WORDS;
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
                rim: 0.9,
                fill: 0.9,
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
                    scene.center(part) + on_arc(x) + Vec3::new(0.0, 7.0, 0.0),
                    shape,
                )
            },
            move || {
                Glow {
                    fill: 0.7,
                    ..Glow::glass(scene.colors.glass)
                }
                .times(scene.shown())
            },
        );
    }
    let knob = move || {
        let x = first + 2.0 * half * motion.value.get();
        scene.center(part) + on_arc(x) + Vec3::new(0.0, -KNOB_FLOAT - KNOB / 2.0, 0.0)
    };
    Glowing(
        Arc::clone(&meshes().crystal),
        move || {
            let shape = scaled(KNOB).then(&Mat4::rotation_y(scene.time() * 0.9));
            scene.at(knob(), shape)
        },
        move || glass_body(scene.colors.chosen, Edges::Facets).times(motion.lift() * scene.shown()),
    );
    Glowing(
        Arc::clone(&meshes().halo),
        move || scene.at(knob(), scaled(KNOB * 0.75)),
        move || Glow::halo(scene.colors.chosen, 2.5).times(0.3 * scene.shown()),
    );
    // A ring of light under the knob, on the arc
    Glowing(
        Arc::clone(&meshes().ring),
        move || {
            let under = knob() + Vec3::new(0.0, KNOB_FLOAT + KNOB / 2.0 - 3.0, 0.0);
            let shape = scaled(16.0).then(&Mat4::rotation_y(scene.time() * 0.5));
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

// A stepper's crystal over the chosen option's words, with two rings that orbit it.
#[composable]
pub(crate) fn Stone(scene: Scene, part: Part, motion: Motion) {
    let rect = part.rect.get();
    let fit = fit(Reach::of(ChoiceForm::Stepper), [rect.width, rect.height]);
    let side = fit.scale;
    let middle = move || scene.center(part) + Vec3::new(0.0, fit.lift, 0.0);
    Glowing(
        Arc::clone(&meshes().stone),
        move || {
            let shape = Mat4::scaling(Vec3::new(side * 1.3, side, side * 0.8))
                .then(&Mat4::rotation_y(scene.time() * 0.35));
            scene.at(middle() + motion.sunk(), shape)
        },
        move || {
            glass_body(scene.colors.chosen, Edges::Facets)
                .times(0.85 * motion.lift() * scene.shown())
        },
    );
    let orbit = side * 0.78;
    for (tilt, speed) in [(1.18, 0.7), (1.38, -0.5)] {
        let ring = Orbit {
            size: [orbit, orbit, orbit * 0.55],
            tilt,
            speed,
            phase: 0.0,
            swing: [0.18, 0.3],
        };
        Orbiting(scene, middle(), Arc::clone(&meshes().ring), ring);
    }
}

#[cfg(test)]
#[path = "tests/hologram_objects_tests.rs"]
mod tests;
