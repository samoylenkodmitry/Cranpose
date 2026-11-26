//! What stands around a hologram's controls: the projector under the page, whose middle is the
//! page's handle, the ribbons beside it and the sparks. Nothing of it stands over the controls.
//!
//! The stage stays close to the page, so the box that holds the page and its stage
//! (`hologram_extent`), which the projection layer covers, stays small.

use std::sync::Arc;

use cranpose::{
    composable,
    space::{Mat4, Vec3},
};
use cranpose_core::{key, remember};

use crate::{
    HologramStage,
    hologram_material::{Edges, Glow},
    hologram_meshes::sparks,
    hologram_scene::{Glowing, HANDLE_DROP, Scene, glass_body, meshes, scaled},
};

// The radius of the projector's disc, as a share of the page's width
const PROJECTOR_RADIUS: f32 = 0.2;
// The side of the crystal in the projector's middle, which is the page's handle, in dp
const CORE: f32 = 30.0;
// How far the light of the projector rises over the page's bottom edge, in dp
const LIGHT_INTO_PAGE: f32 = 90.0;
// The radius of a ribbon's turns as a share of the page's width
const RIBBON_RADIUS: f32 = 0.075;
// How far a ribbon's line stands out of the page's side, as a share of its radius
const RIBBON_OUT: f32 = 1.25;
// The share of the page's height that a ribbon winds over
const RIBBON_RISE: f32 = 0.42;
// The sparks' distance from the page's middle, as a share of the page's width, how far each lies
// from the page's plane, in radians about its side, and the height of their field beyond the page's
const SPARKS_REACH: (f32, f32) = (0.5, 0.62);
const SPARKS_SPREAD: f32 = 0.25;
const SPARKS_RISE: f32 = 120.0;
// The room that an object takes beyond its control's place, as it blooms, floats or orbits, in dp
const OBJECT_REACH: f32 = 40.0;

// The half sizes, in dp, of the box about the page's middle that holds every object of a page of
// `size` dp with `stage` around it, as the projection layer covers it. A movable page's handle
// stands under it.
pub(crate) fn hologram_extent(
    [width, height]: [f32; 2],
    (stage, movable): (HologramStage, bool),
) -> [f32; 3] {
    let mut extent = [
        width / 2.0 + OBJECT_REACH,
        height / 2.0 + OBJECT_REACH,
        OBJECT_REACH,
    ];
    let mut reach = |x: f32, y: f32, z: f32| {
        extent[0] = extent[0].max(x);
        extent[1] = extent[1].max(y);
        extent[2] = extent[2].max(z);
    };
    if movable || stage.projector {
        reach(CORE, height / 2.0 + HANDLE_DROP + CORE, CORE);
    }
    if stage.projector {
        let radius = width * PROJECTOR_RADIUS;
        reach(radius, height / 2.0 + HANDLE_DROP + 10.0, radius);
    }
    if stage.ribbons > 0 {
        let radius = width * RIBBON_RADIUS;
        let pairs = ((stage.ribbons - 1) / 2) as f32;
        reach(
            width / 2.0 + radius * (RIBBON_OUT + 1.0) + 40.0 * pairs,
            height * (0.12 + RIBBON_RISE / 2.0) + 60.0 * pairs + 20.0,
            radius,
        );
    }
    if stage.particles > 0 {
        // The field rocks by 0.12 radians and bobs by 8 dp
        let outer = SPARKS_REACH.1 * width;
        reach(
            outer,
            (height + SPARKS_RISE) / 2.0 + 30.0,
            outer * (SPARKS_SPREAD + 0.12).sin() + 10.0,
        );
    }
    extent
}

// The projector under the page with its light, the handle's crystal, the ribbons and the sparks.
#[composable]
pub(crate) fn Stage(scene: Scene, stage: HologramStage) {
    let [width, height] = scene.size;
    if stage.projector {
        Projector(scene, width * PROJECTOR_RADIUS);
    }
    if stage.projector || scene.movable {
        HandleCore(scene);
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
            side * (width / 2.0 + radius * RIBBON_OUT + 40.0 * pair),
            height * (0.05 - 0.07 * side) + 60.0 * pair,
            0.0,
        );
        let rise = height * RIBBON_RISE;
        Glowing(
            Arc::clone(&meshes().helix),
            move || {
                let shape = Mat4::scaling(Vec3::new(radius, rise, radius))
                    .then(&Mat4::rotation_y(scene.time() * 0.35 * side + at));
                scene.at(axis, shape)
            },
            move || {
                Glow {
                    rim: 0.45,
                    fill: 0.1,
                    ..Glow::glass(scene.colors.glass)
                }
                .edged(Edges::Band, 1.0, 0.16)
                .times(scene.shown())
            },
        );
    }
    if stage.particles > 0 {
        let count = stage.particles;
        let field = key(count, || {
            remember(|| Arc::new(sparks(count, SPARKS_REACH, SPARKS_SPREAD, 0.006)))
                .with(Arc::clone)
        });
        Glowing(
            field,
            move || {
                // The field rocks and bobs a little, so no spark drifts over the controls
                let time = scene.time();
                let shape = Mat4::scaling(Vec3::new(width, height + SPARKS_RISE, width))
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

// The projector's dim disc of rings of `radius` under the page and its light, which rises from
// the disc into the page's bottom. The disc stays where it stands as the page rises out of it.
#[composable]
fn Projector(scene: Scene, radius: f32) {
    let at = scene.projector();
    Glowing(
        Arc::clone(&meshes().disc),
        move || scaled(radius).then(&Mat4::translation(at)),
        move || {
            Glow {
                rim: 0.3,
                fill: 0.35,
                ..Glow::glass(scene.colors.light)
            }
            .times(0.35 + 0.15 * scene.shown())
        },
    );
    Glowing(
        Arc::clone(&meshes().plate),
        move || {
            Mat4::scaling(Vec3::new(radius * 0.92, 1.0, radius * 0.92))
                .then(&Mat4::translation(at + Vec3::new(0.0, 1.0, 0.0)))
        },
        move || {
            Glow {
                rim: 0.0,
                fill: 0.05,
                ..Glow::glass(scene.colors.light)
            }
            .lit_inside(0.25, 0.7)
        },
    );
    let reach = HANDLE_DROP + LIGHT_INTO_PAGE;
    Glowing(
        Arc::clone(&meshes().cone),
        move || {
            let shape = Mat4::scaling(Vec3::new(radius * 0.4, reach * scene.shown(), radius * 0.4));
            shape.then(&Mat4::translation(at))
        },
        move || {
            Glow::halo(scene.colors.light, 1.6)
                .faded()
                .scanned(12.0, -scene.time() * 3.0)
                .times(0.5 * scene.shown())
        },
    );
}

// The crystal in the projector's middle: the page's handle, which the user grabs to carry the
// page. It glows in the light's color and stands still.
#[composable]
fn HandleCore(scene: Scene) {
    let at = scene.projector();
    Glowing(
        Arc::clone(&meshes().crystal),
        move || {
            scaled(CORE)
                .then(&Mat4::rotation_y(0.4))
                .then(&Mat4::translation(at))
        },
        move || glass_body(scene.colors.light, Edges::Facets).times(0.8 + 0.2 * scene.shown()),
    );
    Glowing(
        Arc::clone(&meshes().halo),
        move || scaled(CORE * 0.8).then(&Mat4::translation(at)),
        move || Glow::halo(scene.colors.light, 2.0).times(0.3),
    );
}

#[cfg(test)]
#[path = "tests/hologram_stage_tests.rs"]
mod tests;
