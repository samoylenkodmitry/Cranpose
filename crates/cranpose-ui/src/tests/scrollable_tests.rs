use std::{cell::Cell, rc::Rc};

use cranpose_foundation::PointerEventKind;
use cranpose_ui_layout::Axis;

use super::draggable_tests::{event, pointer_handler_for, with_test_runtime};
use crate::{Modifier, ScrollableState};

#[test]
fn custom_scroll_fling_follows_the_drag_and_stops_at_its_boundary_or_a_new_press() {
    let _app_context = crate::render_state::app_context_test_scope();
    for axis in [Axis::Horizontal, Axis::Vertical] {
        for interrupt in [false, true] {
            with_test_runtime(|| {
                let position = Rc::new(Cell::new(0.0_f32));
                let consumed = Rc::clone(&position);
                let state = ScrollableState::new(move |delta| {
                    let before = consumed.get();
                    let after = (before + delta).clamp(0.0, 240.0);
                    consumed.set(after);
                    after - before
                });
                let (handler, _chain) =
                    pointer_handler_for(Modifier::empty().scrollable(axis, state));
                let pointer = |kind, distance, time| {
                    let (x, y) = if axis.is_vertical() {
                        (0.0, distance)
                    } else {
                        (distance, 0.0)
                    };
                    handler(event(kind, x, y).with_time_ms(Some(time)));
                };
                pointer(PointerEventKind::Down, 0.0, 0);
                for step in 1..=6 {
                    pointer(PointerEventKind::Move, step as f32 * 12.0, step * 16);
                }
                let held = position.get();
                pointer(PointerEventKind::Up, 72.0, 104);
                let runtime = cranpose_core::current_runtime_handle().expect("scroll runtime");
                for frame in 0..=5 {
                    runtime.drain_frame_callbacks(frame * 16_666_667);
                }
                assert!(
                    position.get() > held + 20.0,
                    "release continues in the drag direction"
                );
                let after_fling = position.get();
                if interrupt {
                    pointer(PointerEventKind::Down, 72.0, 200);
                }
                for frame in 6..=240 {
                    runtime.drain_frame_callbacks(frame * 16_666_667);
                }
                if interrupt {
                    assert_eq!(position.get(), after_fling, "a new press stops the coast");
                } else {
                    assert_eq!(
                        position.get(),
                        240.0,
                        "the boundary consumes only the remaining distance"
                    );
                }
            });
        }
    }
}
