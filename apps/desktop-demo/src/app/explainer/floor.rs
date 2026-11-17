// The lamp room's floor: polished concrete that the lamp lights through
// the holes of the plate. See `floor.wgsl`.

use std::sync::{Arc, OnceLock};

use cranpose_core::rememberKeyed;
use cranpose_ui::{
    composable,
    space::{Mat4, Material, Mesh, MeshSurface, Shaded, TextureEdges},
    Box, BoxSpec, Color, Modifier, Size,
};

use super::room::Room;

// Against the stage's height: past the bottom of the stage.
const FLOOR_DEPTH: f32 = 0.62;
const CONCRETE: Color = Color(0.12, 0.10, 0.09, 1.0);
const FLOOR_WGSL: &str = concat!(include_str!("noise.wgsl"), include_str!("floor.wgsl"));

#[composable(no_skip)]
pub(super) fn Floor(room: Room) {
    let size = room.size;
    let line = room.floor_line();
    let mesh = rememberKeyed((size.width.to_bits(), size.height.to_bits()), |_| {
        Arc::new(floor_mesh(size, line))
    });
    Box(
        Modifier::empty()
            .required_size(Size::new(16.0, 16.0))
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&mesh),
                transform: Mat4::IDENTITY,
                material: concrete(room),
                casts_shadow: false,
                solid: true,
            })
            .background(CONCRETE),
        BoxSpec::default(),
        || {},
    );
}

fn floor_mesh(size: Size, line: f32) -> Mesh {
    let (left, right) = (-size.width * 0.1, size.width * 1.1);
    let near = -size.height * FLOOR_DEPTH;
    Mesh::new(
        vec![
            [left, line, 0.0],
            [right, line, 0.0],
            [left, line, near],
            [right, line, near],
        ],
        vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
        Some(vec![[0.0, -1.0, 0.0]; 4]),
        vec![0, 2, 1, 1, 2, 3],
    )
    .expect("two whole triangles")
    .with_texture_edges(TextureEdges::Outline)
}

fn concrete(room: Room) -> Material {
    static SHADER: OnceLock<Arc<str>> = OnceLock::new();
    let lamp = room.lamp();
    let colour = room.lamp_colour();
    let mut params = [0.0; Shaded::PARAMS];
    params[..11].copy_from_slice(&[
        room.size.width,
        room.size.height,
        room.u,
        room.floor_line(),
        colour[0],
        colour[1],
        colour[2],
        0.0,
        lamp.x,
        lamp.y,
        lamp.z,
    ]);
    Material::Shaded(Shaded {
        shader: Arc::clone(SHADER.get_or_init(|| Arc::from(FLOOR_WGSL))),
        params,
    })
}
