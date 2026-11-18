// The melt's wall: pink paint on plaster, lit from the upper left. See
// `plaster.wgsl`.

use std::sync::{Arc, OnceLock};

use cranpose_core::remember;
use cranpose_ui::{
    composable,
    space::{Mat4, Material, Mesh, MeshSurface, Shaded, Vec3},
    Box, BoxSpec, Color, Modifier, Size,
};

// Linear.
pub(super) const WALL_TOP: [f32; 3] = [0.9464, 0.5462, 0.4799];
pub(super) const WALL_BOTTOM: [f32; 3] = [0.9049, 0.4131, 0.3509];
const PAINT: Color = Color(0.97, 0.72, 0.67, 1.0);
const PLASTER_WGSL: &str = cranpose_ui::include_wgsl!("noise.wgsl", "plaster.wgsl");

#[composable(no_skip)]
pub(super) fn PlasterWall(size: Size, u: f32) {
    let plane = remember(|| Arc::new(Mesh::plane())).with(Arc::clone);
    let material = plaster(size, u);
    Box(
        Modifier::empty()
            .required_size(size)
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&plane),
                transform: Mat4::scaling(Vec3::new(size.width, size.height, 1.0)),
                material: material.clone(),
                casts_shadow: false,
                solid: true,
            })
            .background(PAINT),
        BoxSpec::default(),
        || {},
    );
}

fn plaster(size: Size, u: f32) -> Material {
    static SHADER: OnceLock<Arc<str>> = OnceLock::new();
    let mut params = [0.0; Shaded::PARAMS];
    params[..3].copy_from_slice(&[size.width, size.height, u]);
    params[4..7].copy_from_slice(&WALL_TOP);
    params[8..11].copy_from_slice(&WALL_BOTTOM);
    Material::Shaded(Shaded {
        shader: Arc::clone(SHADER.get_or_init(|| Arc::from(PLASTER_WGSL))),
        params,
    })
}
