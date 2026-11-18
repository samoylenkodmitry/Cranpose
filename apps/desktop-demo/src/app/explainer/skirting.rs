// The lamp room's skirting board, along the foot of the wall. See
// `skirting.wgsl`.

use std::sync::{Arc, OnceLock};

use cranpose_core::remember;
use cranpose_ui::{
    composable,
    space::{Mat4, Material, Mesh, MeshSurface, Shaded, Vec3},
    Box, BoxSpec, Color, Modifier, Size,
};

use super::room::Room;

// In units.
const HEIGHT: f32 = 43.0;
const DEPTH: f32 = 7.0;
const PAINT: Color = Color(0.10, 0.08, 0.07, 1.0);

#[composable(no_skip)]
pub(super) fn Skirting(room: Room) {
    let block = remember(|| Arc::new(Mesh::cuboid())).with(Arc::clone);
    let (u, width) = (room.u, room.size.width);
    let middle = Vec3::new(
        width * 0.5,
        room.floor_line() - HEIGHT * u * 0.5,
        -DEPTH * u * 0.5,
    );
    Box(
        Modifier::empty()
            .required_size(Size::new(16.0, 16.0))
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&block),
                transform: Mat4::scaling(Vec3::new(width * 1.3, HEIGHT * u, DEPTH * u))
                    .then(&Mat4::translation(middle)),
                material: paint(room),
                casts_shadow: false,
                solid: true,
            })
            .background(PAINT),
        BoxSpec::default(),
        || {},
    );
}

fn paint(room: Room) -> Material {
    static SHADER: OnceLock<Arc<str>> = OnceLock::new();
    let mut params = [0.0; Shaded::PARAMS];
    params[..3].copy_from_slice(&room.lamp_colour());
    Material::Shaded(Shaded {
        shader: Arc::clone(
            SHADER.get_or_init(|| Arc::from(cranpose_ui::include_wgsl!("skirting.wgsl"))),
        ),
        params,
    })
}
