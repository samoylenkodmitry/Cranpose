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
    graph::{ProjectiveTransform, RenderGraph},
    graph_scene::{HitList, RenderDiagnostics},
    hit_graph::{FlatHitLayer, HitCollector, HitParent},
    scene_builder::release_walk_storage,
};
use cranpose_ui_graphics::Rect;

use crate::{
    PixelRect, damage,
    draw::{ClipBounds, TextRaster},
    layout_walk::{DirtyMarks, KeptSpace, LayoutWalk, kept_space, names},
    moved_subtrees::MovedSubtrees,
    pipeline::{self, InPlaceUpdate, UpdatedReach, WalkedTargets},
    pixel_rect,
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
    // What an update in place pushes each changed subtree again with, and
    // the room for the nodes whose subtrees it pushes again.
    rewrite: Option<Box<Rewrite>>,
    // The nodes the last update in place pushed again, at the records of
    // `raster`: a build forgets them.
    walked: WalkedTargets,
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
            walked: WalkedTargets::default(),
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

    // The items, built at `scale` device pixels per point.
    pub(crate) fn raster(&self) -> Option<&RasterScene> {
        self.raster.as_ref()
    }

    pub(crate) fn reach(&self) -> &[Option<ClipBounds>] {
        &self.reach
    }

    // Builds the scene at `scale` from the next update on.
    pub(crate) fn set_scale(&mut self, scale: f32) {
        if scale.to_bits() != self.scale.to_bits() {
            self.scale = scale;
            self.raster = None;
        }
    }

    // Builds the scene of the window `applier` holds below `root` anew.
    pub(crate) fn rebuild(&mut self, applier: &MemoryApplier, root: NodeId, text: &TextRaster) {
        self.marks.clear(applier);
        // A build with no marks names no node that changed: a subtree kept
        // for a node may hold draws the node no longer has.
        self.moved.get_mut().clear();
        // The build reads nothing of the scene before: its lists and the
        // heads its records keep go to the new scene.
        if let Some(mut old) = self.raster.take() {
            old.drop_heads();
            self.give_back(old);
        }
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
        let moved = self.moved.get_mut();
        if self.marks.mark(applier, updates, moved) {
            self.layer_moves += 1;
        }
        moved.begin_update(cranpose_ui::current_memory_headroom());
        if !self.update_in_place((applier, root), updates, text) {
            // The build copies records of the scene before with no head.
            let mut previous = self.raster.take();
            if let Some(previous) = previous.as_mut() {
                previous.drop_heads();
            }
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
        self.walked.clear();
        let mut raster = self.storage();
        pipeline::fill_raster_scene(
            &mut raster,
            graph,
            &self.diagnostics,
            self.scale,
            None,
            text,
        );
        // The host that takes the whole frame measures where items reach.
        self.reach.clear();
        self.raster = Some(raster);
        self.changed = Changed::Whole;
        self.hits.get_mut().mark_stale();
        self.layer_moves += 1;
    }

    // Updates the scene in place; false when it must be built again.
    fn update_in_place(
        &mut self,
        (applier, root): (&MemoryApplier, NodeId),
        updates: SceneUpdates<'_>,
        text: &TextRaster,
    ) -> bool {
        let Some(raster) = self.raster.as_mut() else {
            return false;
        };
        let walk = LayoutWalk::new(applier, &self.marks, (&self.spaces, &self.moved));
        let mut unread = Vec::new();
        // An update that gave up after it changed part of the scene leaves
        // the areas it changed, which the scene holds already. While the
        // whole frame waits for a host to take it, an update notes no areas
        // and the host's take measures where items reach.
        let (window, areas) = match &mut self.changed {
            Changed::Areas(changed) => (Some(self.frame), changed),
            Changed::Whole => {
                self.reach.clear();
                (None, &mut unread)
            }
        };
        let updated = InPlaceUpdate {
            scene: raster,
            reach: &mut self.reach,
            rewrite: &mut self.rewrite,
            walked: &mut self.walked,
            updates,
        }
        .run_layout(
            (&walk, root),
            &self.diagnostics,
            (self.scale, text),
            window,
            areas,
        );
        if window.is_some() && updated == Some(UpdatedReach::Lost) {
            raster.reach_into(self.frame, &mut self.reach);
        }
        if std::mem::take(&mut raster.hits_changed) {
            self.hits.get_mut().mark_stale();
        }
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
        self.walked.clear();
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
                if let Changed::Areas(changed) = &mut self.changed {
                    let mut areas = Vec::new();
                    damage::changed_areas(&old, &self.reach, &raster, self.frame, &mut areas);
                    changed.append(&mut areas);
                    damage::merge_areas(changed);
                }
                self.give_back(old);
            }
            None => self.changed = Changed::Whole,
        }
        // While the whole frame waits for a host, the host's take measures
        // where items reach.
        match self.changed {
            Changed::Areas(_) => raster.reach_into(self.frame, &mut self.reach),
            Changed::Whole => self.reach.clear(),
        }
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

    // Puts into `areas` the parts of `frame` whose pixels changed since the
    // host took its picture last: the whole frame the first time and after
    // a change of size, a build with nothing to compare, or a scale.
    pub(crate) fn take_changes(&mut self, frame: ClipBounds, areas: &mut Vec<PixelRect>) {
        areas.clear();
        let whole = frame != self.frame || matches!(self.changed, Changed::Whole);
        // Updates leave where items reach unmeasured while no host takes
        // their changes.
        if let Some(raster) = &self.raster
            && (frame != self.frame || self.reach.len() != raster.order.len())
        {
            raster.reach_into(frame, &mut self.reach);
        }
        self.frame = frame;
        match std::mem::replace(&mut self.changed, Changed::Areas(Vec::new())) {
            Changed::Areas(mut changed) if !whole => {
                let clipped = changed
                    .drain(..)
                    .filter_map(|mut area| area.clip_to(frame).then_some(area));
                areas.extend(clipped.map(pixel_rect));
                self.changed = Changed::Areas(changed);
            }
            _ if self.raster.is_some() && frame.max_x > 0 && frame.max_y > 0 => {
                areas.push(pixel_rect(frame));
            }
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
            match self.raster.as_ref() {
                Some(raster) => list.collect_walking(&self.diagnostics, |walk| {
                    let mut holds = Vec::new();
                    collect_hits(
                        walk,
                        &raster.subtrees,
                        &spaces,
                        &HitParent::root(),
                        &mut holds,
                    );
                }),
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

// The recordings the scene's commands keep for their next draws and the
// boxes its walks keep live with the thread: a scene that goes lets them
// go, as no walk of its own takes them again.
impl Drop for Scene {
    fn drop(&mut self) {
        release_walk_storage();
    }
}

// Collects into `walk` the hit targets of the subtree of `records`' first
// record, under `parent`. `holds` takes, past its end, whether the subtree
// of each record holds a target, and gives that room back.
fn collect_hits(
    walk: &mut HitCollector<'_, '_>,
    records: &[SubtreeItems],
    spaces: &[KeptSpace],
    parent: &HitParent,
    holds: &mut Vec<bool>,
) {
    if records.is_empty() {
        return;
    }
    let base = holds.len();
    mark_hit_holders(records, spaces, holds);
    collect_record_hits(walk, (records, base), 0, spaces, parent, holds);
    holds.truncate(base);
}

// Appends to `holds`, for each of `records`, whether its subtree holds a
// hit target.
fn mark_hit_holders(records: &[SubtreeItems], spaces: &[KeptSpace], holds: &mut Vec<bool>) {
    let base = holds.len();
    holds.resize(base + records.len(), false);
    for index in (0..records.len()).rev() {
        let record = &records[index];
        let surface = record.surface();
        let held = if let Some(kept) = kept_space(spaces, record.identity) {
            kept.space
                .layer()
                .is_some_and(|layer| layer.has_hit_targets)
        } else if !surface.is_empty() {
            mark_hit_holders(surface, spaces, holds);
            let held = holds[base + records.len()];
            holds.truncate(base + records.len());
            held
        } else {
            let mut held = record
                .hits
                .as_ref()
                .is_some_and(|hits| hits.hit_test.is_some());
            let mut child = index + 1;
            while !held && child < record.end {
                held = holds[base + child];
                child = records[child].end;
            }
            held
        };
        holds[base + index] = held;
    }
}

// Collects into `walk` the hit targets of the subtree of the record `index`
// of `records`, whose marks start at `base` in `holds`, under `parent`.
fn collect_record_hits(
    walk: &mut HitCollector<'_, '_>,
    (records, base): (&[SubtreeItems], usize),
    index: usize,
    spaces: &[KeptSpace],
    parent: &HitParent,
    holds: &mut Vec<bool>,
) {
    if !holds[base + index] {
        return;
    }
    let record = &records[index];
    // A 3D subtree takes input through its scene's eye, which its layers
    // tell.
    if let Some(kept) = kept_space(spaces, record.identity) {
        if let Some(layer) = kept.space.layer() {
            walk.visit(layer, parent);
        }
        return;
    }
    // A layer that drew its children into a surface of its own keeps their
    // records with its own.
    let surface = record.surface();
    if !surface.is_empty() {
        collect_hits(walk, surface, spaces, parent, holds);
        return;
    }
    let hits = record.hits.as_deref();
    let local_bounds = hits.map_or_else(
        || Rect::from_size(record.node_size),
        |hits| hits.local_bounds,
    );
    let layer = FlatHitLayer {
        node_id: record.identity.0,
        transform_to_parent: record.transform,
        node_rect: hits
            .and_then(|hits| hits.node_bounds)
            .unwrap_or(local_bounds),
        clip: hits.filter(|hits| hits.clips).map(|_| local_bounds),
        hit_test: hits.and_then(|hits| hits.hit_test.as_ref()),
    };
    let Some(entered) = walk.enter(&layer, parent) else {
        return;
    };
    let mut child = index + 1;
    while child < record.end {
        collect_record_hits(walk, (records, base), child, spaces, &entered, holds);
        child = records[child].end;
    }
    walk.leave(&entered);
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
        if let Some(kept) = kept_space(spaces, record.identity) {
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
        // A layer that drew its children into a surface of its own keeps
        // their records with its own.
        let surface = record.surface();
        let (children, first, end) = match surface.first() {
            Some(own) => (surface, 1, own.end),
            None => (records, index + 1, record.end),
        };
        visit_node_bounds(
            (children, spaces),
            (first, end),
            (to_record, surfaces),
            visit,
        );
        index = record.end;
    }
}

// Visits the node of each of `records`, and of the records a layer that drew
// its children into a surface of its own keeps of them.
fn visit_record_nodes(records: &[SubtreeItems], visit: &mut dyn FnMut(NodeId)) {
    for record in records {
        if let Some(node) = record.identity.0 {
            visit(node);
        }
        // The surface's records start with the layer's own.
        if let Some((_, children)) = record.surface().split_first() {
            visit_record_nodes(children, visit);
        }
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

    cranpose_render_common::hit_list_queries!();

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
            visit_record_nodes(&raster.subtrees, visit);
        }
        true
    }
}
