use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::rc::Rc;

use cranpose_core::NodeId;
use cranpose_foundation::{PointerEvent, PointerEventKind};
use cranpose_ui_graphics::{Point, Rect, RoundedCornerShape};

use crate::graph::RenderGraph;
use crate::{HitTestTarget, RenderScene};

#[derive(Clone)]
pub enum ClickAction {
    Simple(Rc<RefCell<dyn FnMut()>>),
    WithPoint(Rc<dyn Fn(Point)>),
}

impl ClickAction {
    fn invoke(&self, rect: Rect, x: f32, y: f32) {
        match self {
            ClickAction::Simple(handler) => (handler.borrow_mut())(),
            ClickAction::WithPoint(handler) => handler(Point {
                x: x - rect.x,
                y: y - rect.y,
            }),
        }
    }
}

#[derive(Clone)]
pub struct HitRegion {
    pub node_id: NodeId,
    pub rect: Rect,
    pub shape: Option<RoundedCornerShape>,
    pub click_actions: Vec<ClickAction>,
    pub pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    pub z_index: usize,
    pub hit_clip: Option<Rect>,
}

impl HitRegion {
    fn contains(&self, x: f32, y: f32) -> bool {
        if let Some(clip) = self.hit_clip {
            if !clip.contains(x, y) {
                return false;
            }
        }

        if let Some(shape) = self.shape {
            point_in_rounded_rect(x, y, self.rect, shape)
        } else {
            self.rect.contains(x, y)
        }
    }
}

impl HitTestTarget for HitRegion {
    fn node_id(&self) -> NodeId {
        self.node_id
    }

    fn dispatch(&self, event: PointerEvent) {
        if event.is_consumed() {
            return;
        }

        let x = event.global_position.x;
        let y = event.global_position.y;
        let kind = event.kind;
        let local_position = Point {
            x: x - self.rect.x,
            y: y - self.rect.y,
        };
        let local_event = event.copy_with_local_position(local_position);
        for handler in &self.pointer_inputs {
            if local_event.is_consumed() {
                break;
            }
            handler(local_event.clone());
        }

        if kind == PointerEventKind::Down && !local_event.is_consumed() {
            for action in &self.click_actions {
                action.invoke(self.rect, x, y);
            }
        }
    }
}

pub struct Scene {
    pub graph: Option<RenderGraph>,
    pub hits: Vec<HitRegion>,
    pub next_hit_z: usize,
    pub node_index: HashMap<NodeId, usize>,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            graph: None,
            hits: Vec::new(),
            next_hit_z: 0,
            node_index: HashMap::new(),
        }
    }

    pub fn push_hit(
        &mut self,
        node_id: NodeId,
        rect: Rect,
        shape: Option<RoundedCornerShape>,
        click_actions: Vec<ClickAction>,
        pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
        hit_clip: Option<Rect>,
    ) {
        if click_actions.is_empty() && pointer_inputs.is_empty() {
            return;
        }

        let z_index = self.next_hit_z;
        self.next_hit_z += 1;
        let hit_index = self.hits.len();
        self.hits.push(HitRegion {
            node_id,
            rect,
            shape,
            click_actions,
            pointer_inputs,
            z_index,
            hit_clip,
        });
        self.node_index.insert(node_id, hit_index);
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
}

fn point_in_rounded_rect(x: f32, y: f32, rect: Rect, shape: RoundedCornerShape) -> bool {
    if !rect.contains(x, y) {
        return false;
    }

    let local_x = x - rect.x;
    let local_y = y - rect.y;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

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
            rect,
            None,
            Vec::new(),
            vec![Rc::new(|_event: PointerEvent| {})],
            Some(clip),
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
            rect,
            None,
            Vec::new(),
            vec![Rc::new(|_event: PointerEvent| {})],
            None,
        );
        scene.push_hit(
            2,
            rect,
            None,
            Vec::new(),
            vec![Rc::new(|_event: PointerEvent| {})],
            None,
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
            rect,
            Some(RoundedCornerShape::uniform(20.0)),
            Vec::new(),
            vec![Rc::new(|_event: PointerEvent| {})],
            None,
        );

        assert!(scene.hit_test(1.0, 1.0).is_empty());
        assert_eq!(scene.hit_test(20.0, 20.0).len(), 1);
    }

    #[test]
    fn dispatch_stops_after_event_consumed() {
        let count_first = Rc::new(Cell::new(0));
        let count_second = Rc::new(Cell::new(0));

        let hit = HitRegion {
            node_id: 1,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 50.0,
            },
            shape: None,
            click_actions: Vec::new(),
            pointer_inputs: vec![
                make_handler(count_first.clone(), true),
                make_handler(count_second.clone(), false),
            ],
            z_index: 0,
            hit_clip: None,
        };

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
    fn dispatch_triggers_click_action_on_down() {
        let click_count = Rc::new(Cell::new(0));
        let click_count_for_handler = Rc::clone(&click_count);
        let click_action = ClickAction::Simple(Rc::new(RefCell::new(move || {
            click_count_for_handler.set(click_count_for_handler.get() + 1);
        })));

        let hit = HitRegion {
            node_id: 1,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 50.0,
            },
            shape: None,
            click_actions: vec![click_action],
            pointer_inputs: Vec::new(),
            z_index: 0,
            hit_clip: None,
        };

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

        let hit = HitRegion {
            node_id: 1,
            rect: Rect {
                x: 10.0,
                y: 12.0,
                width: 50.0,
                height: 50.0,
            },
            shape: None,
            click_actions: vec![click_action],
            pointer_inputs: Vec::new(),
            z_index: 0,
            hit_clip: None,
        };

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

        let hit = HitRegion {
            node_id: 1,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 50.0,
            },
            shape: None,
            click_actions: vec![click_action],
            pointer_inputs: vec![Rc::new(|event: PointerEvent| event.consume())],
            z_index: 0,
            hit_clip: None,
        };

        hit.dispatch(PointerEvent::new(
            PointerEventKind::Down,
            Point { x: 10.0, y: 10.0 },
            Point { x: 10.0, y: 10.0 },
        ));

        assert_eq!(click_count.get(), 0);
    }
}
