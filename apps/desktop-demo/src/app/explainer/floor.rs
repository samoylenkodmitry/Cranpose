// The lamp room's floor: polished concrete that the lamp lights through
// the plate. The floor is a second mesh of the plate's content, so its
// shader reads what the plate lets through. See `floor.wgsl`.

use std::sync::{Arc, OnceLock};

use cranpose_ui::{
    space::{Mat4, Material, Mesh, MeshSurface, Shaded},
    Size,
};

use super::{plate::Openings, room::Room};

// Against the stage's height: past the bottom of the stage.
const FLOOR_DEPTH: f32 = 0.62;
// The lamp's radius against the stage's height: the penumbra on the floor.
const LAMP_RADIUS: f32 = 0.019;
const FLOOR_WGSL: &str = concat!(include_str!("noise.wgsl"), include_str!("floor.wgsl"));

// The floor as a mesh of the plate's picture, lit by the lamp through the
// plate, whose openings are as opaque as `openings` says.
pub(super) fn floor_surface(room: Room, mesh: &Arc<Mesh>, openings: Openings) -> MeshSurface {
    MeshSurface {
        mesh: Arc::clone(mesh),
        transform: Mat4::IDENTITY,
        material: concrete(room, openings),
        casts_shadow: false,
        solid: true,
    }
}

// Every corner of the floor shows the bottom left corner of the plate's
// picture, below its words: a press on the floor lands on no word.
pub(super) fn floor_mesh(size: Size, line: f32) -> Mesh {
    let (left, right) = (-size.width * 0.1, size.width * 1.1);
    let near = -size.height * FLOOR_DEPTH;
    Mesh::new(
        vec![
            [left, line, 0.0],
            [right, line, 0.0],
            [left, line, near],
            [right, line, near],
        ],
        vec![[0.0, 1.0]; 4],
        Some(vec![[0.0, -1.0, 0.0]; 4]),
        vec![0, 2, 1, 1, 2, 3],
    )
    .expect("two whole triangles")
}

fn concrete(room: Room, openings: Openings) -> Material {
    static SHADER: OnceLock<Arc<str>> = OnceLock::new();
    let lamp = room.lamp();
    let colour = room.lamp_colour();
    let mut params = [0.0; Shaded::PARAMS];
    params[..15].copy_from_slice(&[
        room.size.width,
        room.size.height,
        room.u,
        room.floor_line(),
        colour[0],
        colour[1],
        colour[2],
        room.size.height * LAMP_RADIUS,
        lamp.x,
        lamp.y,
        lamp.z,
        openings.bare,
        openings.glass,
        room.eye_distance(),
        room.ahead().y,
    ]);
    Material::Shaded(Shaded {
        shader: Arc::clone(SHADER.get_or_init(|| Arc::from(FLOOR_WGSL))),
        params,
    })
}
