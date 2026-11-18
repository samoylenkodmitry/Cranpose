// A mesh laid over a rectangle of sheet points: the flat copy on the board,
// the bent copy on the print and the labels on each. The caller says where a
// sheet point lies in world space; the mesh's texture spans `texture`, so
// what lies outside it, such as a border round a picture, takes texture
// coordinates outside 0..1.

use std::sync::Arc;

use cranpose_ui::{
    space::{Material, Mesh, MeshSurface, Vec3},
    Point, Rect, Size,
};

use super::{figure::world_to_scene, whole};

// A label on `mesh` in a node of size `node`, drawn in its own colours.
pub(super) fn label_surface(mesh: Mesh, node: Size) -> MeshSurface {
    MeshSurface {
        mesh: Arc::new(mesh),
        transform: world_to_scene(node),
        material: Material::Unlit,
        casts_shadow: false,
        solid: false,
    }
}

// A grid of `columns` by one quads over `rect`, its points placed by
// `place`, which gives each its world position and the normal there.
pub(super) fn sheet_mesh(
    rect: Rect,
    texture: Rect,
    columns: usize,
    place: impl Fn(Point) -> (Vec3, Vec3),
) -> Mesh {
    let columns = columns.max(1);
    let mut positions = Vec::with_capacity((columns + 1) * 2);
    let mut normals = Vec::with_capacity((columns + 1) * 2);
    let mut uvs = Vec::with_capacity((columns + 1) * 2);
    for column in 0..=columns {
        let x = rect.x + rect.width * column as f32 / columns as f32;
        for y in [rect.y, rect.y + rect.height] {
            let (position, normal) = place(Point::new(x, y));
            positions.push(position.to_array());
            normals.push(normal.to_array());
            uvs.push([
                (x - texture.x) / texture.width,
                (y - texture.y) / texture.height,
            ]);
        }
    }
    let mut indices = Vec::with_capacity(columns * 6);
    for column in 0..whole::narrow(columns) {
        let (top, bottom) = (column * 2, column * 2 + 1);
        indices.extend_from_slice(&[top, bottom, top + 2, top + 2, bottom, bottom + 2]);
    }
    Mesh::new(positions, uvs, Some(normals), indices).expect("a whole grid of quads")
}
