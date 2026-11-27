//! The body of a `SpatialGem` on a hologram: an octahedron of glowing glass that turns, with a halo
//! about it.

use std::{f32::consts::TAU, sync::Arc};

use cranpose::{Color, composable, space::Mat4};

use crate::{
    hologram_material::{Edges, Glow},
    hologram_scene::{Glowing, Motion, Scene, glass_body, meshes, scaled},
    tray_part::Part,
};

// How fast the gem turns about its upright axis, in radians a second: one turn in about 10 s
const TURN_SPEED: f32 = 0.6;
// The time of one swell and fade of a pulsing gem's glow, in seconds
const PULSE_SECONDS: f32 = 1.6;
// How far a pulsing gem's glow swells above its steady glow and falls below it
const PULSE_DEPTH: f32 = 0.45;
// The gem's height as a share of its place's smaller side: its corners stay inside the place as it
// turns and sinks
const FILL: f32 = 0.86;
// The gem leans its top toward the viewer a little, so its upper facets face the eye
const LEAN: f32 = -0.22;
// The halo's radius as a share of the gem's height, and its strength
const HALO: f32 = 0.62;
const HALO_STRENGTH: f32 = 0.3;

// A gem of `color` that fills a place of `side` dp, at the middle of `part`'s place.
#[composable]
pub(crate) fn GemBody(scene: Scene, part: Part, (side, color): (f32, Color), motion: Motion) {
    let height = side * FILL;
    let middle = move || scene.center(part) + motion.sunk();
    // A pulsing gem's glow swells and fades about its steady glow. `bloom` eases the pulse in and
    // out as the app turns it on and off.
    let strength = move || {
        let wave = (scene.time() * TAU / PULSE_SECONDS).sin();
        (1.0 + PULSE_DEPTH * motion.bloom.get() * wave) * motion.lift() * scene.shown()
    };
    Glowing(
        Arc::clone(&meshes().octahedron),
        move || {
            let shape = scaled(height)
                .then(&Mat4::rotation_y(scene.time() * TURN_SPEED))
                .then(&Mat4::rotation_x(LEAN));
            scene.at(middle(), shape)
        },
        move || glass_body(color, Edges::Facets).times(strength()),
    );
    Glowing(
        Arc::clone(&meshes().halo),
        move || scene.at(middle(), scaled(height * HALO)),
        move || Glow::halo(color, 2.5).times(HALO_STRENGTH * strength()),
    );
}
