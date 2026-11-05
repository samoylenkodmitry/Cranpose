// The level the Backrooms tab shows: one volume whose shader draws the
// whole maze ray by ray, inside a box that moves with the eye from cell to
// cell, so the maze never ends.

use std::sync::{Arc, OnceLock};

use cranpose_ui::space::{Mat4, Material, Mesh, MeshSurface, Vec3, Volume};

use super::maze::{self, Cell, CEILING, CELL};

// How far the eye sees, in scene units; the box reaches this far around the
// eye's cell.
const SIGHT: f32 = 4400.0;

// What the level shows in a frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Frame {
    // The camcorder's frames so far, which the lamps flicker and the grain
    // moves by.
    pub(super) frame: u32,
    // The eye's cell, at whose corner the scene's origin lies.
    pub(super) cell: Cell,
    // How far a blackout dims every lamp, from 0 to 1.
    pub(super) dim: f32,
    // Where the figure stands on the floor, measured from the corner of
    // `cell`, while it stands there.
    pub(super) figure: Option<Vec3>,
}

impl Frame {
    // The level's surface: `view_box` around the scene's origin, at the
    // corner of the eye's cell.
    pub(super) fn surface(self, view_box: &Arc<Mesh>) -> MeshSurface {
        let mut params = [0.0; Volume::PARAMS];
        // The frame wraps at 2^24, past which an f32 skips frames: over a
        // week at 24 frames a second.
        params[0] = (self.frame & 0x00ff_ffff) as f32;
        params[1] = self.cell.x as f32;
        params[2] = self.cell.z as f32;
        params[3] = self.dim;
        if let Some(at) = self.figure {
            params[4] = at.x;
            params[5] = at.z;
            params[6] = 1.0;
        }
        params[7] = SIGHT;
        MeshSurface {
            mesh: Arc::clone(view_box),
            transform: Mat4::IDENTITY,
            material: Material::Volume(Volume {
                shader: shader(),
                params,
            }),
            casts_shadow: false,
            solid: false,
        }
    }
}

// The box the level is drawn in: from floor to ceiling, and from a cell
// whose corner is its origin out past `SIGHT` from anywhere in the cell, so
// the haze hides every way out of it.
pub(super) fn view_box() -> Option<Mesh> {
    let cube = Mesh::cuboid();
    let reach = SIGHT + 2.0 * CELL;
    let low = Vec3::new(-reach, -CEILING - 1.0, -reach);
    let size = Vec3::new(2.0 * reach + CELL, CEILING + 2.0, 2.0 * reach + CELL);
    let positions = cube
        .positions()
        .iter()
        .map(|&[x, y, z]| {
            [
                low.x + (x + 0.5) * size.x,
                low.y + (y + 0.5) * size.y,
                low.z + (z + 0.5) * size.z,
            ]
        })
        .collect();
    Mesh::new(
        positions,
        cube.uvs().to_vec(),
        Some(cube.normals().to_vec()),
        cube.indices().to_vec(),
    )
    .ok()
}

fn shader() -> Arc<str> {
    static SOURCE: OnceLock<Arc<str>> = OnceLock::new();
    Arc::clone(SOURCE.get_or_init(|| {
        Arc::from(format!(
            "{}{}",
            maze::wgsl_constants(),
            include_str!("level.wgsl")
        ))
    }))
}
