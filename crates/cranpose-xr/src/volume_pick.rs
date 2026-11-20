//! Where a ray meets the meshes of a volume's content, on the CPU.
//!
//! The scene that draws the volumes keeps each mesh with the map that places it in its volume's
//! content space. A ray comes into that space once for each volume, and each mesh takes it into its
//! own space, where the mesh's box turns most rays away before any triangle is read. Nothing here
//! allocates after the list of meshes has grown to the size of the content.

use std::sync::Arc;

use cranpose::{
    GuestView, MeshVisit,
    space::{Mat4, Mesh, Vec3},
};
use cranpose_core::NodeId;

use crate::{
    content_ray::ContentRay,
    mesh_ref::MeshRef,
    space::{PanelKind, PanelSlot, Space},
};

// Where a ray meets a mesh.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MeshPick {
    pub(crate) mesh: MeshRef,
    pub(crate) triangle: u32,
    // How far along the room's ray, in meters
    pub(crate) distance: f32,
    // The place in the volume's content space, in dp
    pub(crate) position: [f32; 3],
}

// One mesh of a volume's content, ready to take rays.
struct PickMesh {
    volume: u64,
    mesh: MeshRef,
    shape: Arc<Mesh>,
    // The map from the content space to the mesh's own space
    from_content: Mat4,
}

// The meshes of the volumes, which the scene's last draw left.
#[derive(Default)]
pub(crate) struct PickMeshes {
    meshes: Vec<PickMesh>,
}

impl PickMeshes {
    // Takes the meshes of the volume `volume` again from `visit`, which calls its argument for each
    // mesh of the volume's content with the map from the mesh to the content space. A mesh whose
    // map has no inverse, such as one scaled to a plane, takes no ray, and neither does a layer
    // that no node owns.
    pub(crate) fn replace(
        &mut self,
        volume: u64,
        visit: impl FnOnce(&mut dyn FnMut(&MeshVisit<'_>)),
    ) {
        self.meshes.retain(|kept| kept.volume != volume);
        let meshes = &mut self.meshes;
        visit(&mut |visit| {
            let (Some(node), Some(from_content)) = (visit.node, visit.to_frame.inverse()) else {
                return;
            };
            meshes.push(PickMesh {
                volume,
                mesh: MeshRef {
                    node,
                    index: visit.mesh,
                },
                shape: Arc::clone(&visit.surface.mesh),
                from_content,
            });
        });
    }

    // Takes the meshes again of each volume among `panels` that `wanted` names, from the scene that
    // the guest view drew into the window root `root`, and forgets the meshes of volumes that left.
    // The scene walk costs the size of the content, so a volume that no ray points at keeps the
    // meshes of its last pick.
    pub(crate) fn refresh(
        &mut self,
        (guest, root): (&mut GuestView, NodeId),
        space: &Space,
        panels: &[PanelSlot],
        wanted: impl Fn(&PanelSlot) -> bool,
    ) {
        self.meshes
            .retain(|kept| panels.iter().any(|slot| slot.id == kept.volume));
        for slot in panels {
            if !matches!(slot.kind, PanelKind::Volume { .. }) || !wanted(slot) {
                continue;
            }
            let Some(under) = space.volumes().node_of(slot.id) else {
                continue;
            };
            self.replace(slot.id, |visit| {
                guest.visit_window_meshes((root, under), visit);
            });
        }
    }

    // Whether the volume has a mesh that a ray can meet. A volume with none is as deep as its box.
    pub(crate) fn has_meshes(&self, volume: u64) -> bool {
        self.meshes.iter().any(|kept| kept.volume == volume)
    }

    // The nearest place in front of the ray's origin where it meets a triangle of the volume's
    // meshes, from either side, or of the one mesh `only`.
    pub(crate) fn pick(
        &self,
        volume: u64,
        ray: &ContentRay,
        only: Option<MeshRef>,
    ) -> Option<MeshPick> {
        let mut nearest: Option<MeshPick> = None;
        for kept in &self.meshes {
            if kept.volume != volume || only.is_some_and(|only| only != kept.mesh) {
                continue;
            }
            let origin = kept
                .from_content
                .transform_point(Vec3::from_array(ray.origin));
            let direction = kept
                .from_content
                .transform_vector(Vec3::from_array(ray.direction));
            if !meets_box(origin, direction, kept.shape.bounds()) {
                continue;
            }
            let Some(hit) = kept.shape.cast(origin, direction) else {
                continue;
            };
            if nearest.is_some_and(|known| known.distance <= hit.distance) {
                continue;
            }
            nearest = Some(MeshPick {
                mesh: kept.mesh,
                triangle: hit.triangle,
                distance: hit.distance,
                position: ray.at(hit.distance),
            });
        }
        nearest
    }
}

// Whether the ray meets the box between `low` and `high` at or in front of its origin.
fn meets_box(origin: Vec3, direction: Vec3, (low, high): (Vec3, Vec3)) -> bool {
    let (mut enters, mut leaves) = (0.0_f32, f32::INFINITY);
    for (start, along, (low, high)) in [
        (origin.x, direction.x, (low.x, high.x)),
        (origin.y, direction.y, (low.y, high.y)),
        (origin.z, direction.z, (low.z, high.z)),
    ] {
        if along == 0.0 {
            if start < low || start > high {
                return false;
            }
            continue;
        }
        let (first, second) = ((low - start) / along, (high - start) / along);
        enters = enters.max(first.min(second));
        leaves = leaves.min(first.max(second));
        if enters > leaves {
            return false;
        }
    }
    true
}

#[cfg(test)]
#[path = "tests/volume_pick_tests.rs"]
mod tests;
