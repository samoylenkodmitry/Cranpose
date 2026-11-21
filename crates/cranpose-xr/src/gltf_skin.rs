//! A skinned primitive whose every vertex follows one joint, cut into one mesh for each joint.
//!
//! A skin moves each vertex by the joints that it names, weighed. A vertex with all its weight on one
//! joint moves as that joint does, so the triangles of that joint form a rigid piece. The piece's
//! vertices go into the joint's space through the joint's inverse bind matrix, and the piece then
//! hangs on the joint's node like any mesh of a node. A joint that moves therefore moves its piece,
//! with no work for each vertex in a frame. A controller's model keeps each button as such a piece.
//!
//! A skin that weighs a vertex between joints, or a triangle whose corners follow different
//! joints, needs a skinning pass that the volume does not have. Such a primitive keeps its bind
//! pose at its own node.

use std::sync::Arc;

use cranpose::space::{Mat4, Mesh, Vec3};

use crate::{
    gltf_buffers::Buffers,
    gltf_import::{Triangles, primitive_triangles},
};

// A vertex counts as following one joint when the joint holds this much of its weight.
const WHOLE: f32 = 0.999;

// The pieces of a skinned primitive: each mesh with the node of its joint.
pub(crate) type Pieces = Vec<(usize, Arc<Mesh>)>;

// The meshes of `primitive` under `skin`, each with the node of its joint, or `None` when the
// primitive is not rigid or names a joint outside the scene that `reached` marks.
pub(crate) fn rigid_pieces(
    skin: &gltf::Skin<'_>,
    primitive: &gltf::Primitive<'_>,
    buffers: &Buffers,
    tex_coord: u32,
    reached: &[bool],
) -> Result<Option<Pieces>, String> {
    let joints: Vec<usize> = skin.joints().map(|joint| joint.index()).collect();
    if joints
        .iter()
        .any(|&node| !reached.get(node).copied().unwrap_or(false))
    {
        return Ok(None);
    }
    let Some(triangles) = primitive_triangles(primitive, buffers, tex_coord)? else {
        return Ok(None);
    };
    let reader = primitive.reader(|buffer| buffers.data(&buffer));
    let (Some(named), Some(weights)) = (reader.read_joints(0), reader.read_weights(0)) else {
        return Ok(None);
    };
    let follows: Option<Vec<usize>> = named
        .into_u16()
        .zip(weights.into_f32())
        .map(|(named, weights)| only_joint(named, weights))
        .collect();
    let Some(follows) = follows else {
        return Ok(None);
    };
    if follows.len() != triangles.positions.len()
        || follows.iter().any(|&joint| joint >= joints.len())
    {
        return Err("its joints and weights do not match its vertices".into());
    }
    let skin_reader = skin.reader(|buffer| buffers.data(&buffer));
    let unbind: Vec<Mat4> = match skin_reader.read_inverse_bind_matrices() {
        Some(matrices) => matrices.map(|columns| column_matrix(&columns)).collect(),
        None => vec![Mat4::IDENTITY; joints.len()],
    };
    if unbind.len() < joints.len() {
        return Err("its skin has fewer inverse bind matrices than joints".into());
    }
    let Some(by_joint) = triangles_by_joint(&triangles.indices, &follows, joints.len()) else {
        return Ok(None);
    };
    let mut pieces = Vec::new();
    for (joint, indices) in by_joint.into_iter().enumerate() {
        if indices.is_empty() {
            continue;
        }
        if let Some(mesh) = piece(&triangles, &indices, &unbind[joint])?.into_mesh()? {
            pieces.push((joints[joint], Arc::new(mesh)));
        }
    }
    Ok(Some(pieces))
}

// The one joint that a vertex follows, or `None` when its weight lies on several.
fn only_joint(named: [u16; 4], weights: [f32; 4]) -> Option<usize> {
    let total: f32 = weights.iter().sum();
    let (slot, top) = weights
        .iter()
        .copied()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    (top >= WHOLE * total && total > 0.0).then(|| usize::from(named[slot]))
}

// The corners of the triangles of each joint, or `None` when a triangle's corners follow
// different joints.
fn triangles_by_joint(indices: &[u32], follows: &[usize], joints: usize) -> Option<Vec<Vec<u32>>> {
    let mut by_joint = vec![Vec::new(); joints];
    for corners in indices.as_chunks::<3>().0 {
        let joint = *follows.get(usize::try_from(corners[0]).ok()?)?;
        for &corner in corners {
            if *follows.get(usize::try_from(corner).ok()?)? != joint {
                return None;
            }
        }
        by_joint[joint].extend_from_slice(corners);
    }
    Some(by_joint)
}

// The triangles `indices` of `all`, with their own vertices moved into their joint's space by
// `unbind`.
fn piece(all: &Triangles, indices: &[u32], unbind: &Mat4) -> Result<Triangles, String> {
    let turn_normal = unbind.inverse().unwrap_or(Mat4::IDENTITY).transpose();
    let mut renumbered = vec![u32::MAX; all.positions.len()];
    let mut piece = Triangles {
        positions: Vec::new(),
        uvs: Vec::new(),
        normals: all.normals.as_ref().map(|_| Vec::new()),
        indices: Vec::with_capacity(indices.len()),
    };
    for &corner in indices {
        let at = usize::try_from(corner).map_err(|error| error.to_string())?;
        if renumbered[at] == u32::MAX {
            renumbered[at] =
                u32::try_from(piece.positions.len()).map_err(|error| error.to_string())?;
            let position = unbind.transform_point(Vec3::from_array(all.positions[at]));
            piece.positions.push(position.to_array());
            piece.uvs.push(all.uvs[at]);
            if let (Some(normals), Some(all_normals)) = (&mut piece.normals, &all.normals) {
                let normal = turn_normal.transform_vector(Vec3::from_array(all_normals[at]));
                normals.push(normal.normalized().unwrap_or(normal).to_array());
            }
        }
        piece.indices.push(renumbered[at]);
    }
    Ok(piece)
}

// The matrix that glTF stores as four columns.
fn column_matrix(columns: &[[f32; 4]; 4]) -> Mat4 {
    let axis = |column: &[f32; 4]| Vec3::new(column[0], column[1], column[2]);
    Mat4::from_axes(
        axis(&columns[0]),
        axis(&columns[1]),
        axis(&columns[2]),
        axis(&columns[3]),
    )
}

#[cfg(test)]
#[path = "tests/gltf_skin_tests.rs"]
mod tests;
