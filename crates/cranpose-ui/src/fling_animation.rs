//! Fling animation driver for scroll containers.
//!
//! Drives decay animation using the runtime's frame callback system.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_animation::{
    ExponentialDecaySpec, FloatDecayAnimationSpec, IOS_DECELERATION_RATE_NORMAL,
};
use cranpose_core::{
    RuntimeHandle,
    internal::{FrameCallbackRegistration, FrameClock},
};

/// Minimum release velocity (in points/sec) for `UIScrollView` to start
/// decelerating at all; below this a release just stops. Measured on the iOS
/// 26.5 Simulator: releases consistently below ~260pt/s never decelerate,
/// releases consistently above ~350pt/s always do, with a noisy transition
/// between (real touch-velocity noise near a threshold, not a measurement
/// artifact — see `ios_fling_measurement.rs`). 300 sits in that band.
pub const MIN_FLING_VELOCITY: f32 = 300.0;

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

            let boundary_hit = (delta - consumed).abs() > BOUNDARY_EPSILON;
            if boundary_hit {
                anim_state.is_running.set(false);
            }

            !is_finished && !boundary_hit
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
        } else if let Some(end_fn) = on_end.borrow_mut().take() {
            end_fn();
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
    decay_spec: ExponentialDecaySpec,
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
    /// * `on_scroll` - Callback invoked each frame with scroll DELTA (not absolute position)
    /// * `on_end` - Callback invoked when animation completes
    pub fn start_fling<F, G>(&self, initial_value: f32, velocity: f32, on_scroll: F, on_end: G)
    where
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

        let decay_spec = ExponentialDecaySpec::new(IOS_DECELERATION_RATE_NORMAL);

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
        schedule_next_frame(
            self.state.clone(),
            self.frame_clock.clone(),
            on_scroll,
            on_end,
        );
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

/// Predicts where a fling starting at `initial_value` with `velocity` would
/// naturally come to rest, using the same decay physics as
/// [`FlingAnimation::start_fling`]. Settle policies remap this proposed rest
/// position before the deceleration starts (the `UIScrollView
/// targetContentOffset` analog).
pub fn fling_rest_position(initial_value: f32, velocity: f32) -> f32 {
    if velocity.abs() < MIN_FLING_VELOCITY {
        return initial_value;
    }
    let spec = ExponentialDecaySpec::new(IOS_DECELERATION_RATE_NORMAL);
    spec.get_target_value(initial_value, velocity)
}

/// Damped-spring parameters for a [`SettleAnimation`]. `advance_spring`
/// already generalizes over damping ratio, so the two settle use cases in
/// this crate share one scheduler and differ only in these two numbers.
#[derive(Debug, Clone, Copy)]
pub struct SpringParams {
    pub stiffness: f32,
    pub damping_ratio: f32,
}

impl SpringParams {
    /// Settle-policy remapping (e.g. the liquid nav bar's title-collapse
    /// snap): critically damped, settling in ≈0.35s, the iOS large-title
    /// snap feel.
    pub const SETTLE_POLICY: Self = Self {
        stiffness: 300.0,
        damping_ratio: 1.0,
    };

    /// Overscroll bounce-back: the spring a scroll container's rubber-banded
    /// offset relaxes through once the finger releases (or a fling's own
    /// velocity decays) while still past the edge. Fit to a `UIScrollView`
    /// bounce-back trace recorded on the iOS 26.5 Simulator (drag past the
    /// top edge, hold to zero velocity, release: -177pt to rest in 667ms) —
    /// see `ios_fling_measurement.rs`. Overdamped rather than critical: a
    /// critically-damped ζ=1 spring at the same stiffness overshoots the
    /// measured curve's shape by 15-20x; this (stiffness, damping_ratio) pair
    /// was fit by least squares against the recorded trace, residual ≤2.9pt
    /// (mean 0.79pt) against a 177pt swing.
    pub const OVERSCROLL_BOUNCE: Self = Self {
        stiffness: 1909.69,
        damping_ratio: 2.71,
    };
}

/// Position/velocity epsilons below which a settle animation finishes.
const SETTLE_REST_DISTANCE: f32 = 0.1;
const SETTLE_REST_VELOCITY: f32 = 4.0;

struct SettleAnimationState {
    value: Cell<f32>,
    velocity: Cell<f32>,
    target: f32,
    params: SpringParams,
    last_frame_time_nanos: Cell<Option<u64>>,
    registration: Option<FrameCallbackRegistration>,
    is_running: Cell<bool>,
}

pub(crate) struct SettleEnd {
    pub(crate) velocity: f32,
    pub(crate) hit_boundary: bool,
}

/// Drives a damped spring toward a settle target on a scroll container,
/// taking over the gesture's release velocity so a policy-adjusted rest
/// position (or an overscroll bounce-back) still reads as one continuous
/// deceleration.
pub struct SettleAnimation {
    state: Rc<RefCell<Option<SettleAnimationState>>>,
    frame_clock: FrameClock,
    params: SpringParams,
}

impl SettleAnimation {
    pub fn new(runtime: RuntimeHandle, params: SpringParams) -> Self {
        Self {
            state: Rc::new(RefCell::new(None)),
            frame_clock: runtime.frame_clock(),
            params,
        }
    }

    /// Starts settling from `initial_value` (with `initial_velocity`, in
    /// offset units/sec) toward `target`. `on_scroll` receives per-frame
    /// deltas and returns the consumed amount; `on_end` fires once when the
    /// spring rests or the target stops consuming (boundary hit).
    pub(crate) fn start_settle<F, G>(
        &self,
        initial_value: f32,
        initial_velocity: f32,
        target: f32,
        on_scroll: F,
        on_end: G,
    ) where
        F: Fn(f32) -> f32 + 'static,
        G: FnOnce(SettleEnd) + 'static,
    {
        self.cancel();
        *self.state.borrow_mut() = Some(SettleAnimationState {
            value: Cell::new(initial_value),
            velocity: Cell::new(initial_velocity),
            target,
            params: self.params,
            last_frame_time_nanos: Cell::new(None),
            registration: None,
            is_running: Cell::new(true),
        });
        schedule_next_settle_frame(
            self.state.clone(),
            self.frame_clock.clone(),
            on_scroll,
            on_end,
        );
    }

    pub fn cancel(&self) {
        if let Some(state) = self.state.borrow_mut().take() {
            state.is_running.set(false);
            drop(state.registration);
        }
    }

    pub fn is_running(&self) -> bool {
        self.state
            .borrow()
            .as_ref()
            .is_some_and(|s| s.is_running.get())
    }
}

impl Clone for SettleAnimation {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            frame_clock: self.frame_clock.clone(),
            params: self.params,
        }
    }
}

fn schedule_next_settle_frame<F, G>(
    state: Rc<RefCell<Option<SettleAnimationState>>>,
    frame_clock: FrameClock,
    on_scroll: F,
    on_end: G,
) where
    F: Fn(f32) -> f32 + 'static,
    G: FnOnce(SettleEnd) + 'static,
{
    let state_for_closure = state.clone();
    let frame_clock_for_closure = frame_clock.clone();
    let on_end = RefCell::new(Some(on_end));
    let hit_boundary = Cell::new(false);

    let registration = frame_clock.with_frame_nanos(move |frame_time_nanos| {
        let should_continue = {
            let state_guard = state_for_closure.borrow();
            let Some(anim_state) = state_guard.as_ref() else {
                return;
            };
            if !anim_state.is_running.get() {
                return;
            }

            let dt = match anim_state.last_frame_time_nanos.get() {
                Some(last) => (frame_time_nanos.saturating_sub(last) as f32) / 1_000_000_000.0,
                None => 0.0,
            };
            anim_state.last_frame_time_nanos.set(Some(frame_time_nanos));

            let (mut next_value, next_velocity) = cranpose_animation::advance_spring(
                anim_state.value.get(),
                anim_state.velocity.get(),
                anim_state.target,
                anim_state.params.damping_ratio,
                anim_state.params.stiffness,
                dt.max(0.0),
            );

            let is_finished = (next_value - anim_state.target).abs() < SETTLE_REST_DISTANCE
                && next_velocity.abs() < SETTLE_REST_VELOCITY;
            if is_finished {
                next_value = anim_state.target;
                anim_state.is_running.set(false);
            }

            let delta = next_value - anim_state.value.get();
            anim_state.value.set(next_value);
            anim_state.velocity.set(next_velocity);

            let consumed = if delta.abs() > 0.0001 {
                on_scroll(delta)
            } else {
                delta
            };
            let boundary_hit = (delta - consumed).abs() > BOUNDARY_EPSILON;
            if boundary_hit {
                anim_state.is_running.set(false);
                hit_boundary.set(true);
            }

            !is_finished && !boundary_hit
        };

        if should_continue {
            if let Some(on_end_fn) = on_end.borrow_mut().take() {
                schedule_next_settle_frame(
                    state_for_closure.clone(),
                    frame_clock_for_closure.clone(),
                    on_scroll,
                    on_end_fn,
                );
            }
        } else if let Some(end_fn) = on_end.borrow_mut().take() {
            let state_guard = state_for_closure.borrow();
            let velocity = state_guard
                .as_ref()
                .map_or(0.0, |anim_state| anim_state.velocity.get());
            end_fn(SettleEnd {
                velocity,
                hit_boundary: hit_boundary.get(),
            });
        }
    });

    if let Some(anim_state) = state.borrow_mut().as_mut() {
        anim_state.registration = Some(registration);
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc, sync::Arc};

    use cranpose_core::{DefaultScheduler, Runtime};

    use super::*;

    #[test]
    fn test_min_velocity_threshold() {
        assert_eq!(MIN_FLING_VELOCITY, 300.0);
    }

    #[test]
    fn settle_animation_springs_to_target_and_ends() {
        let runtime = Runtime::new(Arc::new(DefaultScheduler));
        let handle = runtime.handle();
        let settle = SettleAnimation::new(handle.clone(), SpringParams::SETTLE_POLICY);
        let position = Rc::new(Cell::new(30.0f32));
        let ended = Rc::new(Cell::new(false));
        let position_for_scroll = Rc::clone(&position);
        let ended_for_end = Rc::clone(&ended);
        settle.start_settle(
            30.0,
            0.0,
            52.0,
            move |delta| {
                position_for_scroll.set(position_for_scroll.get() + delta);
                delta
            },
            move |_| ended_for_end.set(true),
        );
        for frame in 0..240u64 {
            handle.drain_frame_callbacks(frame * 16_000_000);
            if ended.get() {
                break;
            }
        }
        assert!(ended.get(), "settle animation must finish");
        assert!(
            (position.get() - 52.0).abs() < 0.2,
            "settle must land on the target, got {}",
            position.get()
        );
    }

    #[test]
    fn settle_reports_reduced_velocity_when_crossing_boundary() {
        let runtime = Runtime::new(Arc::new(DefaultScheduler));
        let handle = runtime.handle();
        let settle = SettleAnimation::new(handle.clone(), SpringParams::SETTLE_POLICY);
        let position = Rc::new(Cell::new(30.0f32));
        let ended = Rc::new(Cell::new(None::<(f32, bool)>));
        let position_for_scroll = Rc::clone(&position);
        let ended_for_end = Rc::clone(&ended);
        settle.start_settle(
            30.0,
            -1_200.0,
            0.0,
            move |delta| {
                let previous = position_for_scroll.get();
                let next = (previous + delta).max(0.0);
                position_for_scroll.set(next);
                next - previous
            },
            move |end| ended_for_end.set(Some((end.velocity, end.hit_boundary))),
        );
        for frame in 0..240u64 {
            handle.drain_frame_callbacks(frame * 16_000_000);
            if ended.get().is_some() {
                break;
            }
        }

        let (velocity, hit_boundary) = ended.get().expect("settle must finish");
        assert!(hit_boundary);
        assert!(velocity < 0.0 && velocity.abs() < 1_200.0);
        assert_eq!(position.get(), 0.0);
    }

    #[test]
    fn fling_rest_position_is_beyond_start_in_fling_direction() {
        let rest = fling_rest_position(100.0, 900.0);
        assert!(rest > 100.0, "rest {rest} must be past the start");
        assert_eq!(fling_rest_position(100.0, 0.0), 100.0);
    }

    #[test]
    fn test_on_end_called_when_boundary_hit() {
        let runtime = Runtime::new(Arc::new(DefaultScheduler));
        let handle = runtime.handle();
        let fling = FlingAnimation::new(handle.clone());
        let finished = Rc::new(Cell::new(false));
        let finished_flag = Rc::clone(&finished);

        fling.start_fling(0.0, 10_000.0, |_| 0.0, move || finished_flag.set(true));

        handle.drain_frame_callbacks(0);
        handle.drain_frame_callbacks(16_000_000);

        assert!(finished.get());
    }
}
