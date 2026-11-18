//! The pointer input of a scroller's drag gesture. It feeds the gesture's
//! detector inside the nested scroll containers around the scroller. Its end
//! stops the gesture's motions.

use std::{cell::RefCell, rc::Rc};

use cranpose_ui_layout::Axis;

use super::{
    Modifier, NestedTarget, PointerEventKind, ScrollGestureDetector, ScrollGestureState,
    ScrollMotionContext, ScrollTarget,
};

pub(in crate::modifier) struct DragGesture<S: ScrollTarget> {
    pub(in crate::modifier) target: S,
    pub(in crate::modifier) axis: Axis,
    pub(in crate::modifier) reverse_input: bool,
    pub(in crate::modifier) motion_context: ScrollMotionContext,
    pub(in crate::modifier) guard: Option<Rc<dyn Fn() -> bool>>,
}

struct ScrollGestureLifetime<S: ScrollTarget> {
    gesture_state: Rc<RefCell<ScrollGestureState>>,
    target: S,
    motion_context: ScrollMotionContext,
}

impl<S: ScrollTarget> Drop for ScrollGestureLifetime<S> {
    fn drop(&mut self) {
        let state = std::mem::replace(
            &mut *self.gesture_state.borrow_mut(),
            ScrollGestureState {
                disposed: true,
                ..ScrollGestureState::default()
            },
        );
        let was_active = state.is_active();
        if let Some(fling) = state.fling_animation {
            fling.cancel();
        }
        if let Some(settle) = state.settle_animation {
            settle.cancel();
        }
        if let Some(watcher) = state.wheel_settle_watcher {
            watcher.cancel();
        }
        if let Some(id) = state.running_scroll {
            self.target.end_scroll(id);
        }
        if was_active {
            self.motion_context.set_active(false);
        }
    }
}

/// The drag gesture of a scroller, inside the [`crate::NestedScroll`]
/// containers of the composition it is built in.
pub(super) fn nested_drag_gesture_input<K, S>(key: K, gesture: DragGesture<S>) -> Modifier
where
    K: std::hash::Hash + 'static,
    S: ScrollTarget + 'static,
{
    let Some(parent) = crate::nested_scroll::current_nested_scroll_parent() else {
        return drag_gesture_input(key, gesture);
    };
    let DragGesture {
        target,
        axis,
        reverse_input,
        motion_context,
        guard,
    } = gesture;
    let identity = parent.identity();
    drag_gesture_input(
        (key, identity),
        DragGesture {
            target: NestedTarget {
                target,
                parent,
                axis,
            },
            axis,
            reverse_input,
            motion_context,
            guard,
        },
    )
}

pub(in crate::modifier) fn drag_gesture_input<K, S>(key: K, gesture: DragGesture<S>) -> Modifier
where
    K: std::hash::Hash + 'static,
    S: ScrollTarget + 'static,
{
    let DragGesture {
        target,
        axis,
        reverse_input,
        motion_context,
        guard,
    } = gesture;

    Modifier::empty().pointer_input(key, move |scope| {
        let gesture_state = Rc::new(RefCell::new(ScrollGestureState::default()));
        let detector = ScrollGestureDetector::new(
            Rc::clone(&gesture_state),
            target.clone(),
            axis,
            reverse_input,
            motion_context.overscroll(),
            motion_context.clone(),
        );
        let guard = guard.clone();
        let lifetime = ScrollGestureLifetime {
            gesture_state: Rc::clone(&gesture_state),
            target: target.clone(),
            motion_context: motion_context.clone(),
        };

        async move {
            let _lifetime = lifetime;
            scope
                .await_pointer_event_scope(|await_scope| async move {
                    loop {
                        let event = await_scope.await_pointer_event().await;

                        if !event.id.is_primary() {
                            continue;
                        }

                        if event.is_consumed() {
                            if matches!(
                                event.kind,
                                PointerEventKind::Down
                                    | PointerEventKind::Move
                                    | PointerEventKind::Up
                                    | PointerEventKind::Cancel
                            ) {
                                detector.on_cancel();
                            }
                            continue;
                        }

                        if let Some(ref guard) = guard
                            && !guard()
                        {
                            if matches!(event.kind, PointerEventKind::Up | PointerEventKind::Cancel)
                            {
                                detector.on_cancel();
                            }
                            continue;
                        }

                        let should_consume = match event.kind {
                            PointerEventKind::Down => detector.on_down(&event),
                            PointerEventKind::Move => {
                                detector.on_move(event.position, event.buttons, &event)
                            }
                            PointerEventKind::Up => detector.on_up(&event),
                            PointerEventKind::Cancel => detector.on_cancel(),
                            PointerEventKind::Scroll => detector.on_scroll(
                                if axis.is_vertical() {
                                    event.scroll_delta.y
                                } else {
                                    event.scroll_delta.x
                                },
                                &event,
                            ),
                            PointerEventKind::Zoom
                            | PointerEventKind::RotaryScrollPre
                            | PointerEventKind::RotaryScroll
                            | PointerEventKind::Enter
                            | PointerEventKind::Exit
                            | PointerEventKind::ButtonDown(_)
                            | PointerEventKind::ButtonUp(_) => false,
                        };

                        if should_consume {
                            event.consume();
                        }
                    }
                })
                .await;
        }
    })
}
