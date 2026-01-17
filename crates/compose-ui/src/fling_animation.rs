//! Fling animation driver for scroll containers.
//!
//! Drives decay animation using the runtime's frame callback system.

use compose_animation::{FloatDecayAnimationSpec, SplineBasedDecaySpec};
use compose_core::{FrameCallbackRegistration, FrameClock, RuntimeHandle};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Minimum velocity (in px/sec) to trigger a fling animation.
/// Below this, the scroll just stops immediately.
pub const MIN_FLING_VELOCITY: f32 = 1.0;

/// Default fling friction value (matches Android ViewConfiguration).
const DEFAULT_FLING_FRICTION: f32 = 0.015;

/// Minimum unconsumed delta (in pixels) to consider a boundary hit.
const BOUNDARY_EPSILON: f32 = 0.5;

/// Schedules the next fling animation frame without creating a FlingAnimation instance.
/// This is called recursively to drive the animation forward.
fn schedule_next_frame<F, G>(
    state: Rc<RefCell<Option<FlingAnimationState>>>,
    frame_clock: FrameClock,
    on_scroll: F,
    on_end: G,
) where
    F: Fn(f32) -> f32 + 'static,
    G: FnOnce() + 'static,
{
    let state_for_closure = state.clone();
    let frame_clock_for_closure = frame_clock.clone();
    let on_end = RefCell::new(Some(on_end));

    let registration = frame_clock.with_frame_nanos(move |frame_time_nanos| {
        let should_continue = {
            let state_guard = state_for_closure.borrow();
            let Some(anim_state) = state_guard.as_ref() else {
                return;
            };

            if !anim_state.is_running.get() {
                return;
            }

            let start_time = match anim_state.start_frame_time_nanos.get() {
                Some(value) => value,
                None => {
                    anim_state
                        .start_frame_time_nanos
                        .set(Some(frame_time_nanos));
                    frame_time_nanos
                }
            };

            let play_time_nanos = frame_time_nanos.saturating_sub(start_time) as i64;

            let new_value = anim_state.decay_spec.get_value_from_nanos(
                play_time_nanos,
                anim_state.initial_value,
                anim_state.initial_velocity,
            );

            let last = anim_state.last_value.get();
            let delta = new_value - last;
            anim_state.last_value.set(new_value);
            anim_state
                .total_delta
                .set(anim_state.total_delta.get() + delta);

            let duration_nanos = anim_state
                .decay_spec
                .get_duration_nanos(anim_state.initial_value, anim_state.initial_velocity);

            let current_velocity = anim_state.decay_spec.get_velocity_from_nanos(
                play_time_nanos,
                anim_state.initial_value,
                anim_state.initial_velocity,
            );

            let is_finished = play_time_nanos >= duration_nanos
                || current_velocity.abs() < anim_state.decay_spec.abs_velocity_threshold();

            if is_finished {
                anim_state.is_running.set(false);
            }

            let consumed = if delta.abs() > 0.001 {
                on_scroll(delta)
            } else {
                0.0
            };

            let hit_boundary = (delta - consumed).abs() > BOUNDARY_EPSILON;
            if hit_boundary {
                anim_state.is_running.set(false);
            }

            !is_finished && !hit_boundary
        };

        if should_continue {
            if let Some(on_end_fn) = on_end.borrow_mut().take() {
                schedule_next_frame(
                    state_for_closure.clone(),
                    frame_clock_for_closure.clone(),
                    on_scroll,
                    on_end_fn,
                );
            }
        } else {
            if let Some(end_fn) = on_end.borrow_mut().take() {
                end_fn();
            }
        }
    });

    // Store the registration to keep the callback alive
    if let Some(anim_state) = state.borrow_mut().as_mut() {
        anim_state.registration = Some(registration);
    }
}

/// State for an active fling animation.
struct FlingAnimationState {
    /// Initial position when fling started (used as reference for decay calc).
    initial_value: f32,
    /// Last applied position (to calculate delta for next frame).
    last_value: Cell<f32>,
    /// Initial velocity in px/sec.
    initial_velocity: f32,
    /// Frame time when the animation started (used for deterministic timing).
    start_frame_time_nanos: Cell<Option<u64>>,
    /// Decay animation spec for computing position/velocity.
    decay_spec: SplineBasedDecaySpec,
    /// Current frame callback registration (kept alive to continue animation).
    registration: Option<FrameCallbackRegistration>,
    /// Whether the animation is still active.
    is_running: Cell<bool>,
    /// Total delta applied so far (for debugging)
    total_delta: Cell<f32>,
}

/// Drives a fling (decay) animation on a scroll target.
///
/// Each frame, it calculates the scroll DELTA based on the decay curve
/// and applies it to the scroll target via the provided callback.
pub struct FlingAnimation {
    state: Rc<RefCell<Option<FlingAnimationState>>>,
    frame_clock: FrameClock,
}

impl FlingAnimation {
    /// Creates a new fling animation driver.
    pub fn new(runtime: RuntimeHandle) -> Self {
        Self {
            state: Rc::new(RefCell::new(None)),
            frame_clock: runtime.frame_clock(),
        }
    }

    /// Starts a fling animation with the given velocity.
    ///
    /// # Arguments
    /// * `initial_value` - Current scroll position (used as reference)
    /// * `velocity` - Initial velocity in px/sec (from VelocityTracker)
    /// * `density` - Screen density for physics calculations
    /// * `on_scroll` - Callback invoked each frame with scroll DELTA (not absolute position)
    /// * `on_end` - Callback invoked when animation completes
    pub fn start_fling<F, G>(
        &self,
        initial_value: f32,
        velocity: f32,
        density: f32,
        on_scroll: F,
        on_end: G,
    ) where
        F: Fn(f32) -> f32 + 'static, // Returns consumed amount
        G: FnOnce() + 'static,
    {
        // Cancel any existing animation
        self.cancel();

        // Check if velocity is high enough to warrant animation
        if velocity.abs() < MIN_FLING_VELOCITY {
            on_end();
            return;
        }

        // Match Jetpack Compose's default friction (ViewConfiguration.getScrollFriction).
        let friction = DEFAULT_FLING_FRICTION;
        let calc = compose_animation::FlingCalculator::new(friction, density);
        let decay_spec = SplineBasedDecaySpec::with_calculator(calc);



        let anim_state = FlingAnimationState {
            initial_value,
            last_value: Cell::new(initial_value),
            initial_velocity: velocity,
            start_frame_time_nanos: Cell::new(None),
            decay_spec,
            registration: None,
            is_running: Cell::new(true),
            total_delta: Cell::new(0.0),
        };

        *self.state.borrow_mut() = Some(anim_state);

        // Start frame loop
        schedule_next_frame(self.state.clone(), self.frame_clock.clone(), on_scroll, on_end);
    }

    pub fn cancel(&self) {
        if let Some(state) = self.state.borrow_mut().take() {
            // Mark as not running to prevent callback from doing anything
            state.is_running.set(false);
            // Registration is dropped, cancelling the callback
            drop(state.registration);
        }
    }

    /// Returns true if a fling animation is currently running.
    pub fn is_running(&self) -> bool {
        self.state
            .borrow()
            .as_ref()
            .is_some_and(|s| s.is_running.get())
    }

}

impl Clone for FlingAnimation {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            frame_clock: self.frame_clock.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compose_core::DefaultScheduler;
    use compose_core::Runtime;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    #[test]
    fn test_min_velocity_threshold() {
        assert_eq!(MIN_FLING_VELOCITY, 1.0);
    }

    #[test]
    fn test_on_end_called_when_boundary_hit() {
        let runtime = Runtime::new(Arc::new(DefaultScheduler));
        let handle = runtime.handle();
        let fling = FlingAnimation::new(handle.clone());
        let finished = Rc::new(Cell::new(false));
        let finished_flag = Rc::clone(&finished);

        fling.start_fling(0.0, 10_000.0, 1.0, |_| 0.0, move || finished_flag.set(true));

        handle.drain_frame_callbacks(0);
        handle.drain_frame_callbacks(16_000_000);

        assert!(finished.get());
    }
}
