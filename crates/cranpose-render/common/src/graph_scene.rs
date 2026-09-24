use std::{
    cell::{Cell, RefCell},
    cmp::Reverse,
    collections::HashMap,
    rc::Rc,
};

use cranpose_core::{MemoryApplier, NodeId, collections::map::HashSet};
use cranpose_foundation::{MINIMUM_TOUCH_TARGET_SIZE, PointerEvent, PointerEventKind};
use cranpose_ui::{LayoutNode, ModifierNodeSlices, SubcomposeLayoutNode};
use cranpose_ui_graphics::{Point, PointerIcon, Rect, RoundedCornerShape};

use crate::{
    HitTestTarget, RenderScene,
    graph::{ProjectiveTransform, RenderGraph},
};

pub struct RenderDiagnostics {
    reported_warnings: RefCell<HashSet<&'static str>>,
    live_modifier_slice_lookup_miss_count: Cell<usize>,
}

impl RenderDiagnostics {
    pub fn new() -> Self {
        Self {
            reported_warnings: RefCell::new(HashSet::default()),
            live_modifier_slice_lookup_miss_count: Cell::new(0),
        }
    }

    pub fn claim_warning_once(&self, key: &'static str) -> bool {
        self.reported_warnings.borrow_mut().insert(key)
    }

    pub fn record_live_modifier_slice_lookup_miss(&self) {
        self.live_modifier_slice_lookup_miss_count.set(
            self.live_modifier_slice_lookup_miss_count
                .get()
                .saturating_add(1),
        );
    }

    pub fn live_modifier_slice_lookup_miss_count(&self) -> usize {
        self.live_modifier_slice_lookup_miss_count.get()
    }
}

impl Default for RenderDiagnostics {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub enum ClickAction {
    Simple(Rc<RefCell<dyn FnMut()>>),
    WithPoint(Rc<dyn Fn(Point)>),
}

impl ClickAction {
    fn invoke(&self, local_position: Point) {
        match self {
            ClickAction::Simple(handler) => (handler.borrow_mut())(),
            ClickAction::WithPoint(handler) => handler(local_position),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitClip {
    pub quad: [[f32; 2]; 4],
    pub bounds: Rect,
}

/// Geometry for a hit target, borrowing the clip chain until the sink records it.
#[derive(Clone, Copy)]
pub struct HitGeometry<'a> {
    pub rect: Rect,
    pub quad: [[f32; 2]; 4],
    pub local_bounds: Rect,
    pub world_to_local: ProjectiveTransform,
    pub hit_clip_bounds: Option<Rect>,
    pub hit_clips: &'a [HitClip],
}

/// What a hit target answers with: the shape that narrows its bounds, the
/// handlers it dispatches to, and the pointer icon it asks for while hovered.
pub struct HitTargetSpec<'a, I> {
    /// Narrows the target's rectangle to a rounded shape, so a point in a
    /// corner cutout misses it.
    pub shape: Option<RoundedCornerShape>,
    /// Click handlers, invoked on an unconsumed press inside the target.
    pub click_actions: I,
    /// Raw pointer handlers, invoked for every event the target receives.
    pub pointer_inputs: &'a [Rc<dyn Fn(PointerEvent)>],
    /// The pointer's appearance while it hovers this target.
    pub pointer_icon: Option<&'a PointerIcon>,
}

#[derive(Clone)]
pub struct HitRegion {
    pub node_id: NodeId,
    pub capture_path: Vec<NodeId>,
    pub rect: Rect,
    pub quad: [[f32; 2]; 4],
    pub local_bounds: Rect,
    pub world_to_local: ProjectiveTransform,
    pub shape: Option<RoundedCornerShape>,
    pub click_actions: Vec<ClickAction>,
    pub pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    pub pointer_icon: Option<PointerIcon>,
    pub z_index: usize,
    pub hit_clip_bounds: Option<Rect>,
    pub hit_clips: Vec<HitClip>,
    diagnostics: Rc<RenderDiagnostics>,
}

struct HitRegionInit<'a> {
    node_id: NodeId,
    capture_path: Vec<NodeId>,
    geometry: HitGeometry<'a>,
    clip_buffer: Vec<HitClip>,
    shape: Option<RoundedCornerShape>,
    click_actions: Vec<ClickAction>,
    pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    pointer_icon: Option<PointerIcon>,
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
            },
            clip_buffer: Vec::new(),
            shape: None,
            click_actions: Vec::new(),
            pointer_inputs: Vec::new(),
            pointer_icon: None,
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
            click_actions,
            pointer_inputs,
            pointer_icon,
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
            click_actions,
            pointer_inputs,
            pointer_icon,
            z_index,
            hit_clip_bounds,
            hit_clips,
            diagnostics,
        }
    }

    fn contains(&self, x: f32, y: f32) -> bool {
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

        let local_point = self.world_to_local.map_point(point);
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
        if self.click_actions.is_empty() && self.pointer_inputs.is_empty() {
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

    fn localize_event(&self, event: &PointerEvent) -> (PointerEvent, Point) {
        let local = self.world_to_local.map_point(event.global_position);
        let local_position = Point {
            x: local.x - self.local_bounds.x,
            y: local.y - self.local_bounds.y,
        };
        (
            event.copy_with_local_position(local_position),
            local_position,
        )
    }

    fn dispatch_pointer_inputs(
        pointer_inputs: &[Rc<dyn Fn(PointerEvent)>],
        local_event: &PointerEvent,
    ) {
        for handler in pointer_inputs {
            if local_event.is_consumed() && !is_terminal_pointer_event(local_event.kind) {
                break;
            }
            handler(local_event.clone());
        }
    }

    fn dispatch_click_actions(&self, local_position: Point) {
        for action in &self.click_actions {
            action.invoke(local_position);
        }
    }

    fn dispatch_modifier_slices(&self, modifier_slices: &ModifierNodeSlices, event: PointerEvent) {
        if should_skip_consumed_event(&event) {
            return;
        }

        let (local_event, _) = self.localize_event(&event);
        modifier_slices.dispatch_pointer_event(local_event);
    }

    fn dispatch_cached_handlers(&self, event: PointerEvent) {
        if should_skip_consumed_event(&event) {
            return;
        }

        let (local_event, local_position) = self.localize_event(&event);
        Self::dispatch_pointer_inputs(&self.pointer_inputs, &local_event);

        if event.kind == PointerEventKind::Down && !local_event.is_consumed() {
            self.dispatch_click_actions(local_position);
        }
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
        self.pointer_icon.clone()
    }

    fn capture_path(&self) -> Vec<NodeId> {
        self.capture_path.clone()
    }

    fn dispatch(&self, event: PointerEvent) {
        self.dispatch_cached_handlers(event);
    }

    fn dispatch_with_applier(&self, applier: &mut MemoryApplier, event: PointerEvent) {
        if let Some(modifier_slices) = self.live_modifier_slices(applier) {
            self.dispatch_modifier_slices(modifier_slices.as_ref(), event);
            return;
        }

        self.diagnostics.record_live_modifier_slice_lookup_miss();
        self.dispatch_cached_handlers(event);
    }
}

#[derive(Default)]
struct HitBuffers {
    hit_clips: Vec<HitClip>,
    capture_path: Vec<NodeId>,
    click_actions: Vec<ClickAction>,
    pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
}

pub struct Scene {
    pub graph: Option<RenderGraph>,
    pub hits: Vec<HitRegion>,
    hit_buffers: Vec<HitBuffers>,
    pub next_hit_z: usize,
    pub node_index: HashMap<NodeId, usize>,
    diagnostics: Rc<RenderDiagnostics>,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            graph: None,
            hits: Vec::new(),
            hit_buffers: Vec::new(),
            next_hit_z: 0,
            node_index: HashMap::new(),
            diagnostics: Rc::new(RenderDiagnostics::new()),
        }
    }

    pub fn diagnostics(&self) -> &RenderDiagnostics {
        self.diagnostics.as_ref()
    }

    /// Adds an interactive target in draw order, ignoring targets that neither
    /// handle a pointer nor name a pointer icon.
    pub fn push_hit<I>(
        &mut self,
        node_id: NodeId,
        capture_path: &[NodeId],
        geometry: HitGeometry<'_>,
        target: HitTargetSpec<'_, I>,
    ) where
        I: IntoIterator<Item = ClickAction>,
    {
        let HitTargetSpec {
            shape,
            click_actions,
            pointer_inputs,
            pointer_icon,
        } = target;
        let mut click_actions = click_actions.into_iter().peekable();
        if click_actions.peek().is_none() && pointer_inputs.is_empty() && pointer_icon.is_none() {
            return;
        }
        let mut buffers = self.hit_buffers.pop().unwrap_or_default();
        buffers.capture_path.extend_from_slice(capture_path);
        buffers.click_actions.extend(click_actions);
        buffers.pointer_inputs.extend_from_slice(pointer_inputs);

        let z_index = self.next_hit_z;
        self.next_hit_z += 1;
        let hit_index = self.hits.len();
        self.hits.push(HitRegion::with_diagnostics(HitRegionInit {
            node_id,
            capture_path: buffers.capture_path,
            geometry,
            clip_buffer: buffers.hit_clips,
            shape,
            click_actions: buffers.click_actions,
            pointer_inputs: buffers.pointer_inputs,
            pointer_icon: pointer_icon.cloned(),
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
                click_actions: hit.click_actions,
                pointer_inputs: hit.pointer_inputs,
            };
            buffers.hit_clips.clear();
            buffers.capture_path.clear();
            buffers.click_actions.clear();
            buffers.pointer_inputs.clear();
            self.hit_buffers.push(buffers);
        }
        self.node_index.clear();
        self.next_hit_z = 0;
    }

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
