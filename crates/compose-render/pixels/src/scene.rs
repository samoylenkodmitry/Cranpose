use std::cell::RefCell;
use std::rc::Rc;

use compose_core::run_in_mutable_snapshot;
use compose_foundation::{PointerEvent, PointerEventKind, PointerPhase};
use compose_render_common::{HitTestTarget, RenderScene};
use compose_ui_graphics::{Brush, Color, Rect, RoundedCornerShape};

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
    pub rect: Rect,
    pub text: String,
    pub color: Color,
    pub scale: f32,
    pub z_index: usize,
    pub clip: Option<Rect>,
}

#[derive(Clone)]
pub enum ClickAction {
    Simple(Rc<RefCell<dyn FnMut()>>),
    WithPoint(Rc<dyn Fn(compose_ui_graphics::Point)>),
}

impl ClickAction {
    fn invoke(&self, rect: Rect, x: f32, y: f32) {
        match self {
            ClickAction::Simple(handler) => (handler.borrow_mut())(),
            ClickAction::WithPoint(handler) => handler(compose_ui_graphics::Point {
                x: x - rect.x,
                y: y - rect.y,
            }),
        }
    }
}

#[derive(Clone)]
pub struct HitRegion {
    pub rect: Rect,
    pub shape: Option<RoundedCornerShape>,
    pub click_actions: Vec<ClickAction>,
    pub pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    pub z_index: usize,
    pub hit_clip: Option<Rect>,
}

impl HitTestTarget for HitRegion {
    fn dispatch(&self, kind: PointerEventKind, x: f32, y: f32) {
        let local = compose_ui_graphics::Point {
            x: x - self.rect.x,
            y: y - self.rect.y,
        };
        let global = compose_ui_graphics::Point { x, y };
        let event = PointerEvent {
            id: 0,
            kind,
            phase: match kind {
                PointerEventKind::Down => PointerPhase::Start,
                PointerEventKind::Move => PointerPhase::Move,
                PointerEventKind::Up => PointerPhase::End,
                PointerEventKind::Cancel => PointerPhase::Cancel,
            },
            position: local,
            global_position: global,
            buttons: Default::default(),
        };
        let has_pointer_inputs = !self.pointer_inputs.is_empty();
        let has_click_actions = kind == PointerEventKind::Down && !self.click_actions.is_empty();

        if !has_pointer_inputs && !has_click_actions {
            return;
        }

        eprintln!(
            "DEBUG: Dispatching {:?} event at ({:.1}, {:.1}) to region with {} pointer_inputs, {} click_actions",
            kind, x, y, self.pointer_inputs.len(), self.click_actions.len()
        );

        if let Err(err) = run_in_mutable_snapshot(|| {
            for handler in &self.pointer_inputs {
                handler(event);
            }
            if kind == PointerEventKind::Down {
                for action in &self.click_actions {
                    action.invoke(self.rect, x, y);
                }
            }
        }) {
            eprintln!(
                "failed to apply mutable snapshot for pointer event {:?} at ({}, {}): {}",
                kind,
                x,
                y,
                err
            );
        }
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
    next_z: usize,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            shapes: Vec::new(),
            texts: Vec::new(),
            hits: Vec::new(),
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
        rect: Rect,
        text: String,
        color: Color,
        scale: f32,
        clip: Option<Rect>,
    ) {
        let z_index = self.next_z;
        self.next_z += 1;
        self.texts.push(TextDraw {
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
        rect: Rect,
        shape: Option<RoundedCornerShape>,
        click_actions: Vec<ClickAction>,
        pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
        hit_clip: Option<Rect>,
    ) {
        if click_actions.is_empty() && pointer_inputs.is_empty() {
            return;
        }
        eprintln!(
            "DEBUG: Creating HitRegion at ({:.1}, {:.1}) {}x{} with {} clicks, {} pointer_inputs",
            rect.x, rect.y, rect.width, rect.height,
            click_actions.len(), pointer_inputs.len()
        );
        let z_index = self.next_z;
        self.next_z += 1;
        self.hits.push(HitRegion {
            rect,
            shape,
            click_actions,
            pointer_inputs,
            z_index,
            hit_clip,
        });
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
        self.next_z = 0;
    }

    fn hit_test(&self, x: f32, y: f32) -> Option<Self::HitTarget> {
        self.hits
            .iter()
            .filter(|hit| hit.contains(x, y))
            .max_by(|a, b| a.z_index.cmp(&b.z_index))
            .cloned()
    }
}
