//! Liquid motion: the spring presets every component shares, and the press
//! interaction (scale + specular boost) that makes glass feel physical.

use cranpose_animation::{spring, tween, Animatable, AnimationType, Easing};
use cranpose_core::{with_current_composer, RuntimeHandle, State};
use cranpose_foundation::VelocityTracker1D;
use cranpose_macros::composable;
use cranpose_ui::Modifier;
use cranpose_ui::MutableInteractionSource;
use cranpose_ui_graphics::GraphicsLayer;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::dynamics::{LiquidDynamics, LiquidPose};

const FLUID_RELAX_MS: u64 = 420;

/// Named springs used across the Liquid components (value-space, velocity
/// preserving).
pub struct LiquidMotion;

impl LiquidMotion {
    /// Snappy interactions: presses, toggles, selection moves.
    pub fn snappy() -> AnimationType {
        spring(0.85, 900.0)
    }

    /// The droplet feel: visible overshoot for morphing shapes.
    pub fn bouncy() -> AnimationType {
        spring(0.55, 500.0)
    }

    /// Gentle settle for large surfaces (sheets, menus).
    pub fn smooth() -> AnimationType {
        spring(1.0, 400.0)
    }

    /// The leading edge of a stretching selection blob (runs ahead).
    pub fn blob_leading() -> AnimationType {
        spring(0.8, 900.0)
    }

    /// The trailing edge of a stretching selection blob (drags behind, giving
    /// the droplet elongation while in motion).
    pub fn blob_trailing() -> AnimationType {
        spring(0.9, 380.0)
    }

    /// A released lens flying to its committed slot: the reference tab-bar
    /// transit crosses ~3 cells in ~330 ms (measured from the iphone17
    /// recording) — much gentler than the finger-chase spring.
    pub fn glide() -> AnimationType {
        spring(1.0, 120.0)
    }
}

/// One travelling coordinate with strict direct-manipulation semantics.
/// Pointer samples are visible immediately; the animation channel is used
/// only after release or for controlled-state changes while idle.
pub(crate) struct LiquidDragAxis {
    animation: RefCell<Animatable<f32>>,
    pointer: Cell<Option<f32>>,
    velocity: RefCell<VelocityTracker1D>,
    runtime: RuntimeHandle,
    last_sample_ms: Cell<Option<i64>>,
    dynamics: LiquidDynamics,
    fluid_clock: RefCell<Animatable<f32>>,
}

impl LiquidDragAxis {
    fn new(initial: f32, runtime: RuntimeHandle) -> Self {
        Self {
            animation: RefCell::new(Animatable::new(initial, runtime.clone())),
            pointer: Cell::new(None),
            velocity: RefCell::new(VelocityTracker1D::new()),
            dynamics: LiquidDynamics::new(runtime.clone()),
            fluid_clock: RefCell::new(Animatable::new(1.0, runtime.clone())),
            runtime,
            last_sample_ms: Cell::new(None),
        }
    }

    fn arm_fluid_frames(&self) {
        let mut clock = self.fluid_clock.borrow_mut();
        clock.snapTo(0.0);
        clock.animateTo(1.0, tween(FLUID_RELAX_MS, Easing::LinearEasing));
    }

    fn sample_time_ms(&self, event_time_ms: Option<i64>) -> i64 {
        let candidate = event_time_ms
            .or_else(|| {
                self.runtime
                    .last_frame_time_nanos()
                    .map(|nanos| (nanos / 1_000_000) as i64)
            })
            .unwrap_or_else(|| self.last_sample_ms.get().unwrap_or(0) + 16);
        let monotonic = self
            .last_sample_ms
            .get()
            .map_or(candidate, |last| candidate.max(last + 1));
        self.last_sample_ms.set(Some(monotonic));
        monotonic
    }

    pub(crate) fn begin(&self, position: f32, event_time_ms: Option<i64>) {
        let time_ms = self.sample_time_ms(event_time_ms);
        let mut velocity = self.velocity.borrow_mut();
        velocity.reset();
        velocity.add_data_point(time_ms, position);
        self.pointer.set(Some(position));
        self.animation.borrow_mut().snapTo(position);
        self.dynamics.anchor_pointer((position, 0.0));
        self.arm_fluid_frames();
    }

    pub(crate) fn move_to(&self, position: f32, event_time_ms: Option<i64>) {
        if self.pointer.get().is_none() {
            return;
        }
        let previous_time_ms = self.last_sample_ms.get();
        let time_ms = self.sample_time_ms(event_time_ms);
        self.velocity.borrow_mut().add_data_point(time_ms, position);
        self.pointer.set(Some(position));
        self.animation.borrow_mut().snapTo(position);
        if let Some(previous_time_ms) = previous_time_ms {
            let dt = (time_ms - previous_time_ms).max(1) as f32 / 1000.0;
            self.dynamics.advance_pointer((position, 0.0), dt);
        }
        self.arm_fluid_frames();
    }

    pub(crate) fn release_to(
        &self,
        target: f32,
        event_time_ms: Option<i64>,
        animation: AnimationType,
    ) {
        let Some(position) = self.pointer.take() else {
            self.settle_to(target, animation);
            return;
        };
        let time_ms = self.sample_time_ms(event_time_ms);
        self.velocity.borrow_mut().add_data_point(time_ms, position);
        let release_velocity = self.velocity.borrow().calculate_velocity_with_max(8_000.0);
        self.dynamics.release_pointer();
        self.animation
            .borrow_mut()
            .animate_to_with_velocity(target, release_velocity, animation);
    }

    pub(crate) fn settle_to(&self, target: f32, animation: AnimationType) {
        if self.pointer.get().is_some() {
            return;
        }
        let mut value = self.animation.borrow_mut();
        if (value.target() - target).abs() > f32::EPSILON {
            value.animateTo(target, animation);
        }
    }

    pub(crate) fn value(&self) -> f32 {
        let _ = self.fluid_clock.borrow().state().value();
        self.pointer
            .get()
            .unwrap_or_else(|| self.animation.borrow().state().value())
    }

    pub(crate) fn liquid_pose(&self) -> LiquidPose {
        self.dynamics.update_pointer((self.value(), 0.0))
    }

    pub(crate) fn is_dragging(&self) -> bool {
        self.pointer.get().is_some()
    }
}

#[composable]
pub(crate) fn remember_liquid_drag_axis(initial: f32) -> Rc<LiquidDragAxis> {
    with_current_composer(|composer| {
        let runtime = composer.runtime_handle();
        composer
            .remember(move || Rc::new(LiquidDragAxis::new(initial, runtime)))
            .with(Rc::clone)
    })
}

/// Press feedback for glass controls, per the Liquid Glass law: touched glass
/// GROWS (spring scale toward `pressed_scale` — never smaller) and turns MORE
/// TRANSPARENT (the returned content alpha dips while pressed, the reference
/// "…" dots fading as the button lifts). Returns the pressed state so callers
/// can also boost the specular highlight.
///
/// Apply the returned modifier *outside* the glass effect so the whole lens
/// scales together; apply the content alpha to the label/icon layer.
#[composable]
pub fn liquid_press_scale(
    modifier: Modifier,
    interaction_source: MutableInteractionSource,
    pressed_scale: f32,
) -> (Modifier, State<bool>, State<f32>) {
    let pressed = interaction_source.collectIsPressedAsState();
    let scale = cranpose_animation::animateFloatAsState(
        if pressed.get() {
            pressed_scale.max(1.0)
        } else {
            1.0
        },
        LiquidMotion::snappy(),
        "liquid-press-scale",
    );
    let content_alpha = cranpose_animation::animateFloatAsState(
        // The reference down-state ghosts glyphs hard (the menu button's
        // dots drop to ~30% while held).
        if pressed.get() { 0.35 } else { 1.0 },
        LiquidMotion::smooth(),
        "liquid-press-content",
    );
    let modifier = modifier.graphics_layer(move || {
        let scale = scale.get();
        GraphicsLayer {
            scale_x: scale,
            scale_y: scale,
            ..Default::default()
        }
    });
    (modifier, pressed, content_alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axis(initial: f32) -> (cranpose_core::Runtime, LiquidDragAxis) {
        let runtime =
            cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
        let axis = LiquidDragAxis::new(initial, runtime.handle());
        (runtime, axis)
    }

    #[test]
    fn pointer_samples_are_the_visual_coordinate_without_a_chase() {
        let (_runtime, axis) = axis(10.0);
        axis.begin(20.0, Some(0));
        assert_eq!(axis.value(), 20.0);
        axis.move_to(180.0, Some(16));
        assert_eq!(axis.value(), 180.0);
    }

    #[test]
    fn pointer_sample_excites_the_incompressible_pose_before_render() {
        let (_runtime, axis) = axis(0.0);
        axis.begin(0.0, Some(0));
        axis.move_to(14.0, Some(16));
        let pose = axis.liquid_pose();
        let deformation = (pose.stretch - 1.0).abs();
        assert!(
            (0.03..=0.08).contains(&deformation),
            "the direct-input frame must deform visibly without treating one sample as extreme acceleration: {pose:?}"
        );
        assert!((pose.stretch * pose.ortho - 1.0).abs() < 1e-4);
        assert_eq!(axis.value(), 14.0);
    }

    #[test]
    fn render_without_a_new_pointer_sample_preserves_velocity_continuity() {
        let (_runtime, axis) = axis(0.0);
        axis.runtime.drain_frame_callbacks(1_000_000);
        axis.begin(0.0, Some(0));
        axis.move_to(14.0, Some(16));
        let sampled = axis.liquid_pose();

        axis.runtime.drain_frame_callbacks(17_000_000);
        let next_frame = axis.liquid_pose();

        assert!(
            (next_frame.stretch - sampled.stretch).abs() < 0.08,
            "a render frame without input must not synthesize a brake impulse: {sampled:?} -> {next_frame:?}"
        );
        assert!((next_frame.stretch * next_frame.ortho - 1.0).abs() < 1e-4);
    }

    #[test]
    fn controlled_retargets_wait_until_direct_manipulation_ends() {
        let (_runtime, axis) = axis(10.0);
        axis.begin(40.0, Some(0));
        axis.settle_to(90.0, LiquidMotion::snappy());
        assert_eq!(axis.value(), 40.0);
        axis.release_to(90.0, Some(16), LiquidMotion::snappy());
        assert!(!axis.is_dragging());
        assert_eq!(axis.animation.borrow().target(), 90.0);
    }

    #[test]
    fn released_flight_is_critically_damped() {
        let AnimationType::Spring(spec) = LiquidMotion::glide() else {
            panic!("released flight must use a spring");
        };
        assert_eq!(spec.damping_ratio, 1.0);
        assert_eq!(spec.stiffness, 120.0);
    }
}
