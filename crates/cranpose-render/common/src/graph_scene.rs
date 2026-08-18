use std::cell::{Cell, RefCell};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_foundation::{PointerEvent, PointerEventKind};
use cranpose_ui::{LayoutNode, ModifierNodeSlices, SubcomposeLayoutNode};
use cranpose_ui_graphics::{Point, Rect, RoundedCornerShape};

use crate::graph::{ProjectiveTransform, RenderGraph};
use crate::{HitTestTarget, RenderScene};

pub struct RenderDiagnostics {
    reported_warnings: RefCell<HashSet<&'static str>>,
    live_modifier_slice_lookup_miss_count: Cell<usize>,
}

impl RenderDiagnostics {
    pub fn new() -> Self {
        Self {
            reported_warnings: RefCell::new(HashSet::new()),
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

#[derive(Clone)]
pub struct HitGeometry {
    pub rect: Rect,
    pub quad: [[f32; 2]; 4],
    pub local_bounds: Rect,
    pub world_to_local: ProjectiveTransform,
    pub hit_clip_bounds: Option<Rect>,
    pub hit_clips: Vec<HitClip>,
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
    pub z_index: usize,
    pub hit_clip_bounds: Option<Rect>,
    pub hit_clips: Vec<HitClip>,
    diagnostics: Rc<RenderDiagnostics>,
}

struct HitRegionInit {
    node_id: NodeId,
    capture_path: Vec<NodeId>,
    geometry: HitGeometry,
    shape: Option<RoundedCornerShape>,
    click_actions: Vec<ClickAction>,
    pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    z_index: usize,
    diagnostics: Rc<RenderDiagnostics>,
}

impl HitRegion {
    fn with_diagnostics(init: HitRegionInit) -> Self {
        let HitRegionInit {
            node_id,
            capture_path,
            geometry,
            shape,
            click_actions,
            pointer_inputs,
            z_index,
            diagnostics,
        } = init;
        let HitGeometry {
            rect,
            quad,
            local_bounds,
            world_to_local,
            hit_clip_bounds,
            hit_clips,
        } = geometry;
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

        if let Some(clip_bounds) = self.hit_clip_bounds {
            if !clip_bounds.contains(x, y) {
                return false;
            }
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

        let (local_event, local_position) = self.localize_event(&event);
        Self::dispatch_pointer_inputs(modifier_slices.pointer_inputs(), &local_event);

        if event.kind == PointerEventKind::Down && !local_event.is_consumed() {
            for handler in modifier_slices.click_handlers() {
                handler(local_position);
            }
        }
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

pub struct Scene {
    pub graph: Option<RenderGraph>,
    pub hits: Vec<HitRegion>,
    pub next_hit_z: usize,
    pub node_index: HashMap<NodeId, usize>,
    diagnostics: Rc<RenderDiagnostics>,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            graph: None,
            hits: Vec::new(),
            next_hit_z: 0,
            node_index: HashMap::new(),
            diagnostics: Rc::new(RenderDiagnostics::new()),
        }
    }

    pub fn diagnostics(&self) -> &RenderDiagnostics {
        self.diagnostics.as_ref()
    }

    pub fn push_hit(
        &mut self,
        node_id: NodeId,
        capture_path: Vec<NodeId>,
        geometry: HitGeometry,
        shape: Option<RoundedCornerShape>,
        click_actions: Vec<ClickAction>,
        pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    ) {
        if click_actions.is_empty() && pointer_inputs.is_empty() {
            return;
        }

        let z_index = self.next_hit_z;
        self.next_hit_z += 1;
        let hit_index = self.hits.len();
        self.hits.push(HitRegion::with_diagnostics(HitRegionInit {
            node_id,
            capture_path,
            geometry,
            shape,
            click_actions,
            pointer_inputs,
            z_index,
            diagnostics: Rc::clone(&self.diagnostics),
        }));
        self.node_index.insert(node_id, hit_index);
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
        self.hits.clear();
        self.node_index.clear();
        self.next_hit_z = 0;
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

    fn find_target(&self, node_id: NodeId) -> Option<Self::HitTarget> {
        self.node_index
            .get(&node_id)
            .and_then(|&index| self.hits.get(index))
            .cloned()
    }

    fn retained_draw_nodes(&self) -> Option<Vec<NodeId>> {
        Some(
            self.graph
                .as_ref()
                .map_or_else(Vec::new, RenderGraph::retained_draw_nodes),
        )
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
mod tests {
    use super::*;
    use std::cell::Cell;

    fn rect_to_quad(rect: Rect) -> [[f32; 2]; 4] {
        [
            [rect.x, rect.y],
            [rect.x + rect.width, rect.y],
            [rect.x, rect.y + rect.height],
            [rect.x + rect.width, rect.y + rect.height],
        ]
    }

    fn translated_world_to_local(rect: Rect) -> ProjectiveTransform {
        ProjectiveTransform::translation(-rect.x, -rect.y)
    }

    fn local_bounds_for_rect(rect: Rect) -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            width: rect.width,
            height: rect.height,
        }
    }

    fn hit_geometry_for_rect(rect: Rect) -> HitGeometry {
        HitGeometry {
            rect,
            quad: rect_to_quad(rect),
            local_bounds: local_bounds_for_rect(rect),
            world_to_local: translated_world_to_local(rect),
            hit_clip_bounds: None,
            hit_clips: Vec::new(),
        }
    }

    fn test_diagnostics() -> Rc<RenderDiagnostics> {
        Rc::new(RenderDiagnostics::new())
    }

    fn make_handler(counter: Rc<Cell<u32>>, consume: bool) -> Rc<dyn Fn(PointerEvent)> {
        Rc::new(move |event: PointerEvent| {
            counter.set(counter.get() + 1);
            if consume {
                event.consume();
            }
        })
    }

    #[test]
    fn hit_test_respects_hit_clip() {
        let mut scene = Scene::new();
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let clip = Rect {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 40.0,
        };
        scene.push_hit(
            1,
            vec![1],
            HitGeometry {
                hit_clip_bounds: Some(clip),
                hit_clips: vec![HitClip {
                    quad: rect_to_quad(clip),
                    bounds: clip,
                }],
                ..hit_geometry_for_rect(rect)
            },
            None,
            Vec::new(),
            vec![Rc::new(|_event: PointerEvent| {})],
        );

        assert!(scene.hit_test(60.0, 20.0).is_empty());
        assert_eq!(scene.hit_test(20.0, 20.0).len(), 1);
    }

    #[test]
    fn hit_test_sorts_by_z_without_duplicating_hit_storage() {
        let mut scene = Scene::new();
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        };

        scene.push_hit(
            1,
            vec![1],
            hit_geometry_for_rect(rect),
            None,
            Vec::new(),
            vec![Rc::new(|_event: PointerEvent| {})],
        );
        scene.push_hit(
            2,
            vec![2],
            hit_geometry_for_rect(rect),
            None,
            Vec::new(),
            vec![Rc::new(|_event: PointerEvent| {})],
        );

        assert_eq!(scene.node_index.get(&1), Some(&0));
        assert_eq!(scene.node_index.get(&2), Some(&1));

        let hits = scene.hit_test(10.0, 10.0);
        assert_eq!(
            hits.iter().map(|hit| hit.node_id).collect::<Vec<_>>(),
            vec![2, 1]
        );
        assert_eq!(scene.find_target(1).map(|hit| hit.node_id), Some(1));
        assert_eq!(scene.find_target(2).map(|hit| hit.node_id), Some(2));
    }

    #[test]
    fn hit_test_rejects_points_in_rounded_corner_cutout() {
        let mut scene = Scene::new();
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 40.0,
        };
        scene.push_hit(
            1,
            vec![1],
            hit_geometry_for_rect(rect),
            Some(RoundedCornerShape::uniform(20.0)),
            Vec::new(),
            vec![Rc::new(|_event: PointerEvent| {})],
        );

        assert!(scene.hit_test(1.0, 1.0).is_empty());
        assert_eq!(scene.hit_test(20.0, 20.0).len(), 1);
    }

    #[test]
    fn render_diagnostics_claim_each_warning_key_once() {
        let diagnostics = RenderDiagnostics::new();

        assert!(diagnostics.claim_warning_once("pixels.effect-fallback"));
        assert!(!diagnostics.claim_warning_once("pixels.effect-fallback"));
        assert!(diagnostics.claim_warning_once("pixels.blend-fallback"));
    }

    #[test]
    fn dispatch_stops_after_event_consumed() {
        let count_first = Rc::new(Cell::new(0));
        let count_second = Rc::new(Cell::new(0));

        let hit = HitRegion::with_diagnostics(HitRegionInit {
            node_id: 1,
            capture_path: vec![1],
            geometry: hit_geometry_for_rect(Rect {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 50.0,
            }),
            shape: None,
            click_actions: Vec::new(),
            pointer_inputs: vec![
                make_handler(count_first.clone(), true),
                make_handler(count_second.clone(), false),
            ],
            z_index: 0,
            diagnostics: test_diagnostics(),
        });

        let event = PointerEvent::new(
            PointerEventKind::Down,
            Point { x: 10.0, y: 10.0 },
            Point { x: 10.0, y: 10.0 },
        );
        hit.dispatch(event);

        assert_eq!(count_first.get(), 1);
        assert_eq!(count_second.get(), 0);
    }

    #[test]
    fn dispatch_delivers_terminal_events_after_consumption_for_cleanup() {
        let count_first = Rc::new(Cell::new(0));
        let count_second = Rc::new(Cell::new(0));

        let hit = HitRegion::with_diagnostics(HitRegionInit {
            node_id: 1,
            capture_path: vec![1],
            geometry: hit_geometry_for_rect(Rect {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 50.0,
            }),
            shape: None,
            click_actions: Vec::new(),
            pointer_inputs: vec![
                make_handler(count_first.clone(), true),
                make_handler(count_second.clone(), false),
            ],
            z_index: 0,
            diagnostics: test_diagnostics(),
        });

        for kind in [PointerEventKind::Up, PointerEventKind::Cancel] {
            let event =
                PointerEvent::new(kind, Point { x: 10.0, y: 10.0 }, Point { x: 10.0, y: 10.0 });
            hit.dispatch(event);
        }

        assert_eq!(count_first.get(), 2);
        assert_eq!(count_second.get(), 2);
    }

    #[test]
    fn dispatch_delivers_terminal_events_to_later_captured_targets_after_consumption() {
        let child_count = Rc::new(Cell::new(0));
        let parent_count = Rc::new(Cell::new(0));

        let child_hit = HitRegion::with_diagnostics(HitRegionInit {
            node_id: 2,
            capture_path: vec![2, 1],
            geometry: hit_geometry_for_rect(Rect {
                x: 8.0,
                y: 8.0,
                width: 20.0,
                height: 20.0,
            }),
            shape: None,
            click_actions: Vec::new(),
            pointer_inputs: vec![make_handler(child_count.clone(), true)],
            z_index: 1,
            diagnostics: test_diagnostics(),
        });
        let parent_hit = HitRegion::with_diagnostics(HitRegionInit {
            node_id: 1,
            capture_path: vec![1],
            geometry: hit_geometry_for_rect(Rect {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 50.0,
            }),
            shape: None,
            click_actions: Vec::new(),
            pointer_inputs: vec![make_handler(parent_count.clone(), false)],
            z_index: 0,
            diagnostics: test_diagnostics(),
        });

        let event = PointerEvent::new(
            PointerEventKind::Up,
            Point { x: 12.0, y: 12.0 },
            Point { x: 12.0, y: 12.0 },
        );
        child_hit.dispatch(event.clone());
        parent_hit.dispatch(event);

        assert_eq!(child_count.get(), 1);
        assert_eq!(parent_count.get(), 1);
    }

    #[test]
    fn dispatch_triggers_click_action_on_down() {
        let click_count = Rc::new(Cell::new(0));
        let click_count_for_handler = Rc::clone(&click_count);
        let click_action = ClickAction::Simple(Rc::new(RefCell::new(move || {
            click_count_for_handler.set(click_count_for_handler.get() + 1);
        })));

        let hit = HitRegion::with_diagnostics(HitRegionInit {
            node_id: 1,
            capture_path: vec![1],
            geometry: hit_geometry_for_rect(Rect {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 50.0,
            }),
            shape: None,
            click_actions: vec![click_action],
            pointer_inputs: Vec::new(),
            z_index: 0,
            diagnostics: test_diagnostics(),
        });

        hit.dispatch(PointerEvent::new(
            PointerEventKind::Down,
            Point { x: 10.0, y: 10.0 },
            Point { x: 10.0, y: 10.0 },
        ));
        hit.dispatch(PointerEvent::new(
            PointerEventKind::Move,
            Point { x: 10.0, y: 10.0 },
            Point { x: 12.0, y: 12.0 },
        ));

        assert_eq!(click_count.get(), 1);
    }

    #[test]
    fn dispatch_passes_local_position_to_click_action() {
        let local_positions = Rc::new(RefCell::new(Vec::new()));
        let local_positions_for_handler = Rc::clone(&local_positions);
        let click_action = ClickAction::WithPoint(Rc::new(move |point| {
            local_positions_for_handler.borrow_mut().push(point);
        }));

        let hit = HitRegion::with_diagnostics(HitRegionInit {
            node_id: 1,
            capture_path: vec![1],
            geometry: hit_geometry_for_rect(Rect {
                x: 10.0,
                y: 12.0,
                width: 50.0,
                height: 50.0,
            }),
            shape: None,
            click_actions: vec![click_action],
            pointer_inputs: Vec::new(),
            z_index: 0,
            diagnostics: test_diagnostics(),
        });

        hit.dispatch(PointerEvent::new(
            PointerEventKind::Down,
            Point { x: 15.0, y: 17.0 },
            Point { x: 15.0, y: 17.0 },
        ));

        assert_eq!(*local_positions.borrow(), vec![Point { x: 5.0, y: 5.0 }]);
    }

    #[test]
    fn dispatch_does_not_trigger_click_action_when_consumed() {
        let click_count = Rc::new(Cell::new(0));
        let click_count_for_handler = Rc::clone(&click_count);
        let click_action = ClickAction::Simple(Rc::new(RefCell::new(move || {
            click_count_for_handler.set(click_count_for_handler.get() + 1);
        })));

        let hit = HitRegion::with_diagnostics(HitRegionInit {
            node_id: 1,
            capture_path: vec![1],
            geometry: hit_geometry_for_rect(Rect {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 50.0,
            }),
            shape: None,
            click_actions: vec![click_action],
            pointer_inputs: vec![Rc::new(|event: PointerEvent| event.consume())],
            z_index: 0,
            diagnostics: test_diagnostics(),
        });

        hit.dispatch(PointerEvent::new(
            PointerEventKind::Down,
            Point { x: 10.0, y: 10.0 },
            Point { x: 10.0, y: 10.0 },
        ));

        assert_eq!(click_count.get(), 0);
    }

    #[test]
    fn hit_test_uses_exact_quad_for_transformed_region() {
        let mut scene = Scene::new();
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 20.0,
        };
        let quad = [[10.0, 10.0], [50.0, 10.0], [20.0, 30.0], [60.0, 30.0]];
        let world_to_local = ProjectiveTransform::from_rect_to_quad(rect, quad)
            .inverse()
            .expect("transformed hit region should be invertible");
        scene.push_hit(
            1,
            vec![1],
            HitGeometry {
                rect: Rect {
                    x: 10.0,
                    y: 10.0,
                    width: 50.0,
                    height: 20.0,
                },
                quad,
                local_bounds: rect,
                world_to_local,
                hit_clip_bounds: None,
                hit_clips: Vec::new(),
            },
            None,
            Vec::new(),
            vec![Rc::new(|_event: PointerEvent| {})],
        );

        assert!(
            scene.hit_test(15.0, 28.0).is_empty(),
            "point inside the quad bounds but outside the transformed quad must not hit"
        );
        assert_eq!(scene.hit_test(30.0, 20.0).len(), 1);
    }

    #[test]
    fn dispatch_uses_inverse_transform_for_local_position() {
        let local_positions = Rc::new(RefCell::new(Vec::new()));
        let local_positions_for_handler = Rc::clone(&local_positions);
        let click_action = ClickAction::WithPoint(Rc::new(move |point| {
            local_positions_for_handler.borrow_mut().push(point);
        }));
        let local_bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 10.0,
        };
        let quad = [[20.0, 10.0], [60.0, 10.0], [20.0, 30.0], [60.0, 30.0]];
        let world_to_local = ProjectiveTransform::from_rect_to_quad(local_bounds, quad)
            .inverse()
            .expect("translated quad should be invertible");
        let hit = HitRegion::with_diagnostics(HitRegionInit {
            node_id: 1,
            capture_path: vec![1],
            geometry: HitGeometry {
                rect: Rect {
                    x: 20.0,
                    y: 10.0,
                    width: 40.0,
                    height: 20.0,
                },
                quad,
                local_bounds,
                world_to_local,
                hit_clip_bounds: None,
                hit_clips: Vec::new(),
            },
            shape: None,
            click_actions: vec![click_action],
            pointer_inputs: Vec::new(),
            z_index: 0,
            diagnostics: test_diagnostics(),
        });

        hit.dispatch(PointerEvent::new(
            PointerEventKind::Down,
            Point { x: 25.0, y: 17.0 },
            Point { x: 25.0, y: 17.0 },
        ));

        assert_eq!(*local_positions.borrow(), vec![Point { x: 2.5, y: 3.5 }]);
    }

    #[test]
    fn dispatch_with_applier_counts_live_modifier_slice_lookup_misses() {
        let handler_calls = Rc::new(Cell::new(0));
        let handler_calls_for_handler = Rc::clone(&handler_calls);
        let diagnostics = test_diagnostics();
        let hit = HitRegion::with_diagnostics(HitRegionInit {
            node_id: 42,
            capture_path: vec![42],
            geometry: hit_geometry_for_rect(Rect {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 50.0,
            }),
            shape: None,
            click_actions: Vec::new(),
            pointer_inputs: vec![Rc::new(move |_event: PointerEvent| {
                handler_calls_for_handler.set(handler_calls_for_handler.get() + 1);
            })],
            z_index: 0,
            diagnostics: Rc::clone(&diagnostics),
        });
        let misses_before = diagnostics.live_modifier_slice_lookup_miss_count();
        let mut applier = MemoryApplier::new();

        hit.dispatch_with_applier(
            &mut applier,
            PointerEvent::new(
                PointerEventKind::Down,
                Point { x: 10.0, y: 10.0 },
                Point { x: 10.0, y: 10.0 },
            ),
        );

        assert_eq!(handler_calls.get(), 1);
        assert_eq!(
            diagnostics.live_modifier_slice_lookup_miss_count(),
            misses_before + 1
        );
    }
}
