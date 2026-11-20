//! Which mesh of a volume's content a ray hit, and where.

use cranpose_core::NodeId;

/// Where a ray meets the content of a volume, as Jetpack XR's `InputEvent.HitInfo` tells it, with
/// the mesh that it meets.
///
/// Jetpack XR's `HitInfo` has the `inputEntity` that the ray hit, the `hitPosition` and the
/// `transform` from the activity space to the entity. Cranpose has no entities. The mesh stands on a
/// composable node, so that node is the entity, and [`mesh_index`](Self::mesh_index) tells which of
/// the node's [`mesh_surfaces`](cranpose::Modifier::mesh_surfaces) it is. A hit has no `transform`:
/// the node's frame is Cranpose's own. A [`SpatialGltfModelState`](crate::SpatialGltfModelState)
/// tells which glTF node a hit is on. Jetpack XR gives a list of every entity that the ray crosses,
/// nearest first, where a hit here holds the nearest mesh only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitInfo {
    /// The node of the volume's content whose mesh the ray hit. Jetpack XR's `inputEntity`. The
    /// node is the one that a `Box` or another composable of the content returns.
    pub input_entity: NodeId,
    /// Which of the node's meshes it is, counted from the first. A node with
    /// [`mesh_surface`](cranpose::Modifier::mesh_surface) has one.
    pub mesh_index: usize,
    /// The place of the hit in the volume's content space, in dp: x to the right, y down and z
    /// away from the viewer, from the box's center. Jetpack XR's `hitPosition`. It is `None` when
    /// the ray no longer meets the mesh during a press that began on it. A `Move` or an `Up`
    /// of that press still names the mesh.
    pub hit_position: Option<[f32; 3]>,
    /// The index of the mesh's triangle that the ray meets, with the hit position.
    pub triangle: Option<u32>,
}
