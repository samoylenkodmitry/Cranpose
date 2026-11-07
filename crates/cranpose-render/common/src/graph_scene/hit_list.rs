// The hit targets of a window's scene and the queries a host asks of them.

use std::{cmp::Reverse, rc::Rc};

use cranpose_core::{NodeId, collections::map::HashMap};
use cranpose_ui_graphics::{Rect, space::Ray};

use super::{
    HitClip, HitGeometry, HitRegion, HitRegionInit, HitStorage, HitTargetSpec, RenderDiagnostics,
};
use crate::{
    graph::{LayerNode, ProjectiveTransform},
    hit_graph::HitCollector,
};

#[derive(Default)]
struct HitBuffers {
    hit_clips: Vec<HitClip>,
    capture_path: Vec<NodeId>,
}

/// The hit targets of a window's scene in draw order, the last drawn at the
/// end, collected again from the layers that hold them after they change,
/// and the storage the next collection of them reuses.
#[derive(Default)]
pub struct HitList {
    pub(super) hits: Vec<HitRegion>,
    buffers: Vec<HitBuffers>,
    // The draw order index the next pushed target receives.
    pub(super) next_z: usize,
    // Where in `hits` the target of each node lies.
    pub(super) node_index: HashMap<NodeId, usize>,
    // The graph changed a target since the list was collected: the next
    // query collects it again.
    stale: bool,
}

impl HitList {
    /// Notes that a target or the geometry above one changed: the next
    /// query collects the targets again.
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// Whether a target changed since the targets were collected.
    pub fn is_stale(&self) -> bool {
        self.stale
    }

    /// Collects the targets of `root`, a window's root layer, and the layers
    /// beneath it, in place of the ones held, keeping their storage.
    pub fn collect(&mut self, root: &LayerNode, diagnostics: &Rc<RenderDiagnostics>) {
        self.clear();
        crate::hit_graph::collect_hits_from_graph(
            root,
            ProjectiveTransform::identity(),
            &mut HitTargets {
                list: self,
                diagnostics,
            },
            None,
        );
    }

    /// Collects the targets that `walk` enters, in draw order, in place of
    /// the ones held, keeping their storage: for a renderer that holds its
    /// layers in a form of its own.
    pub fn collect_walking(
        &mut self,
        diagnostics: &Rc<RenderDiagnostics>,
        walk: impl FnOnce(&mut HitCollector<'_, '_>),
    ) {
        self.clear();
        walk(&mut HitCollector::new(&mut HitTargets {
            list: self,
            diagnostics,
        }));
    }

    /// The targets in draw order, the last drawn at the end.
    pub fn targets(&self) -> &[HitRegion] {
        &self.hits
    }

    /// The targets under the window point `(x, y)`, the last drawn first.
    pub fn hit_test(&self, x: f32, y: f32) -> Vec<HitRegion> {
        let mut hit_indices: Vec<usize> = self
            .hits
            .iter()
            .enumerate()
            .filter_map(|(index, hit)| hit.contains(x, y).then_some(index))
            .collect();
        hit_indices.sort_by_key(|&index| Reverse(self.hits[index].z_index));
        hit_indices
            .into_iter()
            .map(|index| self.hits[index].clone())
            .collect()
    }

    /// The target nearest the window point `(x, y)` within its reach, the
    /// last drawn among equally near ones.
    pub fn hit_test_near(&self, x: f32, y: f32) -> Option<HitRegion> {
        self.hits
            .iter()
            .filter_map(|hit| hit.reach_distance(x, y).map(|distance| (distance, hit)))
            .min_by(|(near, hit), (other_near, other)| {
                near.total_cmp(other_near)
                    .then_with(|| other.z_index.cmp(&hit.z_index))
            })
            .map(|(_, hit)| hit.clone())
    }

    /// The target of `node_id`.
    pub fn find(&self, node_id: NodeId) -> Option<HitRegion> {
        self.node_index
            .get(&node_id)
            .and_then(|&index| self.hits.get(index))
            .cloned()
    }

    /// The targets on the surfaces of the scene `scene` that `ray` meets,
    /// the last drawn first.
    pub fn hit_test_ray(&self, scene: NodeId, ray: Ray) -> Vec<HitRegion> {
        let mut hit: Vec<&HitRegion> = self
            .hits
            .iter()
            .filter(|hit| {
                hit.surface.as_ref().is_some_and(|surface| {
                    surface.target.pick.scene() == Some(scene)
                        && surface
                            .local_point_along(ray)
                            .is_some_and(|local| hit.contains_local(local))
                })
            })
            .collect();
        hit.sort_by_key(|hit| Reverse(hit.z_index));
        hit.into_iter().cloned().collect()
    }

    /// Where the target of `node_id` on a mesh surface shows in the window.
    pub fn surface_window_bounds(&self, node_id: NodeId) -> Option<Rect> {
        let hit = self.hits.get(*self.node_index.get(&node_id)?)?;
        let surface = hit.surface.as_ref()?;
        surface
            .target
            .window_bounds(&surface.content_to_local, hit.local_bounds)
    }

    /// How much storage the targets take.
    pub fn storage(&self) -> HitStorage {
        HitStorage {
            hits_len: self.hits.len(),
            hits_capacity: self.hits.capacity(),
            node_index_len: self.node_index.len(),
            node_index_capacity: self.node_index.capacity(),
        }
    }

    /// Adds an interactive target in draw order, ignoring one that neither
    /// handles a pointer nor names a pointer icon.
    pub fn push(
        &mut self,
        diagnostics: &Rc<RenderDiagnostics>,
        node_id: NodeId,
        capture_path: &[NodeId],
        (geometry, target): (HitGeometry<'_>, HitTargetSpec<'_>),
    ) {
        HitTargets {
            list: self,
            diagnostics,
        }
        .push(node_id, capture_path, geometry, target);
    }

    /// Removes the targets, releases their handlers and keeps their storage.
    pub fn clear(&mut self) {
        self.stale = false;
        self.buffers.clear();
        for hit in self.hits.drain(..) {
            let mut buffers = HitBuffers {
                hit_clips: hit.hit_clips,
                capture_path: hit.capture_path,
            };
            buffers.hit_clips.clear();
            buffers.capture_path.clear();
            self.buffers.push(buffers);
        }
        self.node_index.clear();
        self.next_z = 0;
    }
}

// The hit targets of a [`super::Scene`], borrowed apart from its graph, so
// a walk of the graph pushes targets while it reads the graph in place.
pub(crate) struct HitTargets<'a> {
    list: &'a mut HitList,
    diagnostics: &'a Rc<RenderDiagnostics>,
}

impl HitTargets<'_> {
    // Adds an interactive target in draw order (see [`HitList::push`]).
    pub(crate) fn push(
        &mut self,
        node_id: NodeId,
        capture_path: &[NodeId],
        geometry: HitGeometry<'_>,
        target: HitTargetSpec<'_>,
    ) {
        let HitTargetSpec { shape, handlers } = target;
        if handlers.pointer_inputs().is_empty() && handlers.pointer_icon().is_none() {
            return;
        }
        let list = &mut *self.list;
        let mut buffers = list.buffers.pop().unwrap_or_default();
        buffers.capture_path.extend_from_slice(capture_path);

        let z_index = list.next_z;
        list.next_z += 1;
        let hit_index = list.hits.len();
        list.hits.push(HitRegion::with_diagnostics(HitRegionInit {
            node_id,
            capture_path: buffers.capture_path,
            geometry,
            clip_buffer: buffers.hit_clips,
            shape,
            handlers: Rc::clone(handlers),
            z_index,
            diagnostics: Rc::clone(self.diagnostics),
        }));
        list.node_index.insert(node_id, hit_index);
    }
}
