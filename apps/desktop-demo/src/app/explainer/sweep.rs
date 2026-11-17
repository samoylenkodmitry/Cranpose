// The green paper sweep the figure stands on: a table that curves up into a
// wall, lit through a window whose bars show in its light, which the board
// and the print cast their shadows on.

use std::sync::{Arc, OnceLock};

use cranpose_core::remember;
use cranpose_ui::{
    composable,
    space::{Material, Mesh, MeshSurface, Shaded, Vec3},
    Box, BoxSpec, Color, Modifier, Size,
};

use super::{
    figure::{world_to_scene, Plan},
    grid_mesh,
};

const PAPER: Color = Color(0.03, 0.42, 0.24, 1.0);
const SWEEP_WGSL: &str = concat!(
    include_str!("noise.wgsl"),
    include_str!("sheet_paper.wgsl"),
    include_str!("sweep.wgsl")
);
// Pieces of the quarter circle where the table turns into the wall.
const CURVE_PIECES: usize = 16;
const COLUMNS: usize = 8;

// The sweep, planned by `plan`, in a scene node of `node`'s size, seen by
// a camera that has pulled back as far as `pull` says.
#[composable(no_skip)]
pub(super) fn Sweep(node: Size, plan: Plan, pull: impl Fn() -> f32 + 'static) {
    let mesh = remember(move || Arc::new(sweep_mesh(&plan))).with(Arc::clone);
    Box(
        Modifier::empty()
            .required_size(Size::new(16.0, 16.0))
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&mesh),
                transform: world_to_scene(node),
                material: lit_paper(&plan, pull()),
                casts_shadow: false,
                solid: true,
            })
            .background(PAPER),
        BoxSpec::default(),
        || {},
    );
}

// The sweep's surface in world space: rows along its profile, from the
// table's front edge back and up the wall, columns across.
fn sweep_mesh(plan: &Plan) -> Mesh {
    let u = plan.u;
    let table_y = plan.table_y();
    let curve = plan.wall_curve();
    let radius = 1200.0 * u;
    let mut profile = vec![(3000.0 * u, table_y, 0.0f32)];
    for piece in 0..=CURVE_PIECES {
        let angle = piece as f32 / CURVE_PIECES as f32 * std::f32::consts::FRAC_PI_2;
        profile.push((
            -curve - radius * angle.sin(),
            table_y + radius * (1.0 - angle.cos()),
            angle,
        ));
    }
    profile.push((
        -curve - radius,
        table_y + radius + 4000.0 * u,
        std::f32::consts::FRAC_PI_2,
    ));
    let half_width = 7000.0 * u;
    let mut positions = Vec::with_capacity(profile.len() * (COLUMNS + 1));
    let mut normals = Vec::with_capacity(positions.capacity());
    let mut uvs = Vec::with_capacity(positions.capacity());
    for (row, &(z, y, angle)) in profile.iter().enumerate() {
        for column in 0..=COLUMNS {
            let share = column as f32 / COLUMNS as f32;
            positions.push([-half_width + 2.0 * half_width * share, y, z]);
            normals.push(Vec3::new(0.0, angle.cos(), angle.sin()).to_array());
            uvs.push([share, row as f32 / (profile.len() - 1) as f32]);
        }
    }
    Mesh::new(
        positions,
        uvs,
        Some(normals),
        grid_mesh::quads(COLUMNS + 1, profile.len()),
    )
    .expect("a whole grid of quads")
}

// The paper lit for a camera that has pulled back `pull` of the way.
fn lit_paper(plan: &Plan, pull: f32) -> Material {
    static SHADER: OnceLock<Arc<str>> = OnceLock::new();
    let u = plan.u;
    let (pool_x, pool_z) = plan.light_pool(pull);
    let [cos, sin, shift_y, shift_z, eye_distance] = plan.camera_numbers(pull);
    let mut params = [0.0; Shaded::PARAMS];
    params[..12].copy_from_slice(&[
        u,
        pool_x,
        pool_z,
        plan.table_y(),
        cos,
        sin,
        shift_y,
        shift_z,
        eye_distance,
        plan.sheet.x + plan.sheet.width * 0.5,
        plan.caption_right - 150.0 * u,
        plan.caption_right + (400.0 - 150.0 * pull) * u,
    ]);
    Material::Shaded(Shaded {
        shader: Arc::clone(SHADER.get_or_init(|| Arc::from(SWEEP_WGSL))),
        params,
    })
}
