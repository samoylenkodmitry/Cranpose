// The lamp room's back wall: a plaster plate with the live words cut
// through it. See `plate.wgsl`. The floor is a second mesh of the plate's
// picture, so that it reads the light the plate lets through.

use std::{
    rc::Rc,
    sync::{Arc, OnceLock},
};

use cranpose_core::{remember, rememberKeyed};
use cranpose_ui::{
    composable,
    space::{Mat4, Material, Mesh, MeshSurface, MeshSurfaces, Vec3},
    Box, BoxSpec, CompositingStrategy, GraphicsLayer, Modifier, RenderEffect, RuntimeShader,
};
use cranpose_ui_graphics::{SubstrateSpec, RUNTIME_SHADER_PRELUDE_WGSL};

use super::{
    floor::{floor_mesh, floor_surface},
    room::Room,
};

// Gaussian sigmas in units: the reaches at which the holes' light spills
// over the plaster.
const SIGMAS: [f32; 3] = [3.0, 16.0, 60.0];
// Against the headline's type size.
const THICKNESS: f32 = 0.19;
// How opaque the plate's picture is in an opening: the floor reads the
// rest as how open the plate is there, and a lower alpha as glass.
const OPENINGS: Openings = Openings {
    bare: 0.45,
    glass: 0.30,
};
const PLATE_WGSL: &str = cranpose_ui::include_wgsl!("noise.wgsl", "plate.wgsl");

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Cuts {
    pub head_size: f32,
    pub head_bottom: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Openings {
    pub bare: f32,
    pub glass: f32,
}

// The wall is the whole stage high: the floor hides the part below the
// floor line, where the picture holds what the floor reads.
#[composable(no_skip)]
pub(super) fn Plate(room: Room, (cuts, density): (Cuts, f32), words: impl Fn() + 'static) {
    let plane = remember(|| Arc::new(Mesh::plane())).with(Arc::clone);
    let floor = rememberKeyed(
        (room.size.width.to_bits(), room.size.height.to_bits()),
        |_| Arc::new(floor_mesh(room.size, room.floor_line())),
    );
    let words = Rc::new(words);
    let size = room.size;
    Box(
        Modifier::empty()
            .required_size(size)
            .mesh_surfaces(move || {
                let wall = MeshSurface {
                    mesh: Arc::clone(&plane),
                    transform: Mat4::scaling(Vec3::new(size.width, size.height, 1.0)),
                    material: Material::Emissive,
                    casts_shadow: false,
                    solid: true,
                };
                MeshSurfaces::new(wall, vec![floor_surface(room, &floor, OPENINGS)])
            }),
        BoxSpec::default(),
        move || {
            let words = Rc::clone(&words);
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .graphics_layer(move || plate_layer(room, (cuts, density))),
                BoxSpec::default(),
                move || words(),
            );
        },
    );
}

fn plate_layer(room: Room, (cuts, density): (Cuts, f32)) -> GraphicsLayer {
    let u = room.u;
    let mut shader = RuntimeShader::from_shared_source(source());
    shader.set_substrates(&SIGMAS.map(|sigma| SubstrateSpec::Blur {
        radius_px: 2.0 * sigma * u * density,
    }));
    let [red, green, blue] = room.lamp_colour();
    let (hot, ahead, image) = (room.hot(), room.ahead(), room.lamp_image());
    let values = [
        (0, room.size.width),
        (1, room.floor_line()),
        (2, u),
        (3, density),
        (4, red),
        (5, green),
        (6, blue),
        (7, room.lamp_on()),
        (8, hot.x),
        (9, hot.y),
        (10, ahead.x),
        (11, ahead.y),
        (12, room.eye_distance()),
        (13, cuts.head_size * THICKNESS),
        (14, cuts.head_bottom),
        (15, OPENINGS.bare),
        (16, SIGMAS[0] * u),
        (17, SIGMAS[1] * u),
        (18, SIGMAS[2] * u),
        (19, room.size.height),
        (20, image.x),
        (21, image.y),
        (22, OPENINGS.glass),
    ];
    for (index, value) in values {
        shader.set_float(index, value);
    }
    GraphicsLayer {
        render_effect: Some(RenderEffect::runtime_shader(shader)),
        compositing_strategy: CompositingStrategy::Offscreen,
        ..GraphicsLayer::default()
    }
}

fn source() -> Arc<str> {
    static SOURCE: OnceLock<Arc<str>> = OnceLock::new();
    Arc::clone(
        SOURCE.get_or_init(|| Arc::from(format!("{RUNTIME_SHADER_PRELUDE_WGSL}{PLATE_WGSL}"))),
    )
}
