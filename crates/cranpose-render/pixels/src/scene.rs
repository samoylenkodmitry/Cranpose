//! The retained scene of the pixel renderer: the items it draws, built from
//! the window's layout tree with no render graph, where each item reaches,
//! the pixels that changed since a host took its picture, and the hit
//! targets and node bounds a host asks of it.

use std::{
    cell::{Ref, RefCell},
    rc::Rc,
};

use cranpose_core::{MemoryApplier, NodeId, collections::map::HashMap};
pub use cranpose_render_common::graph_scene::HitRegion;
use cranpose_render_common::{
    RenderScene, SceneUpdates,
    graph::{LayerNode, ProjectiveTransform, RenderGraph, RenderNode},
    graph_scene::{HitList, HitStorage, RenderDiagnostics},
    scene_builder::release_redrawn_recordings,
};
use cranpose_ui_graphics::{Rect, space::Ray};

use crate::{
    damage,
    draw::{ClipBounds, TextRaster},
    layout_walk::{DirtyMarks, KeptSpace, LayoutWalk, names},
    moved_subtrees::MovedSubtrees,
    pipeline::{self, InPlaceUpdate, UpdatedReach},
    raster_scene::{RasterScene, SubtreeItems},
    rewrite::Rewrite,
};

/// The retained scene of a window: the items the pixel renderer draws, at
/// the renderer's scale, and the hit targets of its pointer input.
pub struct Scene {
    raster: Option<RasterScene>,
    // Where each item of `raster` can draw in `frame`, in its order: each
    // strip a host draws passes over the items that miss it.
    reach: Vec<Option<ClipBounds>>,
    // What an update in place pushes each changed subtree again with.
    rewrite: Option<Box<Rewrite>>,
    marks: DirtyMarks,
    scale: f32,
    // The frame the host took the picture in last, which the changes and
    // the reach are cut to.
    frame: ClipBounds,
    changed: Changed,
    // Collected from the scene's records when a query first needs them
    // after an update.
    hits: RefCell<HitList>,
    diagnostics: Rc<RenderDiagnostics>,
    // The updates that may have moved a layer with no layout pass, and the
    // builds, which may have.
    layer_moves: u64,
    // The subtrees whose layers hold a 3D space, kept whole: their scenes
    // draw their surfaces out of the tree's order and take input through
    // their eyes.
    spaces: RefCell<Vec<KeptSpace>>,
    // The subtrees of the children that moved in the update before.
    moved: RefCell<MovedSubtrees>,
}

// The pixels of the scene that changed since a host took its picture: the
// areas the updates changed, or the whole frame after a build that has no
// scene before it to compare with.
enum Changed {
    Areas(Vec<ClipBounds>),
    Whole,
}

const OPEN_FRAME: ClipBounds = ClipBounds {
    min_x: 0,
    min_y: 0,
    max_x: i32::MAX,
    max_y: i32::MAX,
};

impl Scene {
    /// An empty scene at one device pixel per point.
    pub fn new() -> Self {
        Self {
            raster: None,
            reach: Vec::new(),
            rewrite: None,
            marks: DirtyMarks::default(),
            scale: 1.0,
            frame: OPEN_FRAME,
            changed: Changed::Whole,
            hits: RefCell::default(),
            diagnostics: Rc::new(RenderDiagnostics::new()),
            layer_moves: 0,
            spaces: RefCell::default(),
            moved: RefCell::default(),
        }
    }

    /// The diagnostics shared by the scene and its hit targets.
    pub fn diagnostics(&self) -> &RenderDiagnostics {
        self.diagnostics.as_ref()
    }

    /// The hit targets in draw order, the last drawn at the end.
    pub fn hit_targets(&self) -> Vec<HitRegion> {
        self.fresh_hits().targets().to_vec()
    }

    /// How much storage the hit targets take.
    pub fn hit_storage(&self) -> HitStorage {
        self.hits.borrow().storage()
    }

    // The items, built at `scale` device pixels per point.
    pub(crate) fn raster(&self) -> Option<&RasterScene> {
        self.raster.as_ref()
    }

    pub(crate) fn reach(&self) -> &[Option<ClipBounds>] {
        &self.reach
    }

    // Builds the scene at `scale` from the next update on.
    pub(crate) fn set_scale(&mut self, scale: f32) {
        if scale != self.scale {
            self.scale = scale;
            self.raster = None;
        }
    }

    // Builds the scene of the window `applier` holds below `root` anew.
    pub(crate) fn rebuild(&mut self, applier: &MemoryApplier, root: NodeId, text: &TextRaster) {
        self.marks.clear(applier);
        self.build(applier, root, None, text);
        self.layer_moves += 1;
        self.finish_spaces(applier);
    }

    // Brings the scene up to the window `applier` holds below `root` after
    // `updates`: in place, else built again from the scene before.
    pub(crate) fn update(
        &mut self,
        applier: &MemoryApplier,
        root: NodeId,
        updates: SceneUpdates<'_>,
        text: &TextRaster,
    ) {
        if updates.is_empty() {
            return self.rebuild(applier, root, text);
        }
        if self.marks.mark(applier, updates) {
            self.layer_moves += 1;
        }
        if !self.update_in_place(applier, root, text) {
            let previous = self.raster.take();
            self.build(applier, root, previous, text);
        }
        self.finish_spaces(applier);
    }

    // Lets go the 3D subtrees the scene no longer draws, and publishes where
    // the surfaces of the ones the update lowered show.
    fn finish_spaces(&mut self, applier: &MemoryApplier) {
        let spaces = self.spaces.get_mut();
        if spaces.is_empty() {
            return;
        }
        let records = self
            .raster
            .as_ref()
            .map_or(&[][..], |raster| &raster.subtrees);
        spaces.retain_mut(|kept| {
            let Some(at) = space_record(records, kept.space.node()) else {
                return false;
            };
            if !kept.published {
                kept.space.publish(applier, parent_to_window(records, at));
                kept.published = true;
            }
            true
        });
    }

    // Builds the scene from `graph`, a render graph a layout tree snapshot
    // gave, which it keeps none of.
    pub(crate) fn rebuild_from_graph(&mut self, graph: &RenderGraph, text: &TextRaster) {
        let mut raster = self.storage();
        pipeline::fill_raster_scene(
            &mut raster,
            graph,
            &self.diagnostics,
            self.scale,
            None,
            text,
        );
        raster.reach_into(self.frame, &mut self.reach);
        self.raster = Some(raster);
        self.changed = Changed::Whole;
        self.hits.get_mut().mark_stale();
        self.layer_moves += 1;
    }

    // Updates the scene in place; false when it must be built again.
    fn update_in_place(
        &mut self,
        applier: &MemoryApplier,
        root: NodeId,
        text: &TextRaster,
    ) -> bool {
        let Some(raster) = self.raster.as_mut() else {
            return false;
        };
        let walk = LayoutWalk::new(applier, &self.marks, (&self.spaces, &self.moved));
        let mut areas = Vec::new();
        let updated = InPlaceUpdate {
            scene: raster,
            reach: &mut self.reach,
            rewrite: &mut self.rewrite,
        }
        .run_layout(
            (&walk, root),
            &self.diagnostics,
            (self.scale, text),
            self.frame,
            &mut areas,
        );
        if updated == Some(UpdatedReach::Lost) {
            raster.reach_into(self.frame, &mut self.reach);
        }
        if std::mem::take(&mut raster.hits_changed) {
            self.hits.get_mut().mark_stale();
        }
        // An update that gave up after it changed part of the scene leaves
        // the areas it changed, which the scene holds already.
        self.note_changes(&mut areas);
        updated.is_some()
    }

    // Builds the scene anew, copying from `previous`, the scene before, the
    // subtrees whose nodes did not change, and notes the pixels the two
    // draw differently.
    fn build(
        &mut self,
        applier: &MemoryApplier,
        root: NodeId,
        previous: Option<RasterScene>,
        text: &TextRaster,
    ) {
        let mut raster = self.storage();
        let walk = LayoutWalk::new(applier, &self.marks, (&self.spaces, &self.moved));
        let built = pipeline::fill_raster_scene_from_layout(
            &mut raster,
            (&walk, root),
            (self.scale, previous.as_ref()),
            &self.diagnostics,
            text,
        );
        self.hits.get_mut().mark_stale();
        if !built {
            self.raster = None;
            self.reach.clear();
            self.changed = Changed::Whole;
            return;
        }
        match previous.filter(|old| old.start == raster.start) {
            Some(old) => {
                let mut areas = Vec::new();
                damage::changed_areas(&old, &self.reach, &raster, self.frame, &mut areas);
                self.note_changes(&mut areas);
                self.give_back(old);
            }
            None => self.changed = Changed::Whole,
        }
        raster.reach_into(self.frame, &mut self.reach);
        raster.hits_changed = false;
        self.raster = Some(raster);
    }

    // The storage of the spill for a scene built anew.
    fn storage(&mut self) -> RasterScene {
        self.rewrite
            .as_mut()
            .map(|rewrite| rewrite.take_storage())
            .unwrap_or_default()
    }

    // Keeps the storage of `old`, a scene a build replaced, for the next.
    fn give_back(&mut self, mut old: RasterScene) {
        old.clear();
        self.rewrite
            .get_or_insert_with(Box::default)
            .give_storage(old);
    }

    // Adds `areas` to the pixels that changed since the host took its
    // picture.
    fn note_changes(&mut self, areas: &mut Vec<ClipBounds>) {
        if let Changed::Areas(changed) = &mut self.changed {
            changed.append(areas);
            damage::merge_areas(changed);
        }
    }

    // Puts into `areas` the parts of `frame` whose pixels changed since the
    // host took its picture last: the whole frame the first time and after
    // a change of size, a build with nothing to compare, or a scale.
    pub(crate) fn take_changes(&mut self, frame: ClipBounds, areas: &mut Vec<ClipBounds>) {
        areas.clear();
        let whole = frame != self.frame || matches!(self.changed, Changed::Whole);
        if frame != self.frame {
            self.frame = frame;
            if let Some(raster) = &self.raster {
                raster.reach_into(frame, &mut self.reach);
            }
        }
        match std::mem::replace(&mut self.changed, Changed::Areas(Vec::new())) {
            Changed::Areas(mut changed) if !whole => {
                changed.retain_mut(|area| area.clip_to(frame));
                areas.append(&mut changed);
                self.changed = Changed::Areas(changed);
            }
            _ if self.raster.is_some() && frame.max_x > 0 && frame.max_y > 0 => areas.push(frame),
            _ => {}
        }
    }

    // With little memory left: lets the rewrite's spill go and the room the
    // scene's lists grew past their items.
    pub(crate) fn trim(&mut self) {
        if let Some(rewrite) = self.rewrite.as_mut() {
            rewrite.release_spill();
        }
        if let Some(raster) = self.raster.as_mut() {
            crate::growth_trim::trim(raster, &mut self.reach);
        }
    }

    // The hit targets, collected from the records first when an update ran
    // since they were.
    fn fresh_hits(&self) -> Ref<'_, HitList> {
        if self.hits.borrow().is_stale() {
            let mut list = self.hits.borrow_mut();
            let spaces = self.spaces.borrow();
            let skeleton = self
                .raster
                .as_ref()
                .filter(|raster| !raster.subtrees.is_empty())
                .and_then(|raster| hit_skeleton(&raster.subtrees, 0, &spaces));
            match skeleton {
                Some(root) => list.collect(&root, &self.diagnostics),
                None => list.clear(),
            }
        }
        self.hits.borrow()
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

// The recordings the scene's commands keep for their next draws live with
// the thread: a scene that goes lets them go, as no draw of its own takes
// them again.
impl Drop for Scene {
    fn drop(&mut self) {
        release_redrawn_recordings();
    }
}

// The layers of the subtree of the record `index` that hold hit targets or
// cut the ones beneath them, as layers the graph's hit walk reads: their
// node, placement, bounds, clip and handlers. `None` when the subtree holds
// no target.
fn hit_skeleton(
    records: &[SubtreeItems],
    index: usize,
    spaces: &[KeptSpace],
) -> Option<Box<LayerNode>> {
    let record = &records[index];
    // A 3D subtree takes input through its scene's eye, which its layers
    // tell.
    if let Some(kept) = spaces
        .iter()
        .find(|kept| names(record.identity, kept.space.node()))
    {
        return kept.space.layer().map(|layer| Box::new(layer.clone()));
    }
    let mut children = Vec::new();
    let mut child = index + 1;
    while child < record.end {
        if let Some(layer) = hit_skeleton(records, child, spaces) {
            children.push(RenderNode::Layer(layer));
        }
        child = records[child].end;
    }
    let hits = record.hits.as_deref();
    let handles = hits.is_some_and(|hits| hits.hit_test.is_some());
    if children.is_empty() && !handles {
        return None;
    }
    let mut layer = Box::<LayerNode>::default();
    layer.node_id = record.identity.0;
    layer.wraps = record.identity.1;
    layer.transform_to_parent = record.transform;
    layer.local_bounds = Rect::from_size(record.node_size);
    if let Some(hits) = hits {
        layer.local_bounds = hits.local_bounds;
        layer.node_bounds = hits.node_bounds;
        layer.clip_to_bounds = hits.clips;
        layer.hit_test.clone_from(&hits.hit_test);
    }
    layer.has_hit_targets = true;
    layer.children = children;
    Some(layer)
}

// Visits the node of each record from `index` on, before `end`, and their
// subtrees, with the bounds of its rect in the window, given `to_window`
// for their parent.
fn visit_node_bounds(
    (records, spaces): (&[SubtreeItems], &[KeptSpace]),
    (mut index, end): (usize, usize),
    (to_window, surfaces): (ProjectiveTransform, &HashMap<NodeId, ProjectiveTransform>),
    visit: &mut dyn FnMut(NodeId, Rect),
) {
    while index < end {
        let record = &records[index];
        if let Some(kept) = spaces
            .iter()
            .find(|kept| names(record.identity, kept.space.node()))
        {
            kept.space.node_window_bounds(to_window, surfaces, visit);
            index = record.end;
            continue;
        }
        let to_record = record.transform.then(to_window);
        if let Some(node) = record.identity.0 {
            visit(
                node,
                to_record.bounds_for_rect(Rect::from_size(record.node_size)),
            );
        }
        visit_node_bounds(
            (records, spaces),
            (index + 1, record.end),
            (to_record, surfaces),
            visit,
        );
        index = record.end;
    }
}

// The record of the subtree of `node`, which the scene keeps whole.
fn space_record(records: &[SubtreeItems], node: NodeId) -> Option<usize> {
    records
        .iter()
        .position(|record| names(record.identity, node))
}

// The map from the parent of the record `at` to the window: the placements
// of the records above it.
fn parent_to_window(records: &[SubtreeItems], at: usize) -> ProjectiveTransform {
    let mut to_window = ProjectiveTransform::identity();
    let mut index = 0;
    while index < at {
        let record = &records[index];
        if record.end > at {
            to_window = record.transform.then(to_window);
            index += 1;
        } else {
            index = record.end;
        }
    }
    to_window
}

impl RenderScene for Scene {
    type HitTarget = HitRegion;

    fn clear(&mut self) {
        self.raster = None;
        self.reach.clear();
        self.changed = Changed::Whole;
        self.hits.get_mut().clear();
        self.spaces.get_mut().clear();
        self.moved.get_mut().clear();
    }

    fn hit_test(&self, x: f32, y: f32) -> Vec<Self::HitTarget> {
        self.fresh_hits().hit_test(x, y)
    }

    fn hit_test_near(&self, x: f32, y: f32) -> Option<Self::HitTarget> {
        self.fresh_hits().hit_test_near(x, y)
    }

    fn find_target(&self, node_id: NodeId) -> Option<Self::HitTarget> {
        self.fresh_hits().find(node_id)
    }

    fn hit_test_ray(&self, scene: NodeId, ray: Ray) -> Vec<Self::HitTarget> {
        self.fresh_hits().hit_test_ray(scene, ray)
    }

    fn surface_window_bounds(&self, node_id: NodeId) -> Option<Rect> {
        self.fresh_hits().surface_window_bounds(node_id)
    }

    fn surface_windows(&self, visit: &mut dyn FnMut(NodeId, ProjectiveTransform)) {
        let Some(raster) = &self.raster else {
            return;
        };
        for kept in self.spaces.borrow().iter() {
            if let Some(at) = space_record(&raster.subtrees, kept.space.node()) {
                let to_window = parent_to_window(&raster.subtrees, at);
                kept.space.surface_windows(to_window, visit);
            }
        }
    }

    fn node_window_bounds(&self, visit: &mut dyn FnMut(NodeId, Rect)) {
        let Some(raster) = &self.raster else {
            return;
        };
        let mut surfaces = HashMap::default();
        self.surface_windows(&mut |node_id, content| {
            surfaces.insert(node_id, content);
        });
        let records = &raster.subtrees;
        visit_node_bounds(
            (records, &self.spaces.borrow()),
            (0, records.len()),
            (ProjectiveTransform::identity(), &surfaces),
            visit,
        );
    }

    fn layer_moves(&self) -> u64 {
        self.layer_moves
    }

    fn visit_retained_visual_observation_nodes(&self, visit: &mut dyn FnMut(NodeId)) -> bool {
        if let Some(raster) = &self.raster {
            for node in raster
                .subtrees
                .iter()
                .filter_map(|record| record.identity.0)
            {
                visit(node);
            }
        }
        true
    }
}
