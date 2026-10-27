use std::{
    cell::{Cell, Ref, RefCell},
    cmp::Reverse,
    rc::Rc,
};

use cranpose_core::{
    MemoryApplier, NodeId,
    collections::map::{HashMap, HashSet},
};
use cranpose_foundation::{MINIMUM_TOUCH_TARGET_SIZE, PointerEvent, PointerEventKind};
use cranpose_ui::{LayoutNode, ModifierNodeSlices, SubcomposeLayoutNode};
use cranpose_ui_graphics::{Point, PointerIcon, Rect, RoundedCornerShape, space::Ray};

use crate::{
    HitTestTarget, RenderScene, SceneUpdates,
    graph::{ProjectiveTransform, RenderGraph},
    scene_pick::ScenePick,
};

/// Counters and once-only warnings that a scene collects while it dispatches pointer events.
pub struct RenderDiagnostics {
    reported_warnings: RefCell<HashSet<&'static str>>,
    live_modifier_slice_lookup_miss_count: Cell<usize>,
}

impl RenderDiagnostics {
    /// Creates diagnostics with no warnings reported and no lookup misses counted.
    pub fn new() -> Self {
        Self {
            reported_warnings: RefCell::new(HashSet::default()),
            live_modifier_slice_lookup_miss_count: Cell::new(0),
        }
    }

    /// Returns true the first time it is called with `key`, so that a warning is reported once.
    pub fn claim_warning_once(&self, key: &'static str) -> bool {
        self.reported_warnings.borrow_mut().insert(key)
    }

    /// Counts one pointer dispatch that fell back to the captured modifier slices because the live
    /// node could not be found.
    pub fn record_live_modifier_slice_lookup_miss(&self) {
        self.live_modifier_slice_lookup_miss_count.set(
            self.live_modifier_slice_lookup_miss_count
                .get()
                .saturating_add(1),
        );
    }

    /// Number of pointer dispatches that did not find the live node and used the modifier slices
    /// captured at build time instead.
    pub fn live_modifier_slice_lookup_miss_count(&self) -> usize {
        self.live_modifier_slice_lookup_miss_count.get()
    }
}

impl Default for RenderDiagnostics {
    fn default() -> Self {
        Self::new()
    }
}

/// A clip that cuts a hit target, kept as a transformed rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitClip {
    /// The clip rectangle after the layer transforms, as four corners in window space.
    pub quad: [[f32; 2]; 4],
    /// Axis-aligned bounds of `quad` in window coordinates.
    pub bounds: Rect,
}

/// A mesh surface of a scene, as the hit targets on its content see it:
/// the scene's pick, which surface it is, and the clips above it.
pub struct SurfaceTarget {
    /// Finds the surface and content point a window point shows.
    pub pick: Rc<ScenePick>,
    /// The surface's index in `pick`.
    pub index: usize,
    /// Axis-aligned bounds of the clips above the surface, in window space.
    pub outer_clip_bounds: Option<Rect>,
    /// The clips above the surface, as quads in window space.
    pub outer_clips: Vec<HitClip>,
}

/// How many parts a target's rect splits into along each side for its
/// bounds in the window: on a curved surface the corners alone miss the
/// bulge between them.
const WINDOW_BOUNDS_STEPS: u16 = 2;

impl SurfaceTarget {
    /// Where `local`, a rect of a target's own space into which
    /// `content_to_local` maps the surface's content, shows in the window:
    /// the bounds of a grid of its points in front of the eye; `None` when
    /// none is.
    pub fn window_bounds(
        &self,
        content_to_local: &ProjectiveTransform,
        local: Rect,
    ) -> Option<Rect> {
        let local_to_content = content_to_local.inverse()?;
        let steps = f32::from(WINDOW_BOUNDS_STEPS);
        let grid = (0..=WINDOW_BOUNDS_STEPS)
            .flat_map(|row| (0..=WINDOW_BOUNDS_STEPS).map(move |column| (row, column)));
        grid.filter_map(|(row, column)| {
            let point = Point::new(
                local.x + local.width * f32::from(column) / steps,
                local.y + local.height * f32::from(row) / steps,
            );
            let content = local_to_content.map_point(point);
            self.pick.window_point(self.index, content)
        })
        .map(|point| Rect {
            x: point.x,
            y: point.y,
            width: 0.0,
            height: 0.0,
        })
        .reduce(|bounds, point| bounds.union(point))
    }
}

/// Where a hit target on a mesh surface lies on the surface's content.
#[derive(Clone, Copy)]
pub struct SurfaceGeometry<'a> {
    /// The surface the target is on.
    pub target: &'a Rc<SurfaceTarget>,
    /// Maps the surface's content into the node's local space.
    pub content_to_local: ProjectiveTransform,
    /// Axis-aligned bounds of the clips within the surface, in content
    /// space, or `None` when none clips.
    pub clip_bounds: Option<Rect>,
    /// The clips within the surface, as quads in content space.
    pub clips: &'a [HitClip],
}

impl SurfaceGeometry<'_> {
    /// Where `local`, a rect of the target's own space, shows in the window.
    pub fn window_bounds(&self, local: Rect) -> Option<Rect> {
        self.target.window_bounds(&self.content_to_local, local)
    }
}

/// Geometry for a hit target, borrowing the clip chain until the sink records it.
#[derive(Clone, Copy)]
pub struct HitGeometry<'a> {
    /// Axis-aligned bounds of `quad` in window coordinates.
    pub rect: Rect,
    /// The node's rectangle after the layer transforms, as four corners in window space.
    pub quad: [[f32; 2]; 4],
    /// The node's own rectangle in its local space.
    pub local_bounds: Rect,
    /// Maps window coordinates into the node's local space.
    pub world_to_local: ProjectiveTransform,
    /// Axis-aligned bounds of the ancestors' clips in window space, or `None` when none clips.
    pub hit_clip_bounds: Option<Rect>,
    /// Clips of the ancestors that cut the target, as transformed quads in window space.
    pub hit_clips: &'a [HitClip],
    /// For a target on a mesh surface, where it lies on the surface's
    /// content. The window-space fields above then hold the surface's flat
    /// stand-in, exact only on a flat mesh; the surface decides the hit.
    pub surface: Option<SurfaceGeometry<'a>>,
}

/// What a hit target answers with: the shape that narrows its bounds, and the
/// node's modifier slices, which hold the pointer handlers it dispatches to
/// and the pointer icon it asks for while hovered.
pub struct HitTargetSpec<'a> {
    /// Narrows the target's rectangle to a rounded shape, so a point in a
    /// corner cutout misses it.
    pub shape: Option<RoundedCornerShape>,
    /// The node's slices, shared by the target rather than copied.
    pub handlers: &'a Rc<ModifierNodeSlices>,
}

/// A hit target in window space: the node's geometry, clips and pointer handlers.
#[derive(Clone)]
pub struct HitRegion {
    /// The layout node that owns the target.
    pub node_id: NodeId,
    /// Nodes that keep receiving the move, up and cancel events of a gesture that starts here: this
    /// node first, then its pointer-input ancestors from the nearest outward.
    pub capture_path: Vec<NodeId>,
    /// Axis-aligned bounds of `quad` in window coordinates.
    pub rect: Rect,
    /// The node's rectangle after the layer transforms, as four corners in window space.
    pub quad: [[f32; 2]; 4],
    /// The node's own rectangle in its local space.
    pub local_bounds: Rect,
    /// Maps window coordinates into the node's local space.
    pub world_to_local: ProjectiveTransform,
    /// Rounded shape that narrows the target's rectangle, or `None` for a plain rectangle.
    pub shape: Option<RoundedCornerShape>,
    /// The node's modifier slices, shared with the render graph: its pointer
    /// handlers and pointer icon.
    pub handlers: Rc<ModifierNodeSlices>,
    /// Draw order of the target. A larger value is drawn later and is hit first.
    pub z_index: usize,
    /// Axis-aligned bounds of the ancestors' clips in window space, or `None` when none clips.
    pub hit_clip_bounds: Option<Rect>,
    /// Clips of the ancestors that cut this target, as transformed quads in window space.
    pub hit_clips: Vec<HitClip>,
    /// For a target on a mesh surface, where it lies on the surface; shared,
    /// as few targets are and clones of a target hold the same.
    surface: Option<Rc<SurfaceRegion>>,
    diagnostics: Rc<RenderDiagnostics>,
}

/// A hit target's place on a mesh surface's content.
struct SurfaceRegion {
    target: Rc<SurfaceTarget>,
    content_to_local: ProjectiveTransform,
    clip_bounds: Option<Rect>,
    clips: Vec<HitClip>,
}

impl SurfaceRegion {
    /// The local point the window point shows on the target, when the
    /// target's surface is the nearest there and no clip cuts the point.
    fn local_point(&self, window: Point) -> Option<Point> {
        let target = &self.target;
        if target
            .outer_clip_bounds
            .is_some_and(|clip| !clip.contains(window.x, window.y))
            || !target
                .outer_clips
                .iter()
                .all(|clip| point_in_quad(window, clip.quad))
        {
            return None;
        }
        self.local_in(target.pick.content_point(window, target.index)?)
    }

    /// The local point `ray`, in the space the scene's eye stands in, meets
    /// on the target, when the target's surface is the nearest along it and
    /// no clip within the surface cuts the point.
    fn local_point_along(&self, ray: Ray) -> Option<Point> {
        let (index, hit) = self.target.pick.nearest_along(ray)?;
        (index == self.target.index)
            .then(|| self.local_in(hit.point))
            .flatten()
    }

    /// The local point of content point `content`, unless a clip within
    /// the surface cuts it.
    fn local_in(&self, content: Point) -> Option<Point> {
        if self
            .clip_bounds
            .is_some_and(|clip| !clip.contains(content.x, content.y))
            || !self
                .clips
                .iter()
                .all(|clip| point_in_quad(content, clip.quad))
        {
            return None;
        }
        self.content_to_local.map_point_in_front(content)
    }

    /// The local point under the window point, or along the event's ray,
    /// for a gesture the target holds, which may leave the surface.
    fn drag_point(&self, event: &PointerEvent) -> Option<Point> {
        let target = &self.target;
        let content = match event.ray {
            Some(ray) => target.pick.drag_point_along(ray, target.index)?,
            None => target
                .pick
                .drag_point(event.global_position, target.index)?,
        };
        Some(self.content_to_local.map_point(content))
    }
}

struct HitRegionInit<'a> {
    node_id: NodeId,
    capture_path: Vec<NodeId>,
    geometry: HitGeometry<'a>,
    clip_buffer: Vec<HitClip>,
    shape: Option<RoundedCornerShape>,
    handlers: Rc<ModifierNodeSlices>,
    z_index: usize,
    diagnostics: Rc<RenderDiagnostics>,
}

impl Default for HitRegionInit<'_> {
    fn default() -> Self {
        Self {
            node_id: NodeId(0),
            capture_path: Vec::new(),
            geometry: HitGeometry {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                },
                quad: [[0.0, 0.0]; 4],
                local_bounds: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                },
                world_to_local: ProjectiveTransform::identity(),
                hit_clip_bounds: None,
                hit_clips: &[],
                surface: None,
            },
            clip_buffer: Vec::new(),
            shape: None,
            handlers: Rc::default(),
            z_index: 0,
            diagnostics: Rc::new(RenderDiagnostics::new()),
        }
    }
}

impl HitRegion {
    fn with_diagnostics(init: HitRegionInit<'_>) -> Self {
        let HitRegionInit {
            node_id,
            capture_path,
            geometry,
            clip_buffer: mut hit_clips,
            shape,
            handlers,
            z_index,
            diagnostics,
        } = init;
        let HitGeometry {
            rect,
            quad,
            local_bounds,
            world_to_local,
            hit_clip_bounds,
            hit_clips: clips,
            surface,
        } = geometry;
        hit_clips.extend_from_slice(clips);
        Self {
            node_id,
            capture_path,
            rect,
            quad,
            local_bounds,
            world_to_local,
            shape,
            handlers,
            z_index,
            hit_clip_bounds,
            hit_clips,
            surface: surface.map(|surface| {
                Rc::new(SurfaceRegion {
                    target: Rc::clone(surface.target),
                    content_to_local: surface.content_to_local,
                    clip_bounds: surface.clip_bounds,
                    clips: surface.clips.to_vec(),
                })
            }),
            diagnostics,
        }
    }

    fn contains(&self, x: f32, y: f32) -> bool {
        if let Some(surface) = &self.surface {
            return surface
                .local_point(Point { x, y })
                .is_some_and(|local| self.contains_local(local));
        }
        if !self.rect.contains(x, y) {
            return false;
        }

        if let Some(clip_bounds) = self.hit_clip_bounds
            && !clip_bounds.contains(x, y)
        {
            return false;
        }

        let point = Point { x, y };
        if !point_in_quad(point, self.quad) {
            return false;
        }

        for clip in &self.hit_clips {
            if !point_in_quad(point, clip.quad) {
                return false;
            }
        }

        let Some(local_point) = self.world_to_local.map_point_in_front(point) else {
            return false;
        };
        self.contains_local(local_point)
    }

    fn contains_local(&self, local_point: Point) -> bool {
        if let Some(shape) = self.shape {
            point_in_rounded_rect(local_point, self.local_bounds, shape)
        } else {
            self.local_bounds.contains(local_point.x, local_point.y)
        }
    }

    /// The squared distance from the point to this target when the point lies
    /// inside the target grown to the minimum touch size and the target takes
    /// input. None when the target is large enough on its own, when the point
    /// is outside its reach, or when a clip cuts the point off.
    fn reach_distance(&self, x: f32, y: f32) -> Option<f32> {
        if self.handlers.pointer_inputs().is_empty() || self.surface.is_some() {
            return None;
        }
        if let Some(clip_bounds) = self.hit_clip_bounds
            && !clip_bounds.contains(x, y)
        {
            return None;
        }
        let grow_x = ((MINIMUM_TOUCH_TARGET_SIZE - self.rect.width) / 2.0).max(0.0);
        let grow_y = ((MINIMUM_TOUCH_TARGET_SIZE - self.rect.height) / 2.0).max(0.0);
        if grow_x <= 0.0 && grow_y <= 0.0 {
            return None;
        }
        let right = self.rect.x + self.rect.width;
        let bottom = self.rect.y + self.rect.height;
        let in_reach = x >= self.rect.x - grow_x
            && x <= right + grow_x
            && y >= self.rect.y - grow_y
            && y <= bottom + grow_y;
        if !in_reach {
            return None;
        }
        let dx = (self.rect.x - x).max(x - right).max(0.0);
        let dy = (self.rect.y - y).max(y - bottom).max(0.0);
        Some(dx * dx + dy * dy)
    }

    fn localize_event(&self, event: &PointerEvent) -> PointerEvent {
        let local = match &self.surface {
            Some(surface) => surface
                .drag_point(event)
                .unwrap_or_else(|| self.world_to_local.map_point(event.global_position)),
            None => self.world_to_local.map_point(event.global_position),
        };
        event.copy_with_local_position(Point {
            x: local.x - self.local_bounds.x,
            y: local.y - self.local_bounds.y,
        })
    }

    fn dispatch_modifier_slices(&self, modifier_slices: &ModifierNodeSlices, event: PointerEvent) {
        if should_skip_consumed_event(&event) {
            return;
        }

        modifier_slices.dispatch_pointer_event(self.localize_event(&event));
    }

    fn live_modifier_slices(&self, applier: &mut MemoryApplier) -> Option<Rc<ModifierNodeSlices>> {
        if let Ok(modifier_slices) =
            applier.with_node::<LayoutNode, _>(self.node_id, |node| node.modifier_slices_snapshot())
        {
            return Some(modifier_slices);
        }

        applier
            .with_node::<SubcomposeLayoutNode, _>(self.node_id, |node| {
                node.modifier_slices_snapshot()
            })
            .ok()
    }
}

fn is_terminal_pointer_event(kind: PointerEventKind) -> bool {
    matches!(kind, PointerEventKind::Up | PointerEventKind::Cancel)
}

fn should_skip_consumed_event(event: &PointerEvent) -> bool {
    event.is_consumed() && !is_terminal_pointer_event(event.kind)
}

impl HitTestTarget for HitRegion {
    fn node_id(&self) -> NodeId {
        self.node_id
    }

    fn pointer_icon(&self) -> Option<PointerIcon> {
        self.handlers.pointer_icon().cloned()
    }

    fn capture_path(&self) -> Vec<NodeId> {
        self.capture_path.clone()
    }

    fn dispatch(&self, event: PointerEvent) {
        self.dispatch_modifier_slices(&self.handlers, event);
    }

    fn dispatch_with_applier(&self, applier: &mut MemoryApplier, event: PointerEvent) {
        if let Some(modifier_slices) = self.live_modifier_slices(applier) {
            self.dispatch_modifier_slices(modifier_slices.as_ref(), event);
            return;
        }

        self.diagnostics.record_live_modifier_slice_lookup_miss();
        self.dispatch_modifier_slices(&self.handlers, event);
    }
}

#[derive(Default)]
struct HitBuffers {
    hit_clips: Vec<HitClip>,
    capture_path: Vec<NodeId>,
}

// The hit targets of a scene's graph in draw order, the last drawn at the
// end, and the storage the next collection of them reuses.
#[derive(Default)]
struct HitList {
    hits: Vec<HitRegion>,
    buffers: Vec<HitBuffers>,
    // The draw order index the next pushed target receives.
    next_z: usize,
    // Where in `hits` the target of each node lies.
    node_index: HashMap<NodeId, usize>,
    // The graph changed a target since the list was collected: the next
    // query collects it again.
    stale: bool,
}

impl HitList {
    // Removes the targets, releases their handlers and keeps their storage.
    fn clear(&mut self) {
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

/// The hit targets of a [`Scene`], borrowed apart from its graph, so a walk
/// of the graph pushes targets while it reads the graph in place.
pub(crate) struct HitTargets<'a> {
    list: &'a mut HitList,
    diagnostics: &'a Rc<RenderDiagnostics>,
}

impl HitTargets<'_> {
    /// Adds an interactive target in draw order (see [`Scene::push_hit`]).
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

/// The retained render graph of a window together with the hit targets built from it.
pub struct Scene {
    /// The retained render graph, or `None` before the first build.
    pub graph: Option<RenderGraph>,
    // Collected from the graph when a query first needs them after it
    // changed: a frame that moves a target and takes no pointer input does
    // not walk the graph for targets.
    hits: RefCell<HitList>,
    diagnostics: Rc<RenderDiagnostics>,
}

/// How many hit targets a [`Scene`] holds and has room for, and the same for
/// its index of them by node, for debugging memory use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HitStorage {
    /// Hit targets held.
    pub hits_len: usize,
    /// Room reserved for hit targets.
    pub hits_capacity: usize,
    /// Nodes indexed.
    pub node_index_len: usize,
    /// Room reserved in the node index.
    pub node_index_capacity: usize,
}

impl Scene {
    /// Creates an empty scene with no graph and no hit targets.
    pub fn new() -> Self {
        Self {
            graph: None,
            hits: RefCell::default(),
            diagnostics: Rc::new(RenderDiagnostics::new()),
        }
    }

    /// The diagnostics shared by the scene and its hit targets.
    pub fn diagnostics(&self) -> &RenderDiagnostics {
        self.diagnostics.as_ref()
    }

    /// Rebuilds the retained graph from the applier, reusing storage from the
    /// previous graph. The hit targets follow the new graph.
    pub fn rebuild_from_applier(&mut self, applier: &MemoryApplier, root: NodeId) {
        let previous = self.graph.take();
        self.hits.get_mut().stale = true;
        let Some(graph) =
            crate::scene_builder::rebuild_graph_from_applier(applier, root, 1.0, previous)
        else {
            return;
        };
        crate::scene_builder::publish_space(applier, &graph);
        self.graph = Some(graph);
    }

    // The hit targets, collected from the graph first when it changed since
    // they were.
    fn fresh_hits(&self) -> Ref<'_, HitList> {
        if self.hits.borrow().stale {
            let mut list = self.hits.borrow_mut();
            list.clear();
            list.stale = false;
            if let Some(graph) = &self.graph {
                crate::hit_graph::collect_hits_from_graph(
                    &graph.root,
                    ProjectiveTransform::identity(),
                    &mut HitTargets {
                        list: &mut list,
                        diagnostics: &self.diagnostics,
                    },
                    None,
                );
            }
        }
        self.hits.borrow()
    }

    /// The hit targets in draw order, the last drawn at the end.
    pub fn hit_targets(&self) -> Vec<HitRegion> {
        self.fresh_hits().hits.clone()
    }

    /// How much storage the hit targets take.
    pub fn hit_storage(&self) -> HitStorage {
        let list = self.hits.borrow();
        HitStorage {
            hits_len: list.hits.len(),
            hits_capacity: list.hits.capacity(),
            node_index_len: list.node_index.len(),
            node_index_capacity: list.node_index.capacity(),
        }
    }

    /// Applies content and layer-property changes to the retained graph.
    /// Empty updates or an unavailable scoped update rebuild the scene.
    /// The hit targets follow the graph when the update changed a hit target
    /// or the geometry above one: each updated layer compares its hit state,
    /// and the layers beneath one built again, before and after.
    pub fn update_from_applier(
        &mut self,
        applier: &MemoryApplier,
        root: NodeId,
        updates: SceneUpdates<'_>,
    ) {
        let report = if updates.is_empty() {
            None
        } else {
            self.graph.as_mut().map(|graph| {
                crate::scene_builder::update_graph_from_applier_report(applier, graph, updates, 1.0)
            })
        };
        let Some(report) = report.filter(|report| report.applied()) else {
            self.rebuild_from_applier(applier, root);
            return;
        };
        if let Some(graph) = &self.graph {
            crate::scene_builder::publish_space(applier, graph);
        }
        if report.hit_graph_dirty {
            self.hits.get_mut().stale = true;
        }
    }

    // Adds an interactive target in draw order, ignoring targets that neither
    // handle a pointer nor name a pointer icon.
    pub(crate) fn push_hit(
        &mut self,
        node_id: NodeId,
        capture_path: &[NodeId],
        geometry: HitGeometry<'_>,
        target: HitTargetSpec<'_>,
    ) {
        HitTargets {
            list: self.hits.get_mut(),
            diagnostics: &self.diagnostics,
        }
        .push(node_id, capture_path, geometry, target);
    }

    // Removes all hit targets and releases their handlers while retaining
    // their storage.
    fn clear_hits(&mut self) {
        let list = self.hits.get_mut();
        list.clear();
        list.stale = false;
    }

    /// Replaces the retained render graph and leaves the hit targets as they are.
    pub fn replace_graph(&mut self, graph: RenderGraph) {
        self.graph = Some(graph);
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderScene for Scene {
    type HitTarget = HitRegion;

    fn clear(&mut self) {
        self.graph = None;
        self.clear_hits();
    }

    fn hit_test(&self, x: f32, y: f32) -> Vec<Self::HitTarget> {
        let list = self.fresh_hits();
        let mut hit_indices: Vec<usize> = list
            .hits
            .iter()
            .enumerate()
            .filter_map(|(index, hit)| hit.contains(x, y).then_some(index))
            .collect();

        hit_indices.sort_by_key(|&index| Reverse(list.hits[index].z_index));
        hit_indices
            .into_iter()
            .map(|index| list.hits[index].clone())
            .collect()
    }

    fn hit_test_near(&self, x: f32, y: f32) -> Option<Self::HitTarget> {
        self.fresh_hits()
            .hits
            .iter()
            .filter_map(|hit| hit.reach_distance(x, y).map(|distance| (distance, hit)))
            .min_by(|(near, hit), (other_near, other)| {
                near.total_cmp(other_near)
                    .then_with(|| other.z_index.cmp(&hit.z_index))
            })
            .map(|(_, hit)| hit.clone())
    }

    fn find_target(&self, node_id: NodeId) -> Option<Self::HitTarget> {
        let list = self.fresh_hits();
        list.node_index
            .get(&node_id)
            .and_then(|&index| list.hits.get(index))
            .cloned()
    }

    fn hit_test_ray(&self, scene: NodeId, ray: Ray) -> Vec<Self::HitTarget> {
        let list = self.fresh_hits();
        let mut hit: Vec<&HitRegion> = list
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

    fn surface_windows(&self, visit: &mut dyn FnMut(NodeId, ProjectiveTransform)) {
        let Some(graph) = &self.graph else {
            return;
        };
        let root = ProjectiveTransform::identity();
        crate::space::for_each_window_eye(&graph.root, root, &mut |_, view, to_window| {
            for seen in &view.surfaces {
                if let (Some(node_id), Some(content)) =
                    (seen.layer.node_id, view.content_to_window(seen, to_window))
                {
                    visit(node_id, content);
                }
            }
        });
    }

    fn surface_window_bounds(&self, node_id: NodeId) -> Option<Rect> {
        let list = self.fresh_hits();
        let hit = list.hits.get(*list.node_index.get(&node_id)?)?;
        let surface = hit.surface.as_ref()?;
        surface
            .target
            .window_bounds(&surface.content_to_local, hit.local_bounds)
    }

    fn collect_retained_visual_observation_nodes(&self, nodes: &mut HashSet<NodeId>) -> bool {
        if let Some(graph) = &self.graph {
            graph.collect_retained_visual_observation_nodes(nodes);
        } else {
            nodes.clear();
        }
        true
    }
}

fn point_in_rounded_rect(point: Point, rect: Rect, shape: RoundedCornerShape) -> bool {
    if !rect.contains(point.x, point.y) {
        return false;
    }

    let local_x = point.x - rect.x;
    let local_y = point.y - rect.y;
    let radii = shape.resolve(rect.width, rect.height);
    let tl = radii.top_left;
    let tr = radii.top_right;
    let bl = radii.bottom_left;
    let br = radii.bottom_right;

    if local_x < tl && local_y < tl {
        let dx = tl - local_x;
        let dy = tl - local_y;
        return dx * dx + dy * dy <= tl * tl;
    }

    if local_x > rect.width - tr && local_y < tr {
        let dx = local_x - (rect.width - tr);
        let dy = tr - local_y;
        return dx * dx + dy * dy <= tr * tr;
    }

    if local_x < bl && local_y > rect.height - bl {
        let dx = bl - local_x;
        let dy = local_y - (rect.height - bl);
        return dx * dx + dy * dy <= bl * bl;
    }

    if local_x > rect.width - br && local_y > rect.height - br {
        let dx = local_x - (rect.width - br);
        let dy = local_y - (rect.height - br);
        return dx * dx + dy * dy <= br * br;
    }

    true
}

fn point_in_quad(point: Point, quad: [[f32; 2]; 4]) -> bool {
    point_in_triangle(point, quad[0], quad[1], quad[3])
        || point_in_triangle(point, quad[0], quad[3], quad[2])
}

fn point_in_triangle(point: Point, a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> bool {
    let d1 = triangle_sign(point, a, b);
    let d2 = triangle_sign(point, b, c);
    let d3 = triangle_sign(point, c, a);
    let has_negative = d1 < -f32::EPSILON || d2 < -f32::EPSILON || d3 < -f32::EPSILON;
    let has_positive = d1 > f32::EPSILON || d2 > f32::EPSILON || d3 > f32::EPSILON;
    !(has_negative && has_positive)
}

fn triangle_sign(point: Point, a: [f32; 2], b: [f32; 2]) -> f32 {
    (point.x - b[0]) * (a[1] - b[1]) - (a[0] - b[0]) * (point.y - b[1])
}

#[cfg(test)]
#[path = "tests/graph_scene_tests.rs"]
mod tests;
