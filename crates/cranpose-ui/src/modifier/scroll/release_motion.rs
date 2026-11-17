//! What a scroller's release starts: the fling, the settle on the target's
//! policy, or the bounce back from an overscroll.

use cranpose_core::current_runtime_handle;

use super::{
    FlingAnimation, MIN_FLING_VELOCITY, ScrollGestureDetector, ScrollTarget, SettleAnimation,
    SpringParams, fling_rest_position, set_last_fling_velocity,
};

impl<S: ScrollTarget + 'static> ScrollGestureDetector<S> {
    pub(super) fn on_up(&self, time_ms: Option<i64>) -> bool {
        self.finish_gesture(self.scroll_target.allows_fling(), time_ms)
    }

    pub(super) fn on_cancel(&self) -> bool {
        self.finish_gesture(false, None)
    }

    fn finish_gesture(&self, allow_fling: bool, release_time_ms: Option<i64>) -> bool {
        let (was_dragging, interrupted, gesture_owned, velocity, start_fling) = {
            let mut gs = self.gesture_state.borrow_mut();
            let was_dragging = gs.is_dragging;
            let interrupted = std::mem::take(&mut gs.interrupted_animation);
            let gesture_owned = was_dragging || gs.is_overscrolling;
            let velocity = if allow_fling && gesture_owned {
                gs.release_velocity(release_time_ms)
            } else {
                0.0
            };

            let start_fling = allow_fling && was_dragging && velocity.abs() > MIN_FLING_VELOCITY;
            // Whatever the release starts replaces a fling still running.
            if start_fling && let Some(fling) = gs.fling_animation.take() {
                fling.cancel();
            }

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
                interrupted,
                gesture_owned,
                velocity,
                start_fling,
            )
        };

        if allow_fling && gesture_owned {
            log::debug!(
                target: "cranpose::velocity",
                "gesture finished: fling velocity={velocity:.2} dp/s start_fling={start_fling}"
            );
            set_last_fling_velocity(velocity);
        }

        let adjusted_velocity = self.toward_target(velocity);
        // The containers around the scroller take their part first; a fling
        // with too little left does not start.
        let adjusted_velocity = if gesture_owned {
            adjusted_velocity - self.scroll_target.pre_fling(adjusted_velocity)
        } else {
            adjusted_velocity
        };
        let start_fling = start_fling && adjusted_velocity.abs() > MIN_FLING_VELOCITY;
        let fling_velocity = -adjusted_velocity;
        let has_overscroll = self.overscroll.offset().abs() > 0.001;

        let settle_target = (was_dragging || interrupted)
            .then(|| self.settle_target(start_fling, fling_velocity))
            .flatten();

        if has_overscroll {
            self.start_overscroll_settle(-fling_velocity);
        } else if let Some(target) = settle_target {
            self.start_settle_animation(target, fling_velocity);
        } else if start_fling {
            self.start_fling_animation(fling_velocity);
        } else {
            self.motion_context.set_active(false);
        }

        gesture_owned
    }

    /// Where the target's settle policy moves the rest of a release, if it
    /// moves it at all.
    fn settle_target(&self, start_fling: bool, fling_velocity: f32) -> Option<f32> {
        let policy = self.scroll_target.settle_policy()?;
        let current = self.scroll_target.current_offset();
        let proposed = if start_fling {
            fling_rest_position(current, fling_velocity)
        } else {
            current
        };
        let target = policy(proposed, fling_velocity);
        ((target - proposed).abs() > 0.5).then_some(target)
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
                scroll_target.invalidate();
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

    pub(super) fn start_settle_animation(&self, target: f32, initial_velocity: f32) {
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

    pub(super) fn start_overscroll_settle(&self, initial_velocity: f32) {
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
}
