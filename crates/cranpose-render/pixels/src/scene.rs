use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use cranpose_core::{run_in_mutable_snapshot, NodeId};
use cranpose_foundation::{PointerEvent, PointerEventKind};
use cranpose_render_common::{HitTestTarget, RenderScene};
use cranpose_ui_graphics::{Brush, Color, Rect, RoundedCornerShape};

#[derive(Clone)]
pub struct DrawShape {
    pub rect: Rect,
    pub brush: Brush,
    pub shape: Option<RoundedCornerShape>,
    pub z_index: usize,
    pub clip: Option<Rect>,
}

#[derive(Clone)]
pub struct TextDraw {
    pub node_id: NodeId,
    pub rect: Rect,
    pub text: Rc<str>,
    pub color: Color,
    pub scale: f32,
    pub z_index: usize,
    pub clip: Option<Rect>,
}

#[derive(Clone)]
pub enum ClickAction {
    Simple(Rc<RefCell<dyn FnMut()>>),
    WithPoint(Rc<dyn Fn(cranpose_ui_graphics::Point)>),
}

impl ClickAction {
    fn invoke(&self, rect: Rect, x: f32, y: f32) {
        match self {
            ClickAction::Simple(handler) => (handler.borrow_mut())(),
            ClickAction::WithPoint(handler) => handler(cranpose_ui_graphics::Point {
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

impl HitTestTarget for HitRegion {
    fn dispatch(&self, event: PointerEvent) {
        // If event is already consumed before we even start, we might want to bail early
        // BUT for Move events, we still might want to process hover states?
        // For now, we follow the plan: check consumption inside the loop.

        let x = event.global_position.x;
        let y = event.global_position.y;
        let kind = event.kind;

        let local = cranpose_ui_graphics::Point {
            x: x - self.rect.x,
            y: y - self.rect.y,
        };

        let local_event = event.copy_with_local_position(local);

        let has_pointer_inputs = !self.pointer_inputs.is_empty();
        let has_click_actions = kind == PointerEventKind::Down && !self.click_actions.is_empty();

        if !has_pointer_inputs && !has_click_actions {
            return;
        }

        if let Err(err) = run_in_mutable_snapshot(|| {
            for handler in &self.pointer_inputs {
                // If consumed by a previous handler in this loop (or outer loop), stop.
                if local_event.is_consumed() {
                    break;
                }
                handler(local_event.clone());
            }

            // Only perform click actions if NOT consumed
            if kind == PointerEventKind::Down && !local_event.is_consumed() {
                for action in &self.click_actions {
                    action.invoke(self.rect, x, y);
                }
            }
        }) {
            eprintln!(
                "failed to apply mutable snapshot for pointer event {:?} at ({}, {}): {}",
                kind, x, y, err
            );
        }
    }

    fn node_id(&self) -> NodeId {
        self.node_id
    }
}

impl HitRegion {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        if let Some(clip) = self.hit_clip {
            if !clip.contains(x, y) {
                return false;
            }
        }
        if let Some(shape) = self.shape {
            super::style::point_in_rounded_rect(x, y, self.rect, shape)
        } else {
            self.rect.contains(x, y)
        }
    }
}

pub struct Scene {
    pub shapes: Vec<DrawShape>,
    pub texts: Vec<TextDraw>,
    pub hits: Vec<HitRegion>,
    /// Index for O(1) node lookup by NodeId
    node_index: HashMap<NodeId, HitRegion>,
    next_z: usize,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            shapes: Vec::new(),
            texts: Vec::new(),
            hits: Vec::new(),
            node_index: HashMap::new(),
            next_z: 0,
        }
    }

    pub fn push_shape(
        &mut self,
        rect: Rect,
        brush: Brush,
        shape: Option<RoundedCornerShape>,
        clip: Option<Rect>,
    ) {
        let z_index = self.next_z;
        self.next_z += 1;
        self.shapes.push(DrawShape {
            rect,
            brush,
            shape,
            z_index,
            clip,
        });
    }

    pub fn push_text(
        &mut self,
        node_id: NodeId,
        rect: Rect,
        text: Rc<str>,
        color: Color,
        scale: f32,
        clip: Option<Rect>,
    ) {
        let z_index = self.next_z;
        self.next_z += 1;
        self.texts.push(TextDraw {
            node_id,
            rect,
            text,
            color,
            scale,
            z_index,
            clip,
        });
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
        let z_index = self.next_z;
        self.next_z += 1;
        let hit_region = HitRegion {
            node_id,
            rect,
            shape,
            click_actions,
            pointer_inputs,
            z_index,
            hit_clip,
        };
        // Populate both the list and the index for O(1) lookup
        self.node_index.insert(node_id, hit_region.clone());
        self.hits.push(hit_region);
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
        self.shapes.clear();
        self.texts.clear();
        self.hits.clear();
        self.node_index.clear();
        self.next_z = 0;
    }

    fn hit_test(&self, x: f32, y: f32) -> Vec<Self::HitTarget> {
        let mut hits: Vec<_> = self
            .hits
            .iter()
            .filter(|hit| hit.contains(x, y))
            .cloned()
            .collect();

        // Sort by z-index descending (top to bottom)
        hits.sort_by(|a, b| b.z_index.cmp(&a.z_index));
        hits
    }

    fn find_target(&self, node_id: NodeId) -> Option<Self::HitTarget> {
        // O(1) lookup using the node index
        self.node_index.get(&node_id).cloned()
    }
}
