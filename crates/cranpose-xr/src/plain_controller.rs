//! The plain controller that Cranpose builds in code, for a runtime that has no model of its own.
//!
//! The parts are solids from `Mesh`, each with one flat color, in the glTF axes of the runtime's
//! models: meters about the controller's grip, with x to the right, y up and z toward the user. The
//! handle runs along z, the head stands in front of it along -z, the thumbstick and two buttons are
//! domes on top of the head, the trigger hangs under its front and a small light blue tip marks the
//! front.

use std::{
    rc::Rc,
    sync::{Arc, OnceLock},
};

use cranpose::{
    Box, BoxSpec, Color, GraphicsLayer, MutableState, Size, composable,
    space::{Mat4, Mesh, Vec3},
};
use cranpose_core::{key, remember};

use crate::surface_ring::SurfaceRing;

// The side in pixels of the flat swatch that each part shows on its surface.
const SWATCH: f32 = 4.0;

// What a part is made of.
#[derive(Clone, Copy)]
enum Solid {
    Cuboid,
    // An ellipsoid: a flat one makes a dome. `Mesh::cylinder` is an open tube, so no part uses it.
    Sphere,
}

// One part of the controller in meters: its box and its color.
struct Spec {
    solid: Solid,
    center: [f32; 3],
    // The width, height and depth of the part's box
    size: [f32; 3],
    color: [f32; 3],
}

const BODY: [f32; 3] = [0.13, 0.13, 0.15];
const HEAD: [f32; 3] = [0.21, 0.21, 0.24];
const KEY: [f32; 3] = [0.84, 0.84, 0.9];

const SPECS: [Spec; 7] = [
    // The handle that the hand closes on
    Spec {
        solid: Solid::Cuboid,
        center: [0.0, -0.002, 0.02],
        size: [0.032, 0.036, 0.1],
        color: BODY,
    },
    // The head in front of the handle
    Spec {
        solid: Solid::Cuboid,
        center: [0.0, 0.016, -0.055],
        size: [0.062, 0.02, 0.05],
        color: HEAD,
    },
    // The thumbstick's dome, which rises from the head's top
    Spec {
        solid: Solid::Sphere,
        center: [0.0, 0.026, -0.05],
        size: [0.016, 0.01, 0.016],
        color: [0.6, 0.6, 0.65],
    },
    // The trigger under the head's front
    Spec {
        solid: Solid::Cuboid,
        center: [0.0, -0.0045, -0.078],
        size: [0.012, 0.022, 0.01],
        color: [0.45, 0.45, 0.5],
    },
    // The tip, which marks the front
    Spec {
        solid: Solid::Sphere,
        center: [0.0, 0.016, -0.086],
        size: [0.012, 0.012, 0.012],
        color: [0.15, 0.65, 1.0],
    },
    // The two face buttons
    Spec {
        solid: Solid::Sphere,
        center: [0.014, 0.026, -0.068],
        size: [0.008, 0.004, 0.008],
        color: KEY,
    },
    Spec {
        solid: Solid::Sphere,
        center: [-0.014, 0.026, -0.068],
        size: [0.008, 0.004, 0.008],
        color: KEY,
    },
];

// A part ready to draw.
pub(crate) struct Part {
    pub(crate) mesh: Arc<Mesh>,
    // The map from the mesh's own space to the model's meters
    pub(crate) place: Mat4,
    color: [f32; 3],
}

// The parts, made once: a mesh has an identity that the renderer keeps its buffers by, so the
// parts share their meshes among all controllers.
pub(crate) fn parts() -> &'static [Part] {
    static PARTS: OnceLock<Vec<Part>> = OnceLock::new();
    PARTS.get_or_init(|| {
        let cuboid = Arc::new(Mesh::cuboid());
        let sphere = Arc::new(Mesh::sphere(16, 10));
        SPECS
            .iter()
            .map(|spec| {
                let mesh = match spec.solid {
                    Solid::Cuboid => &cuboid,
                    Solid::Sphere => &sphere,
                };
                Part {
                    mesh: Arc::clone(mesh),
                    place: place(spec),
                    color: spec.color,
                }
            })
            .collect()
    })
}

// The map from a unit mesh to the part's box. The cube has sides of one and the sphere a radius of
// one, so the sphere scales by half the box.
fn place(spec: &Spec) -> Mat4 {
    let [x, y, z] = spec.size;
    let [cx, cy, cz] = spec.center;
    let factors = match spec.solid {
        Solid::Cuboid => Vec3::new(x, y, z),
        Solid::Sphere => Vec3::new(x / 2.0, y / 2.0, z / 2.0),
    };
    Mat4::scaling(factors).then(&Mat4::translation(Vec3::new(cx, cy, cz)))
}

// The controller's parts at `map`, the map from the model's meters to the scene. The map is a state
// that the frame loop moves with the controller. The draw reads it, so a controller that moves
// composes nothing.
#[composable]
pub(crate) fn PlainController(map: MutableState<Mat4>) {
    for index in 0..parts().len() {
        key(index, move || PlainPart(index, map));
    }
}

#[composable]
fn PlainPart(index: usize, map: MutableState<Mat4>) {
    let ring = remember(|| Rc::new(SurfaceRing::new())).with(Rc::clone);
    let part = &parts()[index];
    let (mesh, place) = (Arc::clone(&part.mesh), part.place);
    let [red, green, blue] = part.color;
    Box(
        cranpose::Modifier::empty()
            .required_size(Size::new(SWATCH, SWATCH))
            .graphics_layer(move || {
                let fit = map.get();
                let space = ring.space(std::slice::from_ref(&mesh), |_| place.then(&fit));
                GraphicsLayer {
                    space: Some(space),
                    clip: true,
                    ..GraphicsLayer::DEFAULT
                }
            })
            .background(Color(red, green, blue, 1.0)),
        BoxSpec::default(),
        || {},
    );
}
