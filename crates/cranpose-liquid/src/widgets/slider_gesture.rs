use std::{cell::Cell, rc::Rc};

use cranpose_foundation::{PointerEvent, PointerEventKind};
use cranpose_ui::PointerInputScope;

use super::{
    control_motion::ControlContactMotion,
    slider_motion::{SliderDrag, SliderFling},
};
use crate::motion::{LiquidDragAxis, LiquidMotion};

#[derive(Clone)]
pub(super) struct SliderGesture {
    pub(super) active: Rc<Cell<Option<SliderDrag>>>,
    pub(super) axis: Rc<LiquidDragAxis>,
    pub(super) contact: Rc<ControlContactMotion>,
    pub(super) fling: Rc<SliderFling>,
    pub(super) on_change: Rc<dyn Fn(f32)>,
    pub(super) value: f32,
    pub(super) width: f32,
    pub(super) thumb_width: f32,
}

impl SliderGesture {
    pub(super) async fn run(self, scope: PointerInputScope) {
        scope
            .await_pointer_event_scope(|events| async move {
                loop {
                    self.handle(events.await_pointer_event().await);
                }
            })
            .await;
    }

    fn handle(&self, event: PointerEvent) {
        let usable = (self.width - self.thumb_width).max(1.0);
        let input_time = event.time_ms.or_else(|| {
            event
                .animation_time_nanos
                .map(|time| (time / 1_000_000) as i64)
        });
        if event.kind == PointerEventKind::Down && self.active.get().is_none() {
            let center = usable * self.value + self.thumb_width * 0.5;
            if (event.position.x - center).abs() <= self.thumb_width * 0.5 {
                self.fling.cancel();
                self.active.set(Some(SliderDrag::new(
                    event.id,
                    event.position.x,
                    self.value,
                    input_time,
                )));
                self.axis.begin(usable * self.value, event.time_ms);
                self.contact.pressed(true, event.animation_time_nanos);
                event.consume();
            }
            return;
        }
        let Some(mut drag) = self.active.get().filter(|drag| drag.id == event.id) else {
            return;
        };
        match event.kind {
            PointerEventKind::Move | PointerEventKind::Up => {
                let previous = drag.value;
                if event.kind == PointerEventKind::Move {
                    drag.moved(event.position.x, input_time);
                }
                let fraction = drag.fraction(event.position.x, self.width);
                if event.kind == PointerEventKind::Move {
                    self.active.set(Some(drag));
                    self.axis.move_to(usable * fraction, event.time_ms);
                } else {
                    self.fling.release(
                        fraction,
                        drag.release_velocity(),
                        self.width,
                        event.animation_time_nanos,
                    );
                    self.axis.finish_at(usable * fraction, event.time_ms);
                    self.finish(&event);
                }
                if fraction != previous {
                    (self.on_change)(fraction);
                }
                event.consume();
            }
            PointerEventKind::Cancel => {
                self.fling.cancel();
                self.axis
                    .release_to(usable * self.value, event.time_ms, LiquidMotion::snappy());
                self.finish(&event);
                event.consume();
            }
            _ => {}
        }
    }

    fn finish(&self, event: &PointerEvent) {
        self.active.set(None);
        self.contact.pressed(false, event.animation_time_nanos);
    }
}
