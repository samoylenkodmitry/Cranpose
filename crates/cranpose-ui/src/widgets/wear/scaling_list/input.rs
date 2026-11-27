//! The touch and rotary input of a scaling list: a drag scrolls it, a
//! release flings it.

use std::rc::Rc;

use cranpose_foundation::{DRAG_THRESHOLD, MAX_FLING_VELOCITY, PointerId};
use cranpose_ui_graphics::Point;

use super::WearScalingListState;
use crate::{
    fling_animation::FlingAnimation,
    gesture_velocity::GestureVelocity,
    modifier::{Modifier, PointerEvent, PointerEventKind, PointerInputScope},
};

pub(super) fn wear_scaling_list_input(
    modifier: Modifier,
    state: WearScalingListState,
    fling: Rc<FlingAnimation>,
) -> Modifier {
    let rotary_state = state;
    modifier
        .on_rotary_scroll_event(move |event| {
            let delta = if event.vertical_scroll_pixels != 0.0 {
                event.vertical_scroll_pixels
            } else {
                event.horizontal_scroll_pixels
            };
            rotary_state.dispatch_raw_delta(delta) != 0.0
        })
        .pointer_input(state.id(), move |scope: PointerInputScope| {
            let fling = Rc::clone(&fling);
            async move {
                scope
                    .await_pointer_event_scope(|events| async move {
                        let mut swipe = Swipe::new(state, fling);
                        loop {
                            let event = events.await_pointer_event().await;
                            swipe.handle(&event);
                        }
                    })
                    .await;
            }
        })
}

// A finger on the list: it drags the list along its axis and flings it on
// release.
struct Swipe {
    state: WearScalingListState,
    fling: Rc<FlingAnimation>,
    pointer: Option<PointerId>,
    down: Point,
    last: Point,
    dragging: bool,
    velocity: GestureVelocity,
}

impl Swipe {
    fn new(state: WearScalingListState, fling: Rc<FlingAnimation>) -> Self {
        Self {
            state,
            fling,
            pointer: None,
            down: Point::new(0.0, 0.0),
            last: Point::new(0.0, 0.0),
            dragging: false,
            velocity: GestureVelocity::new(),
        }
    }

    fn handle(&mut self, event: &PointerEvent) {
        match event.kind {
            PointerEventKind::Down if self.pointer.is_none() => self.on_down(event),
            PointerEventKind::Move if self.pointer == Some(event.id) => self.on_move(event),
            PointerEventKind::Up if self.pointer == Some(event.id) => self.on_up(event),
            PointerEventKind::Cancel if self.pointer == Some(event.id) => self.let_go(),
            PointerEventKind::Scroll => {
                let consumed = self.state.dispatch_raw_delta(event.scroll_delta.y);
                if consumed != 0.0 {
                    event.consume();
                }
            }
            _ => {}
        }
    }

    fn on_down(&mut self, event: &PointerEvent) {
        self.state.stop_fling();
        self.pointer = Some(event.id);
        self.down = event.position;
        self.last = event.position;
        self.dragging = false;
        self.velocity.begin(event, event.position.y);
    }

    fn on_move(&mut self, event: &PointerEvent) {
        if event.is_consumed() {
            self.let_go();
            return;
        }
        let dx = event.position.x - self.down.x;
        let dy = event.position.y - self.down.y;
        if !self.dragging && dy.abs() > DRAG_THRESHOLD && dy.abs() > dx.abs() {
            self.dragging = true;
        } else if !self.dragging && dx.abs() > DRAG_THRESHOLD && dx.abs() >= dy.abs() {
            self.let_go();
            return;
        }
        if self.dragging {
            let consumed = self
                .state
                .dispatch_raw_delta(self.last.y - event.position.y);
            self.last = event.position;
            self.velocity.add(event, event.position.y);
            if consumed != 0.0 {
                event.consume();
            }
        }
    }

    fn on_up(&mut self, event: &PointerEvent) {
        if self.dragging {
            let speed = -self.velocity.release(event, MAX_FLING_VELOCITY);
            let state = self.state;
            let stop = Rc::clone(&self.fling);
            let id = state.hold_fling(move || stop.cancel());
            self.fling.start_fling(
                0.0,
                speed,
                move |delta| state.dispatch_raw_delta(delta),
                move || {
                    if let Some(id) = id {
                        state.end_fling(id);
                    }
                },
            );
            event.consume();
        }
        self.let_go();
    }

    // The finger is gone, or it went to another gesture.
    fn let_go(&mut self) {
        self.pointer = None;
        self.dragging = false;
        self.velocity.clear();
    }
}
