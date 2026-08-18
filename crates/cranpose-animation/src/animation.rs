//! Animation system for Cranpose
//!
//! Provides time-based animations with easing curves and spring physics.
//!
//! Note: This module uses camelCase for method names (animateTo, snapTo) to maintain
//! 1:1 API parity with Jetpack Compose.

#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::{Rc, Weak};

use cranpose_core::internal::FrameCallbackRegistration;
use cranpose_core::{
    with_current_composer, DisposableEffectResult, Owned, OwnedMutableState, RuntimeHandle,
    SideEffect, State,
};

/// Trait for types that can be linearly interpolated.
pub trait Lerp {
    fn lerp(&self, target: &Self, fraction: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(&self, target: &Self, fraction: f32) -> Self {
        self + (target - self) * fraction
    }
}

impl Lerp for f64 {
    fn lerp(&self, target: &Self, fraction: f32) -> Self {
        self + (target - self) * fraction as f64
    }
}

/// Values springs can animate: fixed-dimension float vectors (at most
/// [`SPRING_MAX_DIMENSIONS`]), mirroring Compose's `AnimationVector1D..4D`.
///
/// The spring integrates each dimension independently **in value space** —
/// velocity is expressed in value units per second — so retargeting an
/// animation mid-flight keeps the physical velocity (no hitch), and gestures
/// can hand their release velocity to [`Animatable::animate_to_with_velocity`].
pub trait SpringScalar: Lerp + Clone {
    /// Number of animated dimensions (1..=4).
    const DIMENSIONS: usize;

    /// Reads dimension `index` (`index < Self::DIMENSIONS`).
    fn dimension(&self, index: usize) -> f32;

    /// Rebuilds a value from per-dimension floats (indices beyond
    /// [`Self::DIMENSIONS`] are ignored).
    fn from_dimensions(dimensions: [f32; SPRING_MAX_DIMENSIONS]) -> Self;
}

/// Upper bound on [`SpringScalar::DIMENSIONS`].
pub const SPRING_MAX_DIMENSIONS: usize = 4;

/// Advances one damped-harmonic-oscillator dimension by `dt` seconds using the
/// closed-form solution for unit mass (`ω = √stiffness`, damping `c = 2ζω`).
/// Returns the new `(value, velocity)`; exact for any `dt`, so springs stay
/// correct across dropped frames and long pauses.
pub fn advance_spring(
    value: f32,
    velocity: f32,
    target: f32,
    damping_ratio: f32,
    stiffness: f32,
    dt: f32,
) -> (f32, f32) {
    let omega = stiffness.max(f32::EPSILON).sqrt();
    let zeta = damping_ratio.max(0.0);
    let displacement = value - target;

    if (zeta - 1.0).abs() < 1e-4 {
        // Critically damped: x(t) = (c1 + c2·t)·e^(−ωt)
        let c1 = displacement;
        let c2 = velocity + omega * displacement;
        let decay = (-omega * dt).exp();
        let next_displacement = (c1 + c2 * dt) * decay;
        let next_velocity = (c2 - omega * (c1 + c2 * dt)) * decay;
        (target + next_displacement, next_velocity)
    } else if zeta < 1.0 {
        // Underdamped: decaying oscillation at ω_d = ω·√(1−ζ²).
        let omega_d = omega * (1.0 - zeta * zeta).sqrt();
        let decay = (-zeta * omega * dt).exp();
        let (sin, cos) = (omega_d * dt).sin_cos();
        let a = displacement;
        let b = (velocity + zeta * omega * displacement) / omega_d;
        let next_displacement = decay * (a * cos + b * sin);
        let next_velocity = decay
            * ((b * omega_d - a * zeta * omega) * cos - (a * omega_d + b * zeta * omega) * sin);
        (target + next_displacement, next_velocity)
    } else {
        // Overdamped: sum of two decaying exponentials.
        let root = (zeta * zeta - 1.0).sqrt();
        let r1 = -omega * (zeta - root);
        let r2 = -omega * (zeta + root);
        let c2 = (velocity - r1 * displacement) / (r2 - r1);
        let c1 = displacement - c2;
        let e1 = (r1 * dt).exp();
        let e2 = (r2 * dt).exp();
        (target + c1 * e1 + c2 * e2, c1 * r1 * e1 + c2 * r2 * e2)
    }
}

impl SpringScalar for f32 {
    const DIMENSIONS: usize = 1;

    fn dimension(&self, _index: usize) -> f32 {
        *self
    }

    fn from_dimensions(dimensions: [f32; SPRING_MAX_DIMENSIONS]) -> Self {
        dimensions[0]
    }
}

impl SpringScalar for f64 {
    const DIMENSIONS: usize = 1;

    fn dimension(&self, _index: usize) -> f32 {
        *self as f32
    }

    fn from_dimensions(dimensions: [f32; SPRING_MAX_DIMENSIONS]) -> Self {
        f64::from(dimensions[0])
    }
}

/// Easing functions for animations matching Jetpack Compose.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Easing {
    /// Linear interpolation (no easing).
    /// Jetpack Compose: LinearEasing
    LinearEasing,
    /// Ease in using cubic curve.
    /// Jetpack Compose: EaseIn (not a standard constant, but supported)
    EaseIn,
    /// Ease out using cubic curve.
    /// Jetpack Compose: EaseOut (not a standard constant, but supported)
    EaseOut,
    /// Ease in and out using cubic curve.
    /// Jetpack Compose: EaseInOut (not a standard constant, but supported)
    EaseInOut,
    /// Fast out, slow in (material design standard).
    /// Jetpack Compose: FastOutSlowInEasing
    FastOutSlowInEasing,
    /// Linear out, slow in (material design).
    /// Jetpack Compose: LinearOutSlowInEasing
    LinearOutSlowInEasing,
    /// Fast out, linear in (material design).
    /// Jetpack Compose: FastOutLinearEasing
    FastOutLinearEasing,
}

impl Easing {
    /// Apply the easing function to a linear fraction [0, 1].
    pub fn transform(&self, fraction: f32) -> f32 {
        match self {
            Easing::LinearEasing => fraction,
            Easing::EaseIn => cubic_bezier(0.42, 0.0, 1.0, 1.0, fraction),
            Easing::EaseOut => cubic_bezier(0.0, 0.0, 0.58, 1.0, fraction),
            Easing::EaseInOut => cubic_bezier(0.42, 0.0, 0.58, 1.0, fraction),
            Easing::FastOutSlowInEasing => cubic_bezier(0.4, 0.0, 0.2, 1.0, fraction),
            Easing::LinearOutSlowInEasing => cubic_bezier(0.0, 0.0, 0.2, 1.0, fraction),
            Easing::FastOutLinearEasing => cubic_bezier(0.4, 0.0, 1.0, 1.0, fraction),
        }
    }
}

/// Cubic bezier curve approximation for easing.
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, fraction: f32) -> f32 {
    if fraction <= 0.0 {
        return 0.0;
    }
    if fraction >= 1.0 {
        return 1.0;
    }

    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;

    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;

    fn sample_curve(a: f32, b: f32, c: f32, t: f32) -> f32 {
        ((a * t + b) * t + c) * t
    }

    fn sample_derivative(a: f32, b: f32, c: f32, t: f32) -> f32 {
        (3.0 * a * t + 2.0 * b) * t + c
    }

    // Use Newton-Raphson iterations to solve for the parametric value `t`
    // corresponding to the provided x fraction. Clamp to [0, 1] to keep the
    // solution within bounds.
    let mut t = fraction;
    let mut newton_success = false;
    for _ in 0..8 {
        let x = sample_curve(ax, bx, cx, t) - fraction;
        if x.abs() < 1e-6 {
            newton_success = true;
            break;
        }
        let dx = sample_derivative(ax, bx, cx, t);
        if dx.abs() < 1e-6 {
            break;
        }
        t = (t - x / dx).clamp(0.0, 1.0);
    }

    if !newton_success {
        // Fall back to a binary subdivision if Newton-Raphson did not converge.
        let mut t0 = 0.0;
        let mut t1 = 1.0;
        t = fraction;
        for _ in 0..16 {
            let x = sample_curve(ax, bx, cx, t);
            let delta = x - fraction;
            if delta.abs() < 1e-6 {
                break;
            }
            if delta > 0.0 {
                t1 = t;
            } else {
                t0 = t;
            }
            t = 0.5 * (t0 + t1);
        }
    }

    sample_curve(ay, by, cy, t)
}

/// Animation specification combining duration and easing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimationSpec {
    /// Duration in milliseconds.
    pub duration_millis: u64,
    /// Easing function to apply.
    pub easing: Easing,
    /// Delay before starting animation in milliseconds.
    pub delay_millis: u64,
}

impl AnimationSpec {
    /// Create a tween animation with duration and easing.
    pub fn tween(duration_millis: u64, easing: Easing) -> Self {
        Self {
            duration_millis,
            easing,
            delay_millis: 0,
        }
    }

    /// Create a linear tween animation.
    pub fn linear(duration_millis: u64) -> Self {
        Self::tween(duration_millis, Easing::LinearEasing)
    }

    /// Add a delay before the animation starts.
    pub fn with_delay(mut self, delay_millis: u64) -> Self {
        self.delay_millis = delay_millis;
        self
    }
}

impl Default for AnimationSpec {
    fn default() -> Self {
        Self::tween(300, Easing::FastOutSlowInEasing)
    }
}

/// Repeat mode for infinite animations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    /// Restart from the beginning each cycle.
    Restart,
    /// Reverse direction every other cycle.
    Reverse,
}

/// Start offset type for infinite animations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartOffsetType {
    /// Delay the start by the specified offset.
    Delay,
    /// Fast forward the start by the specified offset.
    FastForward,
}

/// Start offset configuration for infinite animations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartOffset {
    /// Offset in milliseconds.
    pub offset_millis: i64,
    /// Offset behavior (delay or fast-forward).
    pub offset_type: StartOffsetType,
}

impl Default for StartOffset {
    fn default() -> Self {
        Self {
            offset_millis: 0,
            offset_type: StartOffsetType::Delay,
        }
    }
}

/// Infinite repeatable animation spec built from a duration-based animation.
#[derive(Debug, Clone, PartialEq)]
pub struct InfiniteRepeatableSpec<T> {
    /// Base animation used for each iteration.
    pub animation: AnimationSpec,
    /// Repeat mode (restart or reverse).
    pub repeat_mode: RepeatMode,
    /// Start offset applied before the first iteration.
    pub initial_start_offset: StartOffset,
    _marker: PhantomData<fn() -> T>,
}

/// Creates an infinite repeatable animation spec.
pub fn infiniteRepeatable<T>(
    animation: AnimationSpec,
    repeat_mode: RepeatMode,
    initial_start_offset: StartOffset,
) -> InfiniteRepeatableSpec<T> {
    InfiniteRepeatableSpec {
        animation,
        repeat_mode,
        initial_start_offset,
        _marker: PhantomData,
    }
}

/// Spring animation configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringSpec {
    /// Damping ratio. 1.0 = critically damped, < 1.0 = under-damped (bouncy), > 1.0 = over-damped.
    pub damping_ratio: f32,
    /// Stiffness constant. Higher values = faster animation.
    pub stiffness: f32,
    /// Velocity threshold to stop animation.
    pub velocity_threshold: f32,
    /// Position threshold to stop animation.
    pub position_threshold: f32,
    /// Delay before the spring begins advancing.
    pub delay_millis: u64,
}

impl SpringSpec {
    /// Create a spring with explicit Compose-style physics constants.
    pub fn new(damping_ratio: f32, stiffness: f32) -> Self {
        Self {
            damping_ratio,
            stiffness,
            velocity_threshold: 0.1,
            position_threshold: 0.01,
            delay_millis: 0,
        }
    }

    /// Add a delay before the spring starts integrating.
    pub fn with_delay(mut self, delay_millis: u64) -> Self {
        self.delay_millis = delay_millis;
        self
    }

    /// Create a spring with default material design values.
    pub fn default_spring() -> Self {
        Self {
            damping_ratio: 1.0,
            stiffness: 1500.0,
            velocity_threshold: 0.1,
            position_threshold: 0.01,
            delay_millis: 0,
        }
    }

    /// Create a bouncy spring.
    pub fn bouncy() -> Self {
        Self {
            damping_ratio: 0.5,
            stiffness: 1500.0,
            velocity_threshold: 0.1,
            position_threshold: 0.01,
            delay_millis: 0,
        }
    }

    /// Create a stiff spring (fast, no bounce).
    pub fn stiff() -> Self {
        Self {
            damping_ratio: 1.0,
            stiffness: 3000.0,
            velocity_threshold: 0.1,
            position_threshold: 0.01,
            delay_millis: 0,
        }
    }
}

impl Default for SpringSpec {
    fn default() -> Self {
        Self::default_spring()
    }
}

/// Compose-compatible spring constants.
pub struct Spring;

impl Spring {
    pub const DampingRatioNoBouncy: f32 = 1.0;
    pub const DampingRatioLowBouncy: f32 = 0.75;
    pub const DampingRatioMediumBouncy: f32 = 0.5;
    pub const DampingRatioHighBouncy: f32 = 0.2;

    pub const StiffnessHigh: f32 = 10_000.0;
    pub const StiffnessMedium: f32 = 1_500.0;
    pub const StiffnessMediumLow: f32 = 400.0;
    pub const StiffnessLow: f32 = 200.0;
    pub const StiffnessVeryLow: f32 = 50.0;
}

/// Compose-style spring animation spec factory.
pub fn spring(damping_ratio: f32, stiffness: f32) -> AnimationType {
    AnimationType::Spring(SpringSpec::new(damping_ratio, stiffness))
}

/// Compose-style tween animation spec factory.
pub fn tween(duration_millis: u64, easing: Easing) -> AnimationType {
    AnimationType::Tween(AnimationSpec::tween(duration_millis, easing))
}

/// Animation type specification.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnimationType {
    /// Time-based tween animation.
    Tween(AnimationSpec),
    /// Physics-based spring animation.
    Spring(SpringSpec),
}

impl AnimationType {
    /// Add the same start delay regardless of the animation model.
    pub fn with_delay(self, delay_millis: u64) -> Self {
        match self {
            Self::Tween(spec) => Self::Tween(spec.with_delay(delay_millis)),
            Self::Spring(spec) => Self::Spring(spec.with_delay(delay_millis)),
        }
    }
}

impl Default for AnimationType {
    fn default() -> Self {
        AnimationType::Tween(AnimationSpec::default())
    }
}

trait InfiniteTransitionAnimation {
    fn on_frame(&self, play_time_nanos: u64);
    fn has_subscribers(&self) -> bool;
}

struct TransitionAnimationState<T: Lerp + Clone + PartialEq + 'static> {
    value_state: OwnedMutableState<T>,
    initial_value: RefCell<T>,
    target_value: RefCell<T>,
    spec: RefCell<InfiniteRepeatableSpec<T>>,
    start_on_next_frame: Cell<bool>,
    play_time_offset_nanos: Cell<u64>,
    subscriber_callback_installed: Cell<bool>,
    subscriber_callback: RefCell<Option<Rc<dyn Fn()>>>,
}

impl<T: Lerp + Clone + PartialEq + 'static> TransitionAnimationState<T> {
    fn new(
        initial_value: T,
        target_value: T,
        spec: InfiniteRepeatableSpec<T>,
        runtime: RuntimeHandle,
    ) -> Self {
        Self {
            value_state: OwnedMutableState::with_runtime(initial_value.clone(), runtime),
            initial_value: RefCell::new(initial_value),
            target_value: RefCell::new(target_value),
            spec: RefCell::new(spec),
            start_on_next_frame: Cell::new(true),
            play_time_offset_nanos: Cell::new(0),
            subscriber_callback_installed: Cell::new(false),
            subscriber_callback: RefCell::new(None),
        }
    }

    fn state(&self) -> State<T> {
        self.value_state.as_state()
    }

    fn update_values(&self, initial_value: T, target_value: T, spec: InfiniteRepeatableSpec<T>) {
        let needs_update = {
            let current_initial = self.initial_value.borrow();
            let current_target = self.target_value.borrow();
            *current_initial != initial_value
                || *current_target != target_value
                || *self.spec.borrow() != spec
        };

        if needs_update {
            *self.initial_value.borrow_mut() = initial_value.clone();
            *self.target_value.borrow_mut() = target_value;
            *self.spec.borrow_mut() = spec;
            self.start_on_next_frame.set(true);
            self.value_state.set(initial_value);
        }
    }

    fn compute_value(&self, play_time_nanos: u64) -> T {
        let offset = if self.start_on_next_frame.get() {
            self.start_on_next_frame.set(false);
            self.play_time_offset_nanos.set(play_time_nanos);
            play_time_nanos
        } else {
            self.play_time_offset_nanos.get()
        };
        let local_play_time = play_time_nanos.saturating_sub(offset);
        let spec = self.spec.borrow().clone();
        let initial = self.initial_value.borrow();
        let target = self.target_value.borrow();
        compute_repeatable_value(local_play_time, &initial, &target, spec)
    }

    fn install_subscriber_callback(&self, callback: Rc<dyn Fn()>) {
        if !self.subscriber_callback_installed.replace(true) {
            self.subscriber_callback
                .borrow_mut()
                .replace(callback.clone());
            self.value_state.as_state().on_subscriber(callback);
        }
    }
}

impl<T: Lerp + Clone + PartialEq + 'static> InfiniteTransitionAnimation
    for TransitionAnimationState<T>
{
    fn on_frame(&self, play_time_nanos: u64) {
        let value = self.compute_value(play_time_nanos);
        self.value_state.set(value);
    }

    fn has_subscribers(&self) -> bool {
        self.value_state.as_state().has_subscribers()
    }
}

fn compute_repeatable_value<T: Lerp + Clone>(
    play_time_nanos: u64,
    initial: &T,
    target: &T,
    spec: InfiniteRepeatableSpec<T>,
) -> T {
    let duration_ms = spec.animation.duration_millis.max(1) as i64;
    let delay_ms = spec.animation.delay_millis as i64;
    let mut play_time_ms = (play_time_nanos / 1_000_000) as i64;

    match spec.initial_start_offset.offset_type {
        StartOffsetType::Delay => {
            play_time_ms -= spec.initial_start_offset.offset_millis;
        }
        StartOffsetType::FastForward => {
            play_time_ms += spec.initial_start_offset.offset_millis;
        }
    }

    if play_time_ms < 0 {
        return initial.clone();
    }

    let iteration_duration = (delay_ms + duration_ms).max(1);
    let iteration = play_time_ms / iteration_duration;
    let iteration_time = play_time_ms % iteration_duration;

    let reverse = matches!(spec.repeat_mode, RepeatMode::Reverse) && iteration % 2 != 0;
    let (start, end) = if reverse {
        (target, initial)
    } else {
        (initial, target)
    };

    if iteration_time < delay_ms {
        return start.clone();
    }

    let linear_progress = ((iteration_time - delay_ms) as f32 / duration_ms as f32).clamp(0.0, 1.0);
    let eased = spec.animation.easing.transform(linear_progress);
    start.lerp(end, eased)
}

#[derive(Clone)]
pub struct InfiniteTransition {
    inner: Rc<InfiniteTransitionInner>,
}

struct InfiniteTransitionInner {
    label: String,
    animations: RefCell<Vec<Rc<dyn InfiniteTransitionAnimation>>>,
    run_token: OwnedMutableState<u64>,
    restart_pending: Cell<bool>,
    runtime: RuntimeHandle,
}

impl InfiniteTransition {
    fn new(label: &str, runtime: RuntimeHandle) -> Self {
        Self {
            inner: Rc::new(InfiniteTransitionInner {
                label: label.to_string(),
                animations: RefCell::new(Vec::new()),
                run_token: OwnedMutableState::with_runtime(0u64, runtime.clone()),
                restart_pending: Cell::new(false),
                runtime,
            }),
        }
    }

    pub fn label(&self) -> &str {
        &self.inner.label
    }

    fn run(&self) {
        let run_key = self.inner.run_token.get();
        cranpose_core::label_next_ui_task(format!("loop {}", self.inner.label));
        let weak: Weak<InfiniteTransitionInner> = Rc::downgrade(&self.inner);
        cranpose_core::LaunchedEffectAsync!(run_key, move |scope| {
            Box::pin(async move {
                let clock = scope.runtime().frame_clock();
                let mut start_time: Option<u64> = None;

                loop {
                    if !scope.is_active() {
                        break;
                    }

                    let Some(inner) = weak.upgrade() else {
                        break;
                    };
                    inner.restart_pending.set(false);

                    if inner.animations.borrow().is_empty() || !inner.has_subscribers() {
                        break;
                    }

                    let now = clock.next_perpetual_frame().await;
                    if !scope.is_active() {
                        break;
                    }

                    let start = start_time.get_or_insert(now);
                    let play_time = now.saturating_sub(*start);
                    inner.on_frame(play_time);
                }
            })
        });
    }

    #[allow(non_snake_case)]
    pub fn animateFloat(
        &self,
        initial_value: f32,
        target_value: f32,
        animation_spec: InfiniteRepeatableSpec<f32>,
        label: &str,
    ) -> State<f32> {
        let _ = label;
        self.animateValue(initial_value, target_value, animation_spec)
    }

    #[allow(non_snake_case)]
    pub fn animateValue<T: Lerp + Clone + PartialEq + 'static>(
        &self,
        initial_value: T,
        target_value: T,
        animation_spec: InfiniteRepeatableSpec<T>,
    ) -> State<T> {
        let runtime = with_current_composer(|composer| composer.runtime_handle());
        let initial_for_remember = initial_value.clone();
        let target_for_remember = target_value.clone();
        let spec_for_remember = animation_spec.clone();
        let animation_state = cranpose_core::remember(move || {
            Rc::new(TransitionAnimationState::new(
                initial_for_remember,
                target_for_remember,
                spec_for_remember,
                runtime.clone(),
            ))
        })
        .with(Rc::clone);

        let animation_state_for_effect = Rc::clone(&animation_state);
        let spec_for_effect = animation_spec;
        SideEffect(move || {
            animation_state_for_effect.update_values(
                initial_value.clone(),
                target_value.clone(),
                spec_for_effect,
            );
        });

        let animation_any: Rc<dyn InfiniteTransitionAnimation> = animation_state.clone();
        let transition_inner = Rc::clone(&self.inner);
        let transition_for_subscriber = Rc::downgrade(&transition_inner);
        animation_state.install_subscriber_callback(Rc::new(move || {
            if let Some(transition) = transition_for_subscriber.upgrade() {
                transition.request_restart();
            }
        }));
        let animation_id = Rc::as_ptr(&animation_state) as usize;
        cranpose_core::DisposableEffect!(animation_id, move |_scope| {
            transition_inner.add_animation(animation_any.clone());
            let transition_inner = Rc::clone(&transition_inner);
            let animation_any = animation_any.clone();
            DisposableEffectResult::new(move || {
                transition_inner.remove_animation(&animation_any);
            })
        });

        animation_state.state()
    }
}

impl InfiniteTransitionInner {
    fn add_animation(&self, animation: Rc<dyn InfiniteTransitionAnimation>) {
        let mut list = self.animations.borrow_mut();
        let was_empty = list.is_empty();
        let already_present = list.iter().any(|item| Rc::ptr_eq(item, &animation));
        if !already_present {
            list.push(animation);
        }
        if was_empty && !list.is_empty() {
            self.run_token
                .update(|value| *value = value.wrapping_add(1));
        }
    }

    fn remove_animation(&self, animation: &Rc<dyn InfiniteTransitionAnimation>) {
        let mut list = self.animations.borrow_mut();
        let was_empty = list.is_empty();
        if let Some(index) = list.iter().position(|item| Rc::ptr_eq(item, animation)) {
            list.remove(index);
        }
        let is_empty = list.is_empty();
        drop(list);

        if !was_empty && is_empty {
            self.run_token
                .update(|value| *value = value.wrapping_add(1));
        }
    }

    fn on_frame(&self, play_time_nanos: u64) {
        let animations = self.animations.borrow().clone();
        for animation in animations {
            animation.on_frame(play_time_nanos);
        }
    }

    fn has_subscribers(&self) -> bool {
        self.animations
            .borrow()
            .iter()
            .any(|animation| animation.has_subscribers())
    }

    fn request_restart(self: Rc<Self>) {
        if self.restart_pending.replace(true) {
            return;
        }
        let runtime = self.runtime.clone();
        runtime.enqueue_ui_task(Box::new(move || {
            self.run_token
                .update(|value| *value = value.wrapping_add(1));
        }));
    }
}

#[allow(non_snake_case)]
pub fn rememberInfiniteTransition(label: &str) -> InfiniteTransition {
    let runtime = with_current_composer(|composer| composer.runtime_handle());
    let transition =
        cranpose_core::remember(move || InfiniteTransition::new(label, runtime.clone()))
            .with(|transition| transition.clone());
    transition.run();
    transition
}

/// Generic animatable value holder.
pub struct Animatable<T: SpringScalar + 'static> {
    inner: Rc<RefCell<AnimatableInner<T>>>,
}

struct AnimatableInner<T: SpringScalar + 'static> {
    state: OwnedMutableState<T>,
    runtime: RuntimeHandle,
    current: T,
    /// Per-dimension velocity in value units per second. Preserved across
    /// retargets so interrupted springs keep their physical motion.
    velocity: [f32; SPRING_MAX_DIMENSIONS],
    start: T,
    target: T,
    animation_type: AnimationType,
    start_time_nanos: Option<u64>,
    /// Previous spring frame timestamp; springs integrate the inter-frame
    /// delta (tweens use `start_time_nanos` progress instead).
    last_frame_nanos: Option<u64>,
    registration: Option<FrameCallbackRegistration>,
}

impl<T: SpringScalar + 'static> Animatable<T> {
    /// Create a new animatable with the given initial value.
    pub fn new(initial: T, runtime: RuntimeHandle) -> Self {
        let inner = AnimatableInner {
            state: OwnedMutableState::with_runtime(initial.clone(), runtime.clone()),
            runtime,
            current: initial.clone(),
            velocity: [0.0; SPRING_MAX_DIMENSIONS],
            start: initial.clone(),
            target: initial,
            animation_type: AnimationType::default(),
            start_time_nanos: None,
            last_frame_nanos: None,
            registration: None,
        };
        Self {
            inner: Rc::new(RefCell::new(inner)),
        }
    }

    /// Animate to the target value using the specified animation.
    ///
    /// Retargeting mid-flight keeps the in-flight velocity (springs continue
    /// their physical motion toward the new target).
    pub fn animateTo(&mut self, target: T, animation: AnimationType) {
        self.start_animation(target, animation, None);
    }

    fn start_animation(
        &mut self,
        target: T,
        animation: AnimationType,
        exact_start_time_nanos: Option<u64>,
    ) {
        {
            let mut inner = self.inner.borrow_mut();
            let previous_animation = inner.animation_type;

            // Cancel existing animation
            if let Some(registration) = inner.registration.take() {
                registration.cancel();
            }

            inner.start = inner.current.clone();
            inner.target = target;
            inner.animation_type = animation;
            inner.start_time_nanos = exact_start_time_nanos;
            match animation {
                AnimationType::Spring(spec) => {
                    let continues_spring = matches!(previous_animation, AnimationType::Spring(_));
                    if let Some(start_time_nanos) = exact_start_time_nanos {
                        let delay_nanos = spec.delay_millis.saturating_mul(1_000_000);
                        inner.last_frame_nanos = Some(start_time_nanos.saturating_add(delay_nanos));
                    } else if !continues_spring {
                        inner.last_frame_nanos = None;
                    }
                }
                AnimationType::Tween(_) => {
                    inner.last_frame_nanos = None;
                    inner.velocity = [0.0; SPRING_MAX_DIMENSIONS];
                }
            }
            // The spring frame chain (`last_frame_nanos`) survives a
            // retarget: a mid-flight spring keeps integrating real frame
            // deltas toward the new target. Clearing it made the first
            // frame after every retarget a dt=0 clock-set — under
            // continuous per-move retargeting (gesture tracking) that
            // starved the spring to a standstill. Tweens read only
            // `start_time_nanos`, which does reset. A settled or fresh
            // animatable enters with `last_frame_nanos == None` anyway.
        }

        Self::schedule_frame(&self.inner);
    }

    /// Animate to `target`, seeding the spring with `velocity` (value units
    /// per second, per dimension) — the gesture-handoff entry point: pass the
    /// release velocity so the animation continues the finger's motion.
    pub fn animate_to_with_velocity(&mut self, target: T, velocity: T, animation: AnimationType) {
        {
            let mut inner = self.inner.borrow_mut();
            for index in 0..T::DIMENSIONS.min(SPRING_MAX_DIMENSIONS) {
                inner.velocity[index] = velocity.dimension(index);
            }
        }
        self.animateTo(target, animation);
    }

    /// Animate from an exact point on the shared frame clock. The first
    /// rendered spring sample integrates every elapsed nanosecond since this
    /// boundary, so input-to-animation handoff is independent of which vsync
    /// first services the callback.
    pub fn animate_to_with_velocity_at(
        &mut self,
        target: T,
        velocity: T,
        animation: AnimationType,
        start_time_nanos: u64,
    ) {
        {
            let mut inner = self.inner.borrow_mut();
            for index in 0..T::DIMENSIONS.min(SPRING_MAX_DIMENSIONS) {
                inner.velocity[index] = velocity.dimension(index);
            }
        }
        self.start_animation(target, animation, Some(start_time_nanos));
    }

    /// The current velocity in value units per second (zero when settled).
    pub fn velocity(&self) -> T {
        T::from_dimensions(self.inner.borrow().velocity)
    }

    /// Return the current animation target.
    pub fn target(&self) -> T {
        self.inner.borrow().target.clone()
    }

    /// Return the animation spec currently driving this animatable.
    pub fn animation_type(&self) -> AnimationType {
        self.inner.borrow().animation_type
    }

    /// Get the current state.
    pub fn state(&self) -> State<T> {
        self.inner.borrow().state.as_state()
    }

    /// Snap immediately to the target value without animating.
    pub fn snapTo(&mut self, target: T) {
        let mut inner = self.inner.borrow_mut();
        if let Some(registration) = inner.registration.take() {
            registration.cancel();
        }
        inner.current = target.clone();
        inner.start = target.clone();
        inner.target = target.clone();
        inner.start_time_nanos = None;
        inner.last_frame_nanos = None;
        inner.velocity = [0.0; SPRING_MAX_DIMENSIONS];
        inner.state.set_value(target);
    }

    fn schedule_frame(this: &Rc<RefCell<AnimatableInner<T>>>) {
        let runtime = {
            let inner = this.borrow();
            if inner.registration.is_some() {
                return;
            }
            inner.runtime.clone()
        };
        let weak = Rc::downgrade(this);
        let registration = runtime.frame_clock().with_frame_nanos(move |time| {
            if let Some(strong) = weak.upgrade() {
                Self::on_frame(&strong, time);
            }
        });
        this.borrow_mut().registration = Some(registration);
    }

    fn on_frame(this: &Rc<RefCell<AnimatableInner<T>>>, frame_time_nanos: u64) {
        let mut schedule_next = false;
        {
            let mut inner = this.borrow_mut();
            inner.registration = None;

            match inner.animation_type {
                AnimationType::Tween(spec) => {
                    let start_time = inner.start_time_nanos.get_or_insert(frame_time_nanos);
                    let elapsed_nanos = frame_time_nanos.saturating_sub(*start_time);
                    let delay_nanos = spec.delay_millis.saturating_mul(1_000_000);

                    if elapsed_nanos < delay_nanos {
                        schedule_next = true;
                    } else {
                        let animation_elapsed = elapsed_nanos - delay_nanos;
                        let duration_nanos = spec.duration_millis * 1_000_000;
                        let duration_nanos = duration_nanos.max(1);
                        let linear_progress =
                            (animation_elapsed as f32 / duration_nanos as f32).clamp(0.0, 1.0);
                        let progress = spec.easing.transform(linear_progress);

                        let new_value = inner.start.lerp(&inner.target, progress);
                        inner.current = new_value.clone();
                        inner.state.set_value(new_value);

                        if linear_progress >= 1.0 {
                            inner.current = inner.target.clone();
                            inner.start = inner.target.clone();
                            inner.start_time_nanos = None;
                            inner.state.set_value(inner.target.clone());
                        } else {
                            schedule_next = true;
                        }
                    }
                }
                AnimationType::Spring(spec) => {
                    let start_time = inner.start_time_nanos.get_or_insert(frame_time_nanos);
                    let elapsed_nanos = frame_time_nanos.saturating_sub(*start_time);
                    let delay_nanos = spec.delay_millis.saturating_mul(1_000_000);
                    if elapsed_nanos < delay_nanos {
                        inner.last_frame_nanos = Some(start_time.saturating_add(delay_nanos));
                        schedule_next = true;
                    } else {
                        // Damped harmonic oscillator advanced per dimension in
                        // VALUE space using the closed-form solution (exact for
                        // any frame delta — no integration drift at low frame
                        // rates). Velocity carries across frames and retargets.
                        let last = inner.last_frame_nanos.replace(frame_time_nanos);
                        let dt = last
                            .map(|last| {
                                frame_time_nanos.saturating_sub(last) as f32 / 1_000_000_000.0
                            })
                            .unwrap_or(0.0);

                        if dt <= 0.0 {
                            schedule_next = true;
                        } else {
                            let dimensions = T::DIMENSIONS.min(SPRING_MAX_DIMENSIONS);
                            let mut position = [0.0f32; SPRING_MAX_DIMENSIONS];
                            for (index, slot) in position.iter_mut().enumerate().take(dimensions) {
                                let value = inner.current.dimension(index);
                                let target = inner.target.dimension(index);
                                let (next_value, next_velocity) = advance_spring(
                                    value,
                                    inner.velocity[index],
                                    target,
                                    spec.damping_ratio,
                                    spec.stiffness,
                                    dt,
                                );
                                *slot = next_value;
                                inner.velocity[index] = next_velocity;
                            }

                            inner.current = T::from_dimensions(position);
                            inner.state.set_value(inner.current.clone());

                            // Settled when every dimension is at rest near the target.
                            let settled = (0..dimensions).all(|index| {
                                inner.velocity[index].abs() < spec.velocity_threshold
                                    && (position[index] - inner.target.dimension(index)).abs()
                                        < spec.position_threshold
                            });

                            if settled {
                                inner.current = inner.target.clone();
                                inner.start = inner.target.clone();
                                inner.start_time_nanos = None;
                                inner.last_frame_nanos = None;
                                inner.velocity = [0.0; SPRING_MAX_DIMENSIONS];
                                inner.state.set_value(inner.target.clone());
                            } else {
                                schedule_next = true;
                            }
                        }
                    }
                }
            }
        }

        if schedule_next {
            Self::schedule_frame(this);
        }
    }
}

/// [`animateFloatAsState`] with an explicit initial value: the first
/// composition seeds the animation at `initial` and animates toward `target`,
/// so newly appearing content can enter from 0 instead of snapping. The
/// building block for enter transitions (`Crossfade`, `AnimatedVisibility`,
/// morphing popups).
pub fn animate_float_as_state_with_initial(
    initial: f32,
    target: f32,
    animation: AnimationType,
    label: &str,
) -> State<f32> {
    let _ = label;
    with_current_composer(|composer| {
        let runtime = composer.runtime_handle();
        let anim: Owned<Animatable<f32>> = composer.remember(|| Animatable::new(initial, runtime));
        anim.update(|animatable| {
            let is_new_target = (animatable.target() - target).abs() > f32::EPSILON;
            let is_new_animation = animatable.animation_type() != animation;
            if is_new_target || is_new_animation {
                animatable.animateTo(target, animation);
            }
        });
        anim.with(|animatable| animatable.state())
    })
}

#[allow(non_snake_case)]
pub fn animateFloatAsState(target: f32, animation: AnimationType, label: &str) -> State<f32> {
    let _ = label;
    with_current_composer(|composer| {
        let runtime = composer.runtime_handle();
        let anim: Owned<Animatable<f32>> = composer.remember(|| Animatable::new(target, runtime));
        anim.update(|animatable| {
            let is_new_target = (animatable.target() - target).abs() > f32::EPSILON;
            let is_new_animation = animatable.animation_type() != animation;
            if is_new_target || is_new_animation {
                animatable.animateTo(target, animation);
            }
        });
        anim.with(|animatable| animatable.state())
    })
}

impl<T: SpringScalar + 'static> Clone for Animatable<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

#[cfg(test)]
#[path = "tests/animation_tests.rs"]
mod tests;
