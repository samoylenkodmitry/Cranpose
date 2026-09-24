//! Liquid motion: the spring presets every component shares, and the press
//! interaction (scale + specular boost) that makes glass feel physical.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_animation::{Animatable, AnimationType, Easing, spring, tween};
use cranpose_core::{RuntimeHandle, State, with_current_composer};
use cranpose_foundation::VelocityTracker1D;
use cranpose_macros::composable;
use cranpose_ui::{Modifier, MutableInteractionSource};
use cranpose_ui_graphics::GraphicsLayer;

use crate::dynamics::{LiquidDynamics, LiquidPose};

const FLUID_RELAX_MS: u64 = 420;
const VISUAL_HANDOFF_TOLERANCE_IN_ITEMS: f32 = 0.12;

pub(crate) fn liquid_visual_index(
    selected: usize,
    lens_position: f32,
    item_width: f32,
    count: usize,
    lens_owns_selection: bool,
) -> usize {
    if count == 0 {
        return 0;
    }
    let selected = selected.min(count - 1);
    if !lens_owns_selection || !lens_position.is_finite() || item_width <= f32::EPSILON {
        return selected;
    }
    ((lens_position + item_width * 0.5) / item_width)
        .floor()
        .clamp(0.0, count.saturating_sub(1) as f32) as usize
}

pub(crate) fn liquid_axis_owns_visual_selection(
    direct: bool,
    lens_position: f32,
    state_position: f32,
    item_width: f32,
) -> bool {
    direct
        || (lens_position - state_position).abs()
            > item_width.max(f32::EPSILON) * VISUAL_HANDOFF_TOLERANCE_IN_ITEMS
}

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

    /// A released lens flying to its committed slot: the reference
    /// bottom-bar transfer arrives in ~170 ms (on-white-click sheet,
    /// f_0040 departure to f_0050 arrival at 60 fps) with no visible
    /// overshoot; the optical settle continues after the geometry lands.
    pub fn glide() -> AnimationType {
        spring(1.0, 500.0)
    }
}

pub(crate) struct LiquidDragAxis {
    animation: RefCell<Animatable<f32>>,
    follow: Option<AnimationType>,
    pointer: Cell<Option<f32>>,
    velocity: RefCell<VelocityTracker1D>,
    runtime: RuntimeHandle,
    last_sample_ms: Cell<Option<i64>>,
    dynamics: LiquidDynamics,
    fluid_clock: RefCell<Animatable<f32>>,
}

impl LiquidDragAxis {
    fn new(initial: f32, runtime: RuntimeHandle, follow: Option<AnimationType>) -> Self {
        Self {
            follow,
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
        self.move_to_at(
            position,
            event_time_ms,
            self.runtime.last_frame_time_nanos(),
        );
    }

    fn move_to_at(
        &self,
        position: f32,
        event_time_ms: Option<i64>,
        animation_time_nanos: Option<u64>,
    ) {
        if self.pointer.get().is_none() {
            return;
        }
        let previous_time_ms = self.last_sample_ms.get();
        let time_ms = self.sample_time_ms(event_time_ms);
        self.velocity.borrow_mut().add_data_point(time_ms, position);
        self.pointer.set(Some(position));
        if let Some(animation) = self.follow {
            let mut value = self.animation.borrow_mut();
            if let Some(time) = animation_time_nanos {
                value.animate_to_at(position, animation, time);
            } else {
                value.animateTo(position, animation);
            }
        } else {
            self.animation.borrow_mut().snapTo(position);
        }
        if let Some(previous_time_ms) = previous_time_ms.filter(|_| self.follow.is_none()) {
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
        self.release_to_at(
            target,
            event_time_ms,
            animation,
            self.runtime.last_frame_time_nanos(),
        );
    }

    fn release_to_at(
        &self,
        target: f32,
        event_time_ms: Option<i64>,
        animation: AnimationType,
        animation_time_nanos: Option<u64>,
    ) {
        let Some(position) = self.pointer.take() else {
            self.settle_to_at(target, animation, animation_time_nanos);
            return;
        };
        let time_ms = self.sample_time_ms(event_time_ms);
        self.velocity.borrow_mut().add_data_point(time_ms, position);
        let release_velocity = if self.follow.is_some() {
            self.animation.borrow().velocity()
        } else {
            self.velocity.borrow().calculate_velocity_with_max(8_000.0)
        };
        self.dynamics.release_pointer();
        let mut value = self.animation.borrow_mut();
        if let Some(time) = animation_time_nanos {
            if self.follow.is_some() {
                value.animate_to_at(target, animation, time);
            } else {
                value.animate_to_with_velocity_at(target, release_velocity, animation, time);
            }
        } else {
            value.animate_to_with_velocity(target, release_velocity, animation);
        }
    }

    pub(crate) fn finish_at(&self, position: f32, event_time_ms: Option<i64>) {
        let Some(_) = self.pointer.get() else {
            self.animation.borrow_mut().snapTo(position);
            return;
        };
        let previous_time_ms = self.last_sample_ms.get();
        let time_ms = self.sample_time_ms(event_time_ms);
        self.velocity.borrow_mut().add_data_point(time_ms, position);
        if let Some(previous_time_ms) = previous_time_ms {
            let dt = (time_ms - previous_time_ms).max(1) as f32 / 1000.0;
            self.dynamics.advance_pointer((position, 0.0), dt);
        }
        self.pointer.set(None);
        self.animation.borrow_mut().snapTo(position);
        self.dynamics.release_pointer();
        self.arm_fluid_frames();
    }

    pub(crate) fn settle_to(&self, target: f32, animation: AnimationType) {
        self.settle_to_at(target, animation, self.runtime.last_frame_time_nanos());
    }

    fn settle_to_at(&self, target: f32, animation: AnimationType, time: Option<u64>) {
        if self.pointer.get().is_some() {
            return;
        }
        let mut value = self.animation.borrow_mut();
        if (value.target() - target).abs() > f32::EPSILON {
            if let Some(time) = time {
                value.animate_to_at(target, animation, time);
            } else {
                value.animateTo(target, animation);
            }
        }
    }

    pub(crate) fn value(&self) -> f32 {
        let _ = self.fluid_clock.borrow().state().value();
        if self.follow.is_some() {
            self.animation.borrow().state().value()
        } else {
            self.pointer
                .get()
                .unwrap_or_else(|| self.animation.borrow().state().value())
        }
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
    remember_liquid_axis(initial, None)
}

#[composable]
pub(crate) fn remember_liquid_follow_axis(
    initial: f32,
    animation: AnimationType,
) -> Rc<LiquidDragAxis> {
    remember_liquid_axis(initial, Some(animation))
}

#[composable]
fn remember_liquid_axis(initial: f32, follow: Option<AnimationType>) -> Rc<LiquidDragAxis> {
    with_current_composer(|composer| {
        let runtime = composer.runtime_handle();
        composer
            .remember(move || Rc::new(LiquidDragAxis::new(initial, runtime, follow)))
            .with(Rc::clone)
    })
}

pub(crate) struct LiquidLensGesture {
    pub axis: Rc<LiquidDragAxis>,
    pub cell_width: f32,
    pub cell_offset: f32,
    pub count: usize,
    pub tap_slop: f32,
    pub drag_left: Rc<dyn Fn(f32) -> f32>,
    pub rest_left: Rc<dyn Fn(usize) -> f32>,
    pub selected: usize,
    pub on_pressed: Rc<dyn Fn(bool, Option<u64>)>,
    pub on_touch: Rc<dyn Fn(f32, f32)>,
    pub on_select: Rc<dyn Fn(usize)>,
}

impl LiquidLensGesture {
    fn commit_index(&self, position: f32) -> usize {
        (((position - self.cell_offset) / self.cell_width.max(1.0)).floor() as isize)
            .clamp(0, self.count.saturating_sub(1) as isize) as usize
    }
}

pub(crate) async fn liquid_lens_gesture(
    scope: cranpose_ui::PointerInputScope,
    gesture: LiquidLensGesture,
) {
    use cranpose_foundation::{PointerEventKind, PointerId};
    use cranpose_services::{HapticFeedback, default_haptics};

    scope
        .await_pointer_event_scope(|await_scope| async move {
            let mut down_x = 0.0f32;
            let mut moved = false;
            let mut active_pointer = Option::<PointerId>::None;
            loop {
                let event = await_scope.await_pointer_event().await;
                match event.kind {
                    PointerEventKind::Down if active_pointer.is_none() => {
                        active_pointer = Some(event.id);
                        down_x = event.position.x;
                        moved = false;
                        gesture.axis.settle_to_at(
                            (gesture.rest_left)(gesture.commit_index(down_x)),
                            LiquidMotion::glide(),
                            event.animation_time_nanos,
                        );
                        (gesture.on_pressed)(true, event.animation_time_nanos);
                        (gesture.on_touch)(event.position.x, event.position.y);
                        default_haptics().perform(HapticFeedback::Selection);
                        event.consume();
                    }
                    PointerEventKind::Move if active_pointer == Some(event.id) => {
                        moved |= (event.position.x - down_x).abs() > gesture.tap_slop;
                        if moved {
                            let target = (gesture.drag_left)(event.position.x);
                            if gesture.axis.is_dragging() {
                                gesture.axis.move_to_at(
                                    target,
                                    event.time_ms,
                                    event.animation_time_nanos,
                                );
                            } else if (gesture.axis.value() - target).abs()
                                <= gesture.cell_width * 0.6
                            {
                                gesture.axis.begin(gesture.axis.value(), event.time_ms);
                                gesture.axis.move_to_at(
                                    target,
                                    event.time_ms,
                                    event.animation_time_nanos,
                                );
                            } else {
                                gesture.axis.settle_to_at(
                                    target,
                                    LiquidMotion::glide(),
                                    event.animation_time_nanos,
                                );
                            }
                        }
                        (gesture.on_touch)(event.position.x, event.position.y);
                        event.consume();
                    }
                    PointerEventKind::Up if active_pointer == Some(event.id) => {
                        active_pointer = None;
                        (gesture.on_pressed)(false, event.animation_time_nanos);
                        let commit_x = if moved { event.position.x } else { down_x };
                        let index = gesture.commit_index(commit_x);
                        gesture.axis.release_to_at(
                            (gesture.rest_left)(index),
                            event.time_ms,
                            LiquidMotion::glide(),
                            event.animation_time_nanos,
                        );
                        default_haptics().perform(HapticFeedback::ImpactLight);
                        (gesture.on_select)(index);
                        event.consume();
                    }
                    PointerEventKind::Cancel if active_pointer == Some(event.id) => {
                        active_pointer = None;
                        (gesture.on_pressed)(false, event.animation_time_nanos);
                        gesture.axis.release_to_at(
                            (gesture.rest_left)(gesture.selected),
                            event.time_ms,
                            LiquidMotion::glide(),
                            event.animation_time_nanos,
                        );
                        event.consume();
                    }
                    _ => {}
                }
            }
        })
        .await;
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
#[path = "tests/motion_tests.rs"]
mod tests;
