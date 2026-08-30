use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{NodeId, current_runtime_handle, internal::FrameCallbackRegistration};
use cranpose_foundation::{
    DRAG_THRESHOLD, DelegatableNode, MAX_FLING_VELOCITY, ModifierNode, ModifierNodeElement,
    NodeCapabilities, NodeState, PointerButton, PointerButtons, VelocityTracker1D,
    velocity_tracker::ASSUME_STOPPED_MS,
};
use cranpose_ui_layout::Axis;
use web_time::Instant;

use super::{Modifier, Point, PointerEvent, PointerEventKind, inspector_metadata};
use crate::{
    draggable::DraggableState,
    fling_animation::{
        FlingAnimation, MIN_FLING_VELOCITY, SettleAnimation, SpringParams, fling_rest_position,
    },
    render_state::schedule_modifier_slices_repass,
    scroll::{
        ScrollElement, ScrollMotionContext, ScrollMotionContextKey, ScrollSettlePolicy,
        ScrollState, scroll_motion_context_for_key,
    },
};

#[cfg(feature = "test-helpers")]
pub fn last_fling_velocity() -> f32 {
    crate::render_state::debug_last_fling_velocity()
}

#[cfg(feature = "test-helpers")]
pub fn reset_last_fling_velocity() {
    crate::render_state::debug_reset_last_fling_velocity();
}

#[inline]
fn set_last_fling_velocity(velocity: f32) {
    crate::render_state::record_last_fling_velocity(velocity);
}

struct ScrollGestureState {
    drag_down_position: Option<Point>,

    last_position: Option<Point>,

    is_dragging: bool,

    axis_locked_out: bool,

    velocity_tracker: VelocityTracker1D,

    gesture_start_time: Option<Instant>,

    gesture_start_event_time_ms: Option<i64>,

    last_velocity_sample_ms: Option<i64>,

    fling_animation: Option<FlingAnimation>,

    is_overscrolling: bool,

    settle_animation: Option<SettleAnimation>,

    wheel_settle_watcher: Option<WheelSettleWatcher>,
}

impl Default for ScrollGestureState {
    fn default() -> Self {
        Self {
            drag_down_position: None,
            last_position: None,
            is_dragging: false,
            axis_locked_out: false,
            velocity_tracker: VelocityTracker1D::new(),
            gesture_start_time: None,
            gesture_start_event_time_ms: None,
            last_velocity_sample_ms: None,
            fling_animation: None,
            is_overscrolling: false,
            settle_animation: None,
            wheel_settle_watcher: None,
        }
    }
}

#[inline]
fn calculate_total_delta(from: Point, to: Point, is_vertical: bool) -> f32 {
    if is_vertical {
        to.y - from.y
    } else {
        to.x - from.x
    }
}

#[inline]
fn calculate_incremental_delta(from: Point, to: Point, is_vertical: bool) -> f32 {
    if is_vertical {
        to.y - from.y
    } else {
        to.x - from.x
    }
}

trait ScrollTarget: Clone {
    fn apply_delta(&self, delta: f32) -> f32;

    fn apply_wheel_delta(&self, delta: f32) -> f32 {
        self.apply_delta(delta)
    }

    fn apply_fling_delta(&self, delta: f32) -> f32;

    fn invalidate(&self);

    fn current_offset(&self) -> f32;

    fn can_scroll(&self) -> bool {
        true
    }

    fn can_consume(&self, gesture_delta: f32) -> bool {
        let _ = gesture_delta;
        self.can_scroll()
    }

    fn settle_policy(&self) -> Option<ScrollSettlePolicy> {
        None
    }

    fn allows_fling(&self) -> bool {
        true
    }

    fn set_dragging(&self, dragging: bool) {
        let _ = dragging;
    }
}

impl ScrollTarget for ScrollState {
    fn apply_delta(&self, delta: f32) -> f32 {
        -self.dispatch_raw_delta(-delta)
    }

    fn apply_fling_delta(&self, delta: f32) -> f32 {
        self.dispatch_raw_delta(delta)
    }

    fn invalidate(&self) {}

    fn current_offset(&self) -> f32 {
        self.value()
    }

    fn can_scroll(&self) -> bool {
        self.max_value() > 0.0
    }

    fn can_consume(&self, gesture_delta: f32) -> bool {
        let raw = -gesture_delta;
        if raw > 0.0 {
            self.value_non_reactive() < self.max_value()
        } else {
            self.value_non_reactive() > 0.0
        }
    }

    fn settle_policy(&self) -> Option<ScrollSettlePolicy> {
        ScrollState::settle_policy(self)
    }
}

impl ScrollTarget for LazyListState {
    fn apply_delta(&self, delta: f32) -> f32 {
        self.dispatch_scroll_delta(delta)
    }

    fn apply_wheel_delta(&self, delta: f32) -> f32 {
        if delta.abs() <= 0.001 {
            0.0
        } else {
            self.dispatch_scroll_delta(delta)
        }
    }

    fn apply_fling_delta(&self, delta: f32) -> f32 {
        -self.dispatch_scroll_delta(-delta)
    }

    fn invalidate(&self) {}

    fn current_offset(&self) -> f32 {
        self.first_visible_item_scroll_offset()
    }

    fn can_scroll(&self) -> bool {
        self.layout_info().total_items_count == 0
            || self.can_scroll_forward_non_reactive()
            || self.can_scroll_backward_non_reactive()
    }

    fn can_consume(&self, gesture_delta: f32) -> bool {
        if self.layout_info().total_items_count == 0 {
            return true;
        }
        if gesture_delta < 0.0 {
            self.can_scroll_forward_non_reactive()
        } else {
            self.can_scroll_backward_non_reactive()
        }
    }
}

struct DragGesture<S: ScrollTarget> {
    target: S,
    gesture_state: Rc<RefCell<ScrollGestureState>>,
    is_vertical: bool,
    reverse_input: bool,
    motion_context: ScrollMotionContext,
    guard: Option<Rc<dyn Fn() -> bool>>,
}

fn drag_gesture_input<K, S>(key: K, gesture: DragGesture<S>) -> Modifier
where
    K: std::hash::Hash + 'static,
    S: ScrollTarget + 'static,
{
    let DragGesture {
        target,
        gesture_state,
        is_vertical,
        reverse_input,
        motion_context,
        guard,
    } = gesture;

    Modifier::empty().pointer_input(key, move |scope| {
        let detector = ScrollGestureDetector::new(
            gesture_state.clone(),
            target.clone(),
            is_vertical,
            reverse_input,
            motion_context.overscroll(),
            motion_context.clone(),
        );
        let guard = guard.clone();

        async move {
            scope
                .await_pointer_event_scope(|await_scope| async move {
                    loop {
                        let event = await_scope.await_pointer_event().await;

                        if event.id != 0 {
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
                            PointerEventKind::Down => {
                                detector.on_down(event.position, event.time_ms)
                            }
                            PointerEventKind::Move => detector.on_move(
                                event.position,
                                event.buttons,
                                event.time_ms,
                                &event,
                            ),
                            PointerEventKind::Up => detector.on_up(event.time_ms),
                            PointerEventKind::Cancel => detector.on_cancel(),
                            PointerEventKind::Scroll => detector.on_scroll(
                                if is_vertical {
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
                            | PointerEventKind::Exit => false,
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

impl ScrollTarget for DraggableState {
    fn apply_delta(&self, delta: f32) -> f32 {
        self.drag_by(delta);
        delta
    }

    fn apply_fling_delta(&self, delta: f32) -> f32 {
        self.drag_by(delta);
        delta
    }

    fn invalidate(&self) {}

    fn current_offset(&self) -> f32 {
        self.offset()
    }

    fn allows_fling(&self) -> bool {
        false
    }

    fn set_dragging(&self, dragging: bool) {
        DraggableState::set_dragging(self, dragging);
    }
}

const WHEEL_SETTLE_IDLE_NANOS: u64 = 180_000_000;

struct WheelSettleWatcher {
    is_running: Rc<Cell<bool>>,
    registration: Rc<RefCell<Option<FrameCallbackRegistration>>>,
}

impl WheelSettleWatcher {
    fn cancel(&self) {
        self.is_running.set(false);
        self.registration.borrow_mut().take();
    }
}

struct ScrollGestureDetector<S: ScrollTarget> {
    gesture_state: Rc<RefCell<ScrollGestureState>>,

    scroll_target: S,

    is_vertical: bool,

    reverse_scrolling: bool,

    overscroll: crate::scroll::OverscrollEffect,

    motion_context: ScrollMotionContext,
}

impl<S: ScrollTarget + 'static> ScrollGestureDetector<S> {
    fn new(
        gesture_state: Rc<RefCell<ScrollGestureState>>,
        scroll_target: S,
        is_vertical: bool,
        reverse_scrolling: bool,
        overscroll: crate::scroll::OverscrollEffect,
        motion_context: ScrollMotionContext,
    ) -> Self {
        Self {
            gesture_state,
            scroll_target,
            is_vertical,
            reverse_scrolling,
            overscroll,
            motion_context,
        }
    }

    fn on_down(&self, position: Point, time_ms: Option<i64>) -> bool {
        let mut gs = self.gesture_state.borrow_mut();

        if let Some(fling) = gs.fling_animation.take() {
            fling.cancel();
        }
        if let Some(settle) = gs.settle_animation.take() {
            settle.cancel();
        }
        if let Some(watcher) = gs.wheel_settle_watcher.take() {
            watcher.cancel();
        }
        self.motion_context.set_active(false);

        gs.drag_down_position = Some(position);
        gs.last_position = Some(position);
        gs.is_dragging = false;
        gs.axis_locked_out = false;
        gs.velocity_tracker.reset();
        gs.gesture_start_time = Some(Instant::now());
        gs.gesture_start_event_time_ms = time_ms;
        gs.is_overscrolling = self.overscroll.offset().abs() > 0.001;

        let pos = if self.is_vertical {
            position.y
        } else {
            position.x
        };
        gs.velocity_tracker.add_data_point(0, pos);
        gs.last_velocity_sample_ms = Some(0);

        false
    }

    fn on_move(
        &self,
        position: Point,
        buttons: PointerButtons,
        time_ms: Option<i64>,
        event: &PointerEvent,
    ) -> bool {
        let mut gs = self.gesture_state.borrow_mut();

        if !buttons.contains(PointerButton::Primary) && gs.drag_down_position.is_some() {
            if gs.is_dragging {
                self.scroll_target.set_dragging(false);
            }
            gs.drag_down_position = None;
            gs.last_position = None;
            gs.is_dragging = false;
            gs.axis_locked_out = false;
            gs.gesture_start_time = None;
            gs.gesture_start_event_time_ms = None;
            gs.last_velocity_sample_ms = None;
            gs.velocity_tracker.reset();
            self.motion_context.set_active(false);
            return false;
        }

        let Some(down_pos) = gs.drag_down_position else {
            return false;
        };

        let Some(last_pos) = gs.last_position else {
            gs.last_position = Some(position);
            return false;
        };

        let incremental_delta = calculate_incremental_delta(last_pos, position, self.is_vertical);

        let mut edge_candidate = false;
        if !gs.is_dragging && !gs.axis_locked_out {
            let signed_main_delta = calculate_total_delta(down_pos, position, self.is_vertical);
            let main_delta = signed_main_delta.abs();
            let cross_delta = calculate_total_delta(down_pos, position, !self.is_vertical).abs();
            if main_delta > DRAG_THRESHOLD && main_delta >= cross_delta {
                if self.scroll_target.can_consume(signed_main_delta) {
                    gs.is_dragging = true;
                    self.scroll_target.set_dragging(true);
                    self.motion_context.set_active(true);
                } else if self.scroll_target.can_scroll() {
                    edge_candidate = true;
                }
            } else if cross_delta > DRAG_THRESHOLD && cross_delta > main_delta {
                gs.axis_locked_out = true;
            }
        }

        gs.last_position = Some(position);

        let pos = if self.is_vertical {
            position.y
        } else {
            position.x
        };
        let event_sample_ms = gs
            .gesture_start_event_time_ms
            .zip(time_ms)
            .map(|(start_ms, now_ms)| now_ms - start_ms);
        let sample_ms = if let Some(event_sample_ms) = event_sample_ms {
            Some(match gs.last_velocity_sample_ms {
                Some(last_sample_ms) => event_sample_ms.max(last_sample_ms),
                None => event_sample_ms.max(0),
            })
        } else if let Some(start_time) = gs.gesture_start_time {
            let elapsed_ms = start_time.elapsed().as_millis() as i64;
            Some(match gs.last_velocity_sample_ms {
                Some(last_sample_ms) => {
                    let mut sample_ms = if elapsed_ms <= last_sample_ms {
                        last_sample_ms + 1
                    } else {
                        elapsed_ms
                    };
                    if sample_ms - last_sample_ms > ASSUME_STOPPED_MS {
                        sample_ms = last_sample_ms + ASSUME_STOPPED_MS;
                    }
                    sample_ms
                }
                None => elapsed_ms,
            })
        } else {
            None
        };
        if let Some(sample_ms) = sample_ms {
            log::trace!(
                target: "cranpose::velocity",
                "sample t={sample_ms}ms pos={pos:.2} event_time={time_ms:?}"
            );
            gs.velocity_tracker.add_data_point(sample_ms, pos);
            gs.last_velocity_sample_ms = Some(sample_ms);
        }

        if gs.is_dragging {
            drop(gs);
            let delta = if self.reverse_scrolling {
                -incremental_delta
            } else {
                incremental_delta
            };
            let overscroll = self.overscroll.clone();
            overscroll.apply_to_scroll(delta, |delta| self.scroll_target.apply_delta(delta));
            self.scroll_target.invalidate();
            true
        } else if gs.is_overscrolling {
            drop(gs);
            let delta = if self.reverse_scrolling {
                -incremental_delta
            } else {
                incremental_delta
            };
            self.apply_overscroll_delta(delta)
        } else if edge_candidate {
            drop(gs);
            let delta = if self.reverse_scrolling {
                -incremental_delta
            } else {
                incremental_delta
            };
            let detector = self.clone_for_watcher();
            event.defer_post_dispatch_action(move || detector.apply_overscroll_candidate(delta));
            false
        } else {
            false
        }
    }

    fn apply_overscroll_delta(&self, delta: f32) -> bool {
        self.motion_context.set_active(true);
        let overscroll = self.overscroll.clone();
        let target_consumed = Cell::new(0.0);
        let before_overscroll = overscroll.offset();
        overscroll.apply_to_scroll(delta, |delta| {
            let consumed = self.scroll_target.apply_delta(delta);
            target_consumed.set(consumed);
            consumed
        });
        self.scroll_target.invalidate();
        let consumed = target_consumed.get().abs() > 0.001
            || (overscroll.offset() - before_overscroll).abs() > 0.001;
        if !consumed {
            self.motion_context.set_active(false);
        }
        consumed
    }

    fn apply_overscroll_candidate(&self, delta: f32) -> bool {
        let consumed = self.apply_overscroll_delta(delta);
        if consumed {
            self.gesture_state.borrow_mut().is_overscrolling = true;
        }
        consumed
    }

    fn finish_gesture(&self, allow_fling: bool, release_time_ms: Option<i64>) -> bool {
        let (was_dragging, gesture_owned, velocity, start_fling, existing_fling) = {
            let mut gs = self.gesture_state.borrow_mut();
            let was_dragging = gs.is_dragging;
            let gesture_owned = was_dragging || gs.is_overscrolling;
            let mut velocity = 0.0;

            if allow_fling && gesture_owned && gs.gesture_start_time.is_some() {
                let release_sample_ms = release_time_ms
                    .zip(gs.gesture_start_event_time_ms)
                    .map(|(release_ms, start_ms)| release_ms - start_ms)
                    .or_else(|| {
                        gs.gesture_start_time
                            .map(|start| start.elapsed().as_millis() as i64)
                    });
                let rested_before_release = release_sample_ms
                    .zip(gs.last_velocity_sample_ms)
                    .is_some_and(|(release_ms, last_sample_ms)| {
                        release_ms - last_sample_ms > ASSUME_STOPPED_MS
                    });
                if !rested_before_release {
                    velocity = gs
                        .velocity_tracker
                        .calculate_velocity_with_max(MAX_FLING_VELOCITY);
                }
            }

            let start_fling = allow_fling && was_dragging && velocity.abs() > MIN_FLING_VELOCITY;
            let existing_fling = if start_fling {
                gs.fling_animation.take()
            } else {
                None
            };

            if was_dragging {
                self.scroll_target.set_dragging(false);
            }
            gs.drag_down_position = None;
            gs.last_position = None;
            gs.is_dragging = false;
            gs.is_overscrolling = false;
            gs.axis_locked_out = false;
            gs.gesture_start_time = None;
            gs.gesture_start_event_time_ms = None;
            gs.last_velocity_sample_ms = None;

            (
                was_dragging,
                gesture_owned,
                velocity,
                start_fling,
                existing_fling,
            )
        };

        if allow_fling && gesture_owned {
            log::debug!(
                target: "cranpose::velocity",
                "gesture finished: fling velocity={velocity:.2} dp/s start_fling={start_fling}"
            );
            set_last_fling_velocity(velocity);
        }

        let adjusted_velocity = if self.reverse_scrolling {
            -velocity
        } else {
            velocity
        };
        let fling_velocity = -adjusted_velocity;
        let has_overscroll = self.overscroll.offset().abs() > 0.001;

        let settle_target = if was_dragging {
            self.scroll_target.settle_policy().and_then(|policy| {
                let current = self.scroll_target.current_offset();
                let proposed = if start_fling {
                    fling_rest_position(current, fling_velocity)
                } else {
                    current
                };
                let target = policy(proposed, fling_velocity);
                ((target - proposed).abs() > 0.5).then_some(target)
            })
        } else {
            None
        };

        if has_overscroll {
            if let Some(old_fling) = existing_fling {
                old_fling.cancel();
            }
            self.start_overscroll_settle(-fling_velocity);
        } else if let Some(target) = settle_target {
            if let Some(old_fling) = existing_fling {
                old_fling.cancel();
            }
            self.start_settle_animation(target, fling_velocity);
        } else if start_fling {
            if let Some(old_fling) = existing_fling {
                old_fling.cancel();
            }
            self.start_fling_animation(fling_velocity);
        } else {
            self.motion_context.set_active(false);
        }

        gesture_owned
    }

    fn start_fling_animation(&self, fling_velocity: f32) {
        let Some(runtime) = current_runtime_handle() else {
            self.motion_context.set_active(false);
            return;
        };
        self.motion_context.set_active(true);
        let scroll_target = self.scroll_target.clone();
        let fling = FlingAnimation::new(runtime);
        let motion_context = self.motion_context.clone();
        let initial_value = scroll_target.current_offset();
        let scroll_target_for_fling = scroll_target.clone();
        let scroll_target_for_end = scroll_target.clone();
        let detector_for_end = self.clone_for_watcher();
        let overscroll_for_fling = self.overscroll.clone();

        fling.start_fling(
            initial_value,
            fling_velocity,
            move |delta| {
                let consumed = overscroll_for_fling.apply_to_fling(delta, |delta| {
                    scroll_target_for_fling.apply_fling_delta(delta)
                });
                scroll_target_for_fling.invalidate();
                consumed
            },
            move || {
                scroll_target_for_end.invalidate();
                let settle_running = detector_for_end
                    .gesture_state
                    .borrow()
                    .settle_animation
                    .as_ref()
                    .is_some_and(SettleAnimation::is_running);
                if detector_for_end.overscroll.offset().abs() > 0.001 && !settle_running {
                    detector_for_end.start_overscroll_settle(0.0);
                }
                detector_for_end.update_motion_active(&motion_context);
            },
        );

        let mut gs = self.gesture_state.borrow_mut();
        gs.fling_animation = Some(fling);
    }

    fn start_settle_animation(&self, target: f32, initial_velocity: f32) {
        let Some(runtime) = current_runtime_handle() else {
            self.motion_context.set_active(false);
            return;
        };
        self.motion_context.set_active(true);
        let settle = SettleAnimation::new(runtime, SpringParams::SETTLE_POLICY);
        let scroll_target_for_settle = self.scroll_target.clone();
        let scroll_target_for_end = self.scroll_target.clone();
        let detector_for_end = self.clone_for_watcher();
        let motion_context = self.motion_context.clone();
        settle.start_settle(
            self.scroll_target.current_offset(),
            initial_velocity,
            target,
            move |delta| {
                let consumed = scroll_target_for_settle.apply_fling_delta(delta);
                scroll_target_for_settle.invalidate();
                consumed
            },
            move |_| {
                scroll_target_for_end.invalidate();
                detector_for_end.update_motion_active(&motion_context);
            },
        );
        let mut gs = self.gesture_state.borrow_mut();
        gs.settle_animation = Some(settle);
    }

    fn start_overscroll_settle(&self, initial_velocity: f32) {
        let Some(runtime) = current_runtime_handle() else {
            let current = self.overscroll.offset();
            self.overscroll.apply_settle_delta(-current);
            self.motion_context.set_active(false);
            return;
        };
        let settle = SettleAnimation::new(runtime, SpringParams::OVERSCROLL_BOUNCE);
        let overscroll_for_settle = self.overscroll.clone();
        let overscroll_for_end = self.overscroll.clone();
        let detector_for_end = self.clone_for_watcher();
        let motion_context = self.motion_context.clone();
        let initial = self.overscroll.offset();
        settle.start_settle(
            initial,
            initial_velocity,
            0.0,
            move |delta| overscroll_for_settle.apply_settle_delta(delta),
            move |end| {
                let current = overscroll_for_end.offset();
                overscroll_for_end.apply_settle_delta(-current);
                if end.hit_boundary && end.velocity.abs() > MIN_FLING_VELOCITY {
                    detector_for_end.start_fling_animation(-end.velocity);
                } else {
                    detector_for_end.update_motion_active(&motion_context);
                }
            },
        );
        let mut gs = self.gesture_state.borrow_mut();
        gs.is_overscrolling = false;
        gs.settle_animation = Some(settle);
    }

    fn update_motion_active(&self, motion_context: &ScrollMotionContext) {
        let running = {
            let gs = self.gesture_state.borrow();
            gs.fling_animation
                .as_ref()
                .is_some_and(FlingAnimation::is_running)
                || gs
                    .settle_animation
                    .as_ref()
                    .is_some_and(SettleAnimation::is_running)
        };
        if !running {
            motion_context.set_active(false);
        }
    }

    fn on_up(&self, time_ms: Option<i64>) -> bool {
        self.finish_gesture(self.scroll_target.allows_fling(), time_ms)
    }

    fn on_cancel(&self) -> bool {
        self.finish_gesture(false, None)
    }

    fn on_scroll(&self, axis_delta: f32, event: &PointerEvent) -> bool {
        if axis_delta.abs() <= f32::EPSILON {
            return false;
        }

        let delta = if self.reverse_scrolling {
            -axis_delta
        } else {
            axis_delta
        };
        if !self.scroll_target.can_consume(delta) && self.scroll_target.can_scroll() {
            let detector = self.clone_for_watcher();
            event.defer_post_dispatch_action(move || detector.apply_wheel_delta(delta));
            return false;
        }
        self.apply_wheel_delta(delta)
    }

    fn apply_wheel_delta(&self, delta: f32) -> bool {
        {
            let mut gs = self.gesture_state.borrow_mut();
            if let Some(fling) = gs.fling_animation.take() {
                fling.cancel();
            }
            if let Some(settle) = gs.settle_animation.take() {
                settle.cancel();
            }
            gs.drag_down_position = None;
            gs.last_position = None;
            gs.is_dragging = false;
            gs.axis_locked_out = false;
            gs.gesture_start_time = None;
            gs.gesture_start_event_time_ms = None;
            gs.last_velocity_sample_ms = None;
            gs.velocity_tracker.reset();
        }

        self.motion_context.activate_for_current_frame();
        let overscroll = self.overscroll.clone();
        let consumed =
            overscroll.apply_to_scroll(delta, |delta| self.scroll_target.apply_wheel_delta(delta));
        let overscroll_active = self.overscroll.offset().abs() > 0.001;
        if consumed.abs() > 0.001 {
            self.scroll_target.invalidate();
        }
        if consumed.abs() > 0.001 || overscroll_active {
            self.ensure_wheel_settle_watcher();
            true
        } else {
            false
        }
    }

    fn ensure_wheel_settle_watcher(&self) {
        if self.scroll_target.settle_policy().is_none() && self.overscroll.offset().abs() <= 0.001 {
            return;
        }
        {
            let gs = self.gesture_state.borrow();
            if gs
                .wheel_settle_watcher
                .as_ref()
                .is_some_and(|watcher| watcher.is_running.get())
            {
                return;
            }
        }
        let Some(runtime) = current_runtime_handle() else {
            return;
        };

        let is_running = Rc::new(Cell::new(true));
        let registration = Rc::new(RefCell::new(None));

        struct WheelSettleLoop<S: ScrollTarget> {
            detector: ScrollGestureDetector<S>,
            gesture_state: Rc<RefCell<ScrollGestureState>>,
            frame_clock: cranpose_core::internal::FrameClock,
            is_running: Rc<Cell<bool>>,
            registration: Rc<RefCell<Option<FrameCallbackRegistration>>>,
            last_offset: Rc<Cell<f32>>,
            idle_nanos: Rc<Cell<u64>>,
            last_frame: Rc<Cell<Option<u64>>>,
        }

        impl<S: ScrollTarget + 'static> WheelSettleLoop<S> {
            fn next(&self) -> Self {
                Self {
                    detector: self.detector.clone_for_watcher(),
                    gesture_state: Rc::clone(&self.gesture_state),
                    frame_clock: self.frame_clock.clone(),
                    is_running: Rc::clone(&self.is_running),
                    registration: Rc::clone(&self.registration),
                    last_offset: Rc::clone(&self.last_offset),
                    idle_nanos: Rc::clone(&self.idle_nanos),
                    last_frame: Rc::clone(&self.last_frame),
                }
            }

            fn schedule(self) {
                let continuation = self.next();
                let registration_slot = Rc::clone(&self.registration);
                let new_registration = self.frame_clock.with_frame_nanos(move |frame_time_nanos| {
                    let this = &continuation;
                    if !this.is_running.get() {
                        return;
                    }
                    {
                        let gs = this.gesture_state.borrow();
                        let animating = gs.is_dragging
                            || gs
                                .fling_animation
                                .as_ref()
                                .is_some_and(FlingAnimation::is_running)
                            || gs
                                .settle_animation
                                .as_ref()
                                .is_some_and(SettleAnimation::is_running);
                        if animating {
                            this.is_running.set(false);
                            return;
                        }
                    }

                    let offset = this.detector.scroll_target.current_offset();
                    let dt = this
                        .last_frame
                        .get()
                        .map_or(0, |last| frame_time_nanos.saturating_sub(last));
                    this.last_frame.set(Some(frame_time_nanos));
                    if (offset - this.last_offset.get()).abs() > 0.01 {
                        this.last_offset.set(offset);
                        this.idle_nanos.set(0);
                    } else {
                        this.idle_nanos.set(this.idle_nanos.get() + dt);
                    }

                    if this.idle_nanos.get() >= WHEEL_SETTLE_IDLE_NANOS {
                        this.is_running.set(false);
                        if this.detector.overscroll.offset().abs() > 0.001 {
                            this.detector.start_overscroll_settle(0.0);
                            return;
                        }
                        if let Some(policy) = this.detector.scroll_target.settle_policy() {
                            let target = policy(offset, 0.0);
                            if (target - offset).abs() > 0.5 {
                                this.detector.start_settle_animation(target, 0.0);
                            }
                        }
                        return;
                    }

                    continuation.next().schedule();
                });
                *registration_slot.borrow_mut() = Some(new_registration);
            }
        }

        WheelSettleLoop {
            detector: self.clone_for_watcher(),
            gesture_state: Rc::clone(&self.gesture_state),
            frame_clock: runtime.frame_clock(),
            is_running: Rc::clone(&is_running),
            registration: Rc::clone(&registration),
            last_offset: Rc::new(Cell::new(self.scroll_target.current_offset())),
            idle_nanos: Rc::new(Cell::new(0u64)),
            last_frame: Rc::new(Cell::new(None::<u64>)),
        }
        .schedule();

        self.gesture_state.borrow_mut().wheel_settle_watcher = Some(WheelSettleWatcher {
            is_running,
            registration,
        });
    }

    fn clone_for_watcher(&self) -> ScrollGestureDetector<S> {
        ScrollGestureDetector {
            gesture_state: Rc::clone(&self.gesture_state),
            scroll_target: self.scroll_target.clone(),
            is_vertical: self.is_vertical,
            reverse_scrolling: self.reverse_scrolling,
            overscroll: self.overscroll.clone(),
            motion_context: self.motion_context.clone(),
        }
    }
}

pub(crate) struct MotionContextAnimatedNode {
    state: NodeState,
    motion_context: ScrollMotionContext,
    invalidation_callback_id: Option<u64>,
    overscroll_callback_id: Option<u64>,
    node_id: Option<NodeId>,
}

impl MotionContextAnimatedNode {
    fn new(motion_context: ScrollMotionContext) -> Self {
        Self {
            state: NodeState::new(),
            motion_context,
            invalidation_callback_id: None,
            overscroll_callback_id: None,
            node_id: None,
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        self.motion_context.is_active()
    }
}

pub(crate) struct TranslatedContentContextNode {
    state: NodeState,
    identity: usize,
    offset_source: TranslatedContentOffsetSource,
    overscroll: crate::scroll::OverscrollEffect,
    overscroll_callback_id: Option<u64>,
}

impl TranslatedContentContextNode {
    fn new(
        identity: usize,
        offset_source: TranslatedContentOffsetSource,
        overscroll: crate::scroll::OverscrollEffect,
    ) -> Self {
        Self {
            state: NodeState::new(),
            identity,
            offset_source,
            overscroll,
            overscroll_callback_id: None,
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        true
    }

    pub(crate) fn identity(&self) -> usize {
        self.identity
    }

    pub(crate) fn content_offset_reader(&self) -> Option<Rc<dyn Fn() -> Point>> {
        self.offset_source
            .content_offset_reader(self.overscroll.clone())
    }
}

impl DelegatableNode for TranslatedContentContextNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for TranslatedContentContextNode {
    fn on_attach(&mut self, context: &mut dyn cranpose_foundation::ModifierNodeContext) {
        if let Some(node_id) = context.node_id() {
            self.overscroll_callback_id =
                Some(self.overscroll.add_invalidate_callback(Box::new(move || {
                    schedule_modifier_slices_repass(node_id)
                })));
        }
    }

    fn on_detach(&mut self) {
        if let Some(id) = self.overscroll_callback_id.take() {
            self.overscroll.remove_invalidate_callback(id);
        }
    }
}

impl DelegatableNode for MotionContextAnimatedNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for MotionContextAnimatedNode {
    fn on_attach(&mut self, context: &mut dyn cranpose_foundation::ModifierNodeContext) {
        let node_id = context.node_id();
        self.node_id = node_id;
        if let Some(node_id) = node_id {
            let callback_id = self
                .motion_context
                .add_invalidate_callback(Box::new(move || {
                    schedule_modifier_slices_repass(node_id);
                }));
            self.invalidation_callback_id = Some(callback_id);
            let callback_id = self
                .motion_context
                .overscroll()
                .add_invalidate_callback(Box::new(move || {
                    crate::schedule_measure_repass(node_id);
                    schedule_modifier_slices_repass(node_id);
                }));
            self.overscroll_callback_id = Some(callback_id);
        }
    }

    fn on_detach(&mut self) {
        if let Some(id) = self.invalidation_callback_id.take() {
            self.motion_context.remove_invalidate_callback(id);
        }
        if let Some(id) = self.overscroll_callback_id.take() {
            self.motion_context
                .overscroll()
                .remove_invalidate_callback(id);
        }
        self.node_id = None;
    }
}

#[derive(Clone)]
struct MotionContextAnimatedElement {
    motion_context: ScrollMotionContext,
}

impl MotionContextAnimatedElement {
    fn new(motion_context: ScrollMotionContext) -> Self {
        Self { motion_context }
    }
}

impl std::fmt::Debug for MotionContextAnimatedElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MotionContextAnimatedElement").finish()
    }
}

impl PartialEq for MotionContextAnimatedElement {
    fn eq(&self, other: &Self) -> bool {
        self.motion_context.ptr_eq(&other.motion_context)
    }
}

impl Eq for MotionContextAnimatedElement {}

impl std::hash::Hash for MotionContextAnimatedElement {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.motion_context.stable_key().hash(state);
    }
}

impl ModifierNodeElement for MotionContextAnimatedElement {
    type Node = MotionContextAnimatedNode;

    fn create(&self) -> Self::Node {
        MotionContextAnimatedNode::new(self.motion_context.clone())
    }

    fn update(&self, node: &mut Self::Node) {
        if node.motion_context.ptr_eq(&self.motion_context) {
            return;
        }
        if let Some(id) = node.invalidation_callback_id.take() {
            node.motion_context.remove_invalidate_callback(id);
        }
        node.motion_context = self.motion_context.clone();
        if let Some(node_id) = node.node_id {
            let callback_id = node
                .motion_context
                .add_invalidate_callback(Box::new(move || {
                    schedule_modifier_slices_repass(node_id);
                }));
            node.invalidation_callback_id = Some(callback_id);
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

#[derive(Clone)]
enum TranslatedContentOffsetSource {
    LayoutContentOffset,
    LazyList {
        state: LazyListState,
        is_vertical: bool,
        reverse_scrolling: bool,
    },
}

impl TranslatedContentOffsetSource {
    fn content_offset_reader(
        &self,
        overscroll: crate::scroll::OverscrollEffect,
    ) -> Option<Rc<dyn Fn() -> Point>> {
        match self {
            Self::LayoutContentOffset => None,
            Self::LazyList {
                state, is_vertical, ..
            } => Some(Rc::new(lazy_list_content_offset_reader(
                *state,
                *is_vertical,
                overscroll,
            ))),
        }
    }

    fn is_vertical(&self) -> Option<bool> {
        match self {
            Self::LayoutContentOffset => None,
            Self::LazyList { is_vertical, .. } => Some(*is_vertical),
        }
    }

    fn reverse_scrolling(&self) -> Option<bool> {
        match self {
            Self::LayoutContentOffset => None,
            Self::LazyList {
                reverse_scrolling, ..
            } => Some(*reverse_scrolling),
        }
    }
}

fn lazy_list_content_offset_reader(
    state: LazyListState,
    is_vertical: bool,
    overscroll: crate::scroll::OverscrollEffect,
) -> impl Fn() -> Point {
    move || {
        let info = state.layout_info();
        if info.visible_items_info.is_empty() {
            return Point::default();
        };
        overscroll.set_dimension(info.viewport_size);
        let main_offset = info.snap_anchor_offset;
        let bounce = overscroll.offset();
        if is_vertical {
            Point::new(0.0, main_offset + bounce)
        } else {
            Point::new(main_offset + bounce, 0.0)
        }
    }
}

#[derive(Clone)]
struct TranslatedContentContextElement {
    identity: usize,
    offset_source: TranslatedContentOffsetSource,
    overscroll: crate::scroll::OverscrollEffect,
}

impl TranslatedContentContextElement {
    fn new(
        identity: usize,
        offset_source: TranslatedContentOffsetSource,
        overscroll: crate::scroll::OverscrollEffect,
    ) -> Self {
        Self {
            identity,
            offset_source,
            overscroll,
        }
    }
}

impl std::fmt::Debug for TranslatedContentContextElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let offset_source = match &self.offset_source {
            TranslatedContentOffsetSource::LayoutContentOffset => "layout",
            TranslatedContentOffsetSource::LazyList { .. } => "lazy_list",
        };
        f.debug_struct("TranslatedContentContextElement")
            .field("identity", &self.identity)
            .field("offset_source", &offset_source)
            .finish()
    }
}

impl PartialEq for TranslatedContentContextElement {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.overscroll.ptr_eq(&other.overscroll)
            && self.offset_source.is_vertical() == other.offset_source.is_vertical()
            && self.offset_source.reverse_scrolling() == other.offset_source.reverse_scrolling()
    }
}

impl Eq for TranslatedContentContextElement {}

impl std::hash::Hash for TranslatedContentContextElement {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.identity.hash(state);
        self.offset_source.is_vertical().hash(state);
        self.offset_source.reverse_scrolling().hash(state);
    }
}

impl ModifierNodeElement for TranslatedContentContextElement {
    type Node = TranslatedContentContextNode;

    fn create(&self) -> Self::Node {
        TranslatedContentContextNode::new(
            self.identity,
            self.offset_source.clone(),
            self.overscroll.clone(),
        )
    }

    fn update(&self, node: &mut Self::Node) {
        node.identity = self.identity;
        node.offset_source = self.offset_source.clone();
        node.overscroll = self.overscroll.clone();
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

impl Modifier {
    /// Creates a horizontally scrollable modifier.
    ///
    /// # Arguments
    /// * `state` - The ScrollState to control scroll position
    /// * `reverse_scrolling` - If true, reverses the scroll direction in layout.
    ///   Note: This affects how scroll offset is applied to content (via `ScrollNode`),
    ///   NOT the drag direction. Drag gestures always follow natural touch semantics:
    ///   drag right = scroll left (content moves right under finger).
    ///
    /// # Example
    /// ```text
    /// let scroll_state = ScrollState::new(0.0);
    /// Row(
    ///     Modifier::empty().horizontal_scroll(scroll_state, false),
    ///     // ... content
    /// );
    /// ```
    pub fn horizontal_scroll(self, state: ScrollState, reverse_scrolling: bool) -> Self {
        self.then(scroll_impl(state, false, reverse_scrolling, None))
    }

    /// Creates a vertically scrollable modifier.
    ///
    /// # Arguments
    /// * `state` - The ScrollState to control scroll position
    /// * `reverse_scrolling` - If true, reverses the scroll direction in layout.
    ///   Note: This affects how scroll offset is applied to content (via `ScrollNode`),
    ///   NOT the drag direction. Drag gestures always follow natural touch semantics:
    ///   drag down = scroll up (content moves down under finger).
    pub fn vertical_scroll(self, state: ScrollState, reverse_scrolling: bool) -> Self {
        self.then(scroll_impl(state, true, reverse_scrolling, None))
    }

    /// A horizontal scroll that never claims a pointer gesture.
    ///
    /// The position still moves through [`ScrollState`], but drags and wheel
    /// events pass straight through, so an owner can run its own
    /// `pointer_input` on the same row -- a drag-to-reorder list, say --
    /// without the two competing for the gesture.
    pub fn horizontal_scroll_without_gestures(
        self,
        state: ScrollState,
        reverse_scrolling: bool,
    ) -> Self {
        self.then(scroll_impl(
            state,
            false,
            reverse_scrolling,
            Some(Rc::new(|| false)),
        ))
    }

    /// A vertical scroll that never claims a pointer gesture.
    ///
    /// The vertical counterpart of [`Modifier::horizontal_scroll_without_gestures`].
    pub fn vertical_scroll_without_gestures(
        self,
        state: ScrollState,
        reverse_scrolling: bool,
    ) -> Self {
        self.then(scroll_impl(
            state,
            true,
            reverse_scrolling,
            Some(Rc::new(|| false)),
        ))
    }
}

fn scroll_impl(
    state: ScrollState,
    is_vertical: bool,
    reverse_scrolling: bool,
    guard: Option<Rc<dyn Fn() -> bool>>,
) -> Modifier {
    let gesture_state = Rc::new(RefCell::new(ScrollGestureState::default()));
    let motion_context = scroll_motion_context_for_key(ScrollMotionContextKey::ScrollState {
        state_id: state.id(),
        is_vertical,
        reverse_scrolling,
    });

    let pointer_input = drag_gesture_input(
        (state.id(), is_vertical),
        DragGesture {
            target: state,
            gesture_state,
            is_vertical,
            reverse_input: false,
            motion_context: motion_context.clone(),
            guard,
        },
    );

    let overscroll = motion_context.overscroll();
    let element = ScrollElement::new(state, overscroll.clone(), is_vertical, reverse_scrolling);
    let layout_modifier =
        Modifier::with_element(element).with_inspector_metadata(inspector_metadata(
            if is_vertical {
                "verticalScroll"
            } else {
                "horizontalScroll"
            },
            move |info| {
                info.add_property("isVertical", is_vertical.to_string());
                info.add_property("reverseScrolling", reverse_scrolling.to_string());
            },
        ));
    let motion_modifier =
        Modifier::with_element(MotionContextAnimatedElement::new(motion_context.clone()));
    let translated_content_modifier = Modifier::with_element(TranslatedContentContextElement::new(
        state.id() as usize,
        TranslatedContentOffsetSource::LayoutContentOffset,
        overscroll,
    ));

    pointer_input
        .then(motion_modifier)
        .then(translated_content_modifier)
        .then(layout_modifier)
        .clip_to_bounds()
}

use cranpose_foundation::lazy::LazyListState;

impl Modifier {
    /// Creates a vertically scrollable modifier for lazy lists.
    ///
    /// This connects pointer gestures to LazyListState for scroll handling.
    /// Unlike regular vertical_scroll, no layout offset is applied here
    /// since LazyListState manages item positioning internally.
    /// Creates a vertically scrollable modifier for lazy lists.
    ///
    /// This connects pointer gestures to LazyListState for scroll handling.
    /// Unlike regular vertical_scroll, no layout offset is applied here
    /// since LazyListState manages item positioning internally.
    pub fn lazy_vertical_scroll(self, state: LazyListState, reverse_scrolling: bool) -> Self {
        let motion_context = scroll_motion_context_for_key(ScrollMotionContextKey::LazyList {
            state_identity: state.inner_ptr() as usize,
            is_vertical: true,
            reverse_scrolling,
        });
        self.lazy_vertical_scroll_with_context(state, reverse_scrolling, motion_context)
    }

    pub(crate) fn lazy_vertical_scroll_with_context(
        self,
        state: LazyListState,
        reverse_scrolling: bool,
        motion_context: ScrollMotionContext,
    ) -> Self {
        self.then(lazy_scroll_impl(
            state,
            true,
            reverse_scrolling,
            motion_context,
        ))
    }

    pub(crate) fn lazy_horizontal_scroll_with_context(
        self,
        state: LazyListState,
        reverse_scrolling: bool,
        motion_context: ScrollMotionContext,
    ) -> Self {
        self.then(lazy_scroll_impl(
            state,
            false,
            reverse_scrolling,
            motion_context,
        ))
    }
}

fn lazy_scroll_impl(
    state: LazyListState,
    is_vertical: bool,
    reverse_scrolling: bool,
    motion_context: ScrollMotionContext,
) -> Modifier {
    let gesture_state = Rc::new(RefCell::new(ScrollGestureState::default()));
    let list_state = state;
    let state_id = state.inner_ptr() as usize;
    let key = (state_id, is_vertical, reverse_scrolling);
    let overscroll = motion_context.overscroll();
    let translated_content_modifier = Modifier::with_element(TranslatedContentContextElement::new(
        state_id,
        TranslatedContentOffsetSource::LazyList {
            state,
            is_vertical,
            reverse_scrolling,
        },
        overscroll,
    ));

    Modifier::with_element(MotionContextAnimatedElement::new(motion_context.clone()))
        .then(translated_content_modifier)
        .then(drag_gesture_input(
            key,
            DragGesture {
                target: list_state,
                gesture_state,
                is_vertical,
                reverse_input: reverse_scrolling,
                motion_context,
                guard: None,
            },
        ))
}

impl Modifier {
    /// Drags along `axis`, reporting each delta to `state`.
    ///
    /// The gesture is the one the scroll containers run — touch slop before a
    /// drag begins, axis locking so a mostly-vertical drag never steals a
    /// horizontal one, the primary pointer only, and yielding to whoever
    /// consumed the event first — so a dragged control and a scrolled list feel
    /// the same under a finger. Unlike a scroll, a release does not fling: a
    /// control the user placed stays where it was let go.
    ///
    /// Deltas are logical pixels, positive to the right for [`Axis::Horizontal`]
    /// and downwards for [`Axis::Vertical`].
    pub fn draggable(self, axis: Axis, state: DraggableState) -> Self {
        self.then(draggable_impl(axis, state, None))
    }

    /// Drags along `axis` only while `guard` says so.
    ///
    /// The guard is asked per event, so a control can stop accepting a drag
    /// half way through one and the gesture ends rather than being applied to
    /// a control that has since been disabled.
    pub fn draggable_guarded(
        self,
        axis: Axis,
        state: DraggableState,
        guard: impl Fn() -> bool + 'static,
    ) -> Self {
        self.then(draggable_impl(axis, state, Some(Rc::new(guard))))
    }
}

fn draggable_impl(
    axis: Axis,
    state: DraggableState,
    guard: Option<Rc<dyn Fn() -> bool>>,
) -> Modifier {
    let is_vertical = axis.is_vertical();
    let identity = state.identity();
    drag_gesture_input(
        (identity, is_vertical),
        DragGesture {
            target: state,
            gesture_state: Rc::new(RefCell::new(ScrollGestureState::default())),
            is_vertical,
            reverse_input: false,
            motion_context: scroll_motion_context_for_key(ScrollMotionContextKey::Draggable {
                state_identity: identity,
                is_vertical,
            }),
            guard,
        },
    )
}
