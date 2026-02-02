//! Scene structures for GPU rendering

use cranpose_core::NodeId;
use cranpose_foundation::PointerEvent;
use cranpose_render_common::{HitTestTarget, RenderScene};
use cranpose_ui_graphics::{Brush, Color, Point, Rect, RoundedCornerShape};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone)]
pub enum ClickAction {
    Simple(Rc<RefCell<dyn FnMut()>>),
    WithPoint(Rc<dyn Fn(Point)>),
}

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
    pub font_size: f32,
    pub scale: f32,
    pub z_index: usize,
    pub clip: Option<Rect>,
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
        // Simple rect check + shape check if needed
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
        let local_position = Point {
            x: event.position.x - self.rect.x,
            y: event.position.y - self.rect.y,
        };
        let local_event = event.copy_with_local_position(local_position);
        for handler in &self.pointer_inputs {
            handler(local_event.clone());
        }
    }
}

pub struct Scene {
    pub shapes: Vec<DrawShape>,
    pub texts: Vec<TextDraw>,
    pub hits: Vec<HitRegion>,
    pub next_z: usize,
    pub node_index: HashMap<NodeId, HitRegion>,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            shapes: Vec::new(),
            texts: Vec::new(),
            hits: Vec::new(),
            next_z: 0,
            node_index: HashMap::new(),
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

    #[allow(clippy::too_many_arguments)]
    pub fn push_text(
        &mut self,
        node_id: NodeId,
        rect: Rect,
        text: Rc<str>,
        color: Color,
        font_size: f32,
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
            font_size,
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
        let mut hits = self.hits.clone();
        hits.retain(|hit| hit.contains(x, y));

        // Sort by z-index descending (top to bottom)
        hits.sort_by(|a, b| b.z_index.cmp(&a.z_index));
        hits
    }

    fn find_target(&self, node_id: NodeId) -> Option<Self::HitTarget> {
        // O(1) lookup using the node index
        self.node_index.get(&node_id).cloned()
    }
}

// Helper function for rounded rectangle hit testing
fn point_in_rounded_rect(x: f32, y: f32, rect: Rect, shape: RoundedCornerShape) -> bool {
    if !rect.contains(x, y) {
        return false;
    }

    let local_x = x - rect.x;
    let local_y = y - rect.y;

    // Check corners
    let radii = shape.resolve(rect.width, rect.height);
    let tl = radii.top_left;
    let tr = radii.top_right;
    let bl = radii.bottom_left;
    let br = radii.bottom_right;

    // Top-left corner
    if local_x < tl && local_y < tl {
        let dx = tl - local_x;
        let dy = tl - local_y;
        return dx * dx + dy * dy <= tl * tl;
    }

    // Top-right corner
    if local_x > rect.width - tr && local_y < tr {
        let dx = local_x - (rect.width - tr);
        let dy = tr - local_y;
        return dx * dx + dy * dy <= tr * tr;
    }

    // Bottom-left corner
    if local_x < bl && local_y > rect.height - bl {
        let dx = bl - local_x;
        let dy = local_y - (rect.height - bl);
        return dx * dx + dy * dy <= bl * bl;
    }

    // Bottom-right corner
    if local_x > rect.width - br && local_y > rect.height - br {
        let dx = local_x - (rect.width - br);
        let dy = local_y - (rect.height - br);
        return dx * dx + dy * dy <= br * br;
    }

    true
}
