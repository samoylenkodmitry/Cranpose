//! Scene structures for GPU rendering

use cranpose_core::NodeId;
use cranpose_foundation::{PointerEvent, PointerEventKind};
use cranpose_render_common::{HitTestTarget, RenderScene};
use cranpose_ui_graphics::{
    Brush, Color, ColorFilter, ImageBitmap, Point, Rect, RoundedCornerShape,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

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
pub struct ImageDraw {
    pub rect: Rect,
    pub image: ImageBitmap,
    pub alpha: f32,
    pub color_filter: Option<ColorFilter>,
    pub z_index: usize,
    pub clip: Option<Rect>,
    /// Source sub-region in image-pixel coordinates. `None` means full image.
    pub src_rect: Option<Rect>,
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
    pub shapes: Vec<DrawShape>,
    pub images: Vec<ImageDraw>,
    pub texts: Vec<TextDraw>,
    pub hits: Vec<HitRegion>,
    pub next_z: usize,
    pub node_index: HashMap<NodeId, HitRegion>,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            shapes: Vec::new(),
            images: Vec::new(),
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

    pub fn push_image(
        &mut self,
        rect: Rect,
        image: ImageBitmap,
        alpha: f32,
        color_filter: Option<ColorFilter>,
        clip: Option<Rect>,
        src_rect: Option<Rect>,
    ) {
        let z_index = self.next_z;
        self.next_z += 1;
        self.images.push(ImageDraw {
            rect,
            image,
            alpha: alpha.clamp(0.0, 1.0),
            color_filter,
            z_index,
            clip,
            src_rect,
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
        self.images.clear();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

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
