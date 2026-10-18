use std::{
    cell::{Cell, RefCell},
    cmp::Reverse,
    rc::Rc,
};

use cranpose_core::{
    MemoryApplier, NodeId,
    collections::map::{HashMap, HashSet},
};
use cranpose_foundation::{MINIMUM_TOUCH_TARGET_SIZE, PointerEvent, PointerEventKind};
use cranpose_ui::{LayoutNode, ModifierNodeSlices, SubcomposeLayoutNode};
use cranpose_ui_graphics::{Point, PointerIcon, Rect, RoundedCornerShape};

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
        let content = target.pick.content_point(window, target.index)?;
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

    /// The local point under the window point for a gesture the target
    /// holds, which may leave the surface.
    fn drag_point(&self, window: Point) -> Option<Point> {
        let target = &self.target;
        let content = target.pick.drag_point(window, target.index)?;
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
            node_id: 0,
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
                .drag_point(event.global_position)
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

/// The retained render graph of a window together with the hit targets built from it.
pub struct Scene {
    /// The retained render graph, or `None` before the first build.
    pub graph: Option<RenderGraph>,
    /// Hit targets in draw order, the last drawn at the end.
    pub hits: Vec<HitRegion>,
    hit_buffers: Vec<HitBuffers>,
    /// Draw order index that the next pushed hit target receives.
    pub next_hit_z: usize,
    /// Position in `hits` of the target of each node, for finding a target by node id.
    pub node_index: HashMap<NodeId, usize>,
    diagnostics: Rc<RenderDiagnostics>,
}

impl Scene {
    /// Creates an empty scene with no graph and no hit targets.
    pub fn new() -> Self {
        Self {
            graph: None,
            hits: Vec::new(),
            hit_buffers: Vec::new(),
            next_hit_z: 0,
            node_index: HashMap::default(),
            diagnostics: Rc::new(RenderDiagnostics::new()),
        }
    }

    /// The diagnostics shared by the scene and its hit targets.
    pub fn diagnostics(&self) -> &RenderDiagnostics {
        self.diagnostics.as_ref()
    }

    /// Rebuilds the retained graph and hit targets from the applier, reusing
    /// storage from the previous graph.
    pub fn rebuild_from_applier(&mut self, applier: &MemoryApplier, root: NodeId) {
        let previous = self.graph.take();
        self.clear_hits();
        let Some(mut graph) =
            crate::scene_builder::rebuild_graph_from_applier(applier, root, 1.0, previous)
        else {
            return;
        };
        follow_placed_viewers(&mut graph);
        self.install_graph_with_hits(graph);
    }

    fn install_graph_with_hits(&mut self, graph: RenderGraph) {
        crate::hit_graph::collect_hits_from_graph(
            &graph.root,
            ProjectiveTransform::identity(),
            self,
            (None, &graph.viewers),
        );
        self.replace_graph(graph);
    }

    /// Applies content and layer-property changes to the retained graph.
    /// Empty updates or an unavailable scoped update rebuild the scene.
    /// Hit targets are refreshed when their geometry changes or `refresh_hits`
    /// is true, as required after layout or pointer-handler changes.
    pub fn update_from_applier(
        &mut self,
        applier: &MemoryApplier,
        root: NodeId,
        updates: SceneUpdates<'_>,
        refresh_hits: bool,
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
        let walked = self.graph.as_mut().is_some_and(follow_placed_viewers);
        if !refresh_hits && !report.hit_graph_dirty && !walked {
            return;
        }
        self.clear_hits();
        if let Some(graph) = self.graph.take() {
            self.install_graph_with_hits(graph);
        }
    }

    /// Adds an interactive target in draw order, ignoring targets that neither
    /// handle a pointer nor name a pointer icon.
    pub fn push_hit(
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
        let mut buffers = self.hit_buffers.pop().unwrap_or_default();
        buffers.capture_path.extend_from_slice(capture_path);

        let z_index = self.next_hit_z;
        self.next_hit_z += 1;
        let hit_index = self.hits.len();
        self.hits.push(HitRegion::with_diagnostics(HitRegionInit {
            node_id,
            capture_path: buffers.capture_path,
            geometry,
            clip_buffer: buffers.hit_clips,
            shape,
            handlers: Rc::clone(handlers),
            z_index,
            diagnostics: Rc::clone(&self.diagnostics),
        }));
        self.node_index.insert(node_id, hit_index);
    }

    /// Removes all hit targets and releases their handlers while retaining vector capacity.
    pub fn clear_hits(&mut self) {
        self.hit_buffers.clear();
        for hit in self.hits.drain(..) {
            let mut buffers = HitBuffers {
                hit_clips: hit.hit_clips,
                capture_path: hit.capture_path,
            };
            buffers.hit_clips.clear();
            buffers.capture_path.clear();
            self.hit_buffers.push(buffers);
        }
        self.node_index.clear();
        self.next_hit_z = 0;
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

/// Follows the viewers of `graph`'s scenes through the doorways their eyes
/// passed, when the build or update that made `graph` placed a scene's
/// layer: a flat window asks nothing of its layers. True when an eye
/// entered or left a space.
fn follow_placed_viewers(graph: &mut RenderGraph) -> bool {
    crate::thread_state::with(|state| state.scene_placed.replace(false))
        && crate::scene_space::follow_viewers(graph)
}

impl RenderScene for Scene {
    type HitTarget = HitRegion;

    fn clear(&mut self) {
        self.graph = None;
        self.clear_hits();
    }

    fn hit_test(&self, x: f32, y: f32) -> Vec<Self::HitTarget> {
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

    fn hit_test_near(&self, x: f32, y: f32) -> Option<Self::HitTarget> {
        self.hits
            .iter()
            .filter_map(|hit| hit.reach_distance(x, y).map(|distance| (distance, hit)))
            .min_by(|(near, hit), (other_near, other)| {
                near.total_cmp(other_near)
                    .then_with(|| other.z_index.cmp(&hit.z_index))
            })
            .map(|(_, hit)| hit.clone())
    }

    fn find_target(&self, node_id: NodeId) -> Option<Self::HitTarget> {
        self.node_index
            .get(&node_id)
            .and_then(|&index| self.hits.get(index))
            .cloned()
    }

    fn surface_window_bounds(&self, node_id: NodeId) -> Option<Rect> {
        let hit = self.hits.get(*self.node_index.get(&node_id)?)?;
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
