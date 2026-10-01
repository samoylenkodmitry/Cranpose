use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_animation::{Animatable, AnimationType, SpringSpec};
use cranpose_core::{RuntimeHandle, with_current_composer};
use cranpose_foundation::PointerId;
use cranpose_macros::composable;

const DRAG_THRESHOLD: f32 = 10.0;
const FLING_THRESHOLD: f32 = 100.0;
const PROJECTION_SECONDS: f32 = 0.099;

#[derive(Clone, Copy)]
pub(super) struct SliderDrag {
    pub(super) id: PointerId,
    pub(super) value: f32,
    origin_x: f32,
    origin_value: f32,
    threshold: Option<f32>,
    previous_x: f32,
    previous_time: Option<i64>,
    previous_velocity: f32,
    velocity: f32,
}

impl SliderDrag {
    pub(super) fn new(id: PointerId, x: f32, value: f32, time: Option<i64>) -> Self {
        Self {
            id,
            value,
            origin_x: x,
            origin_value: value,
            threshold: None,
            previous_x: x,
            previous_time: time,
            previous_velocity: 0.0,
            velocity: 0.0,
        }
    }

    pub(super) fn fraction(&mut self, x: f32, width: f32) -> f32 {
        let distance = x - self.origin_x;
        if self.threshold.is_none() && distance.abs() > DRAG_THRESHOLD {
            self.threshold = Some(DRAG_THRESHOLD.copysign(distance));
        }
        if let Some(threshold) = self.threshold {
            self.value = (self.origin_value + (distance - threshold) / width).clamp(0.0, 1.0);
        }
        self.value
    }

    pub(super) fn moved(&mut self, x: f32, time: Option<i64>) {
        if let (Some(previous), Some(now)) = (self.previous_time, time) {
            let elapsed = now - previous;
            if elapsed > 0 {
                let velocity = (x - self.previous_x) * 1000.0 / elapsed as f32;
                self.velocity = if elapsed <= 100 {
                    0.8 * self.previous_velocity + 0.2 * velocity
                } else {
                    0.0
                };
                self.previous_velocity = velocity;
            }
        }
        self.previous_x = x;
        self.previous_time = time;
    }

    pub(super) fn release_velocity(&self) -> f32 {
        if self.threshold.is_some() {
            self.velocity
        } else {
            0.0
        }
    }
}

pub(super) struct SliderFling {
    animation: RefCell<Animatable<f32>>,
    active: Cell<bool>,
    controlled: Cell<f32>,
    reported: Cell<f32>,
    runtime: RuntimeHandle,
}

impl SliderFling {
    fn new(value: f32, runtime: RuntimeHandle) -> Self {
        Self {
            animation: RefCell::new(Animatable::new(value, runtime.clone())),
            active: Cell::new(false),
            controlled: Cell::new(value),
            reported: Cell::new(value),
            runtime,
        }
    }

    pub(super) fn synchronize(&self, value: f32) {
        if self.controlled.replace(value) != value
            && self.active.get()
            && self.reported.get() != value
        {
            self.cancel();
        }
    }

    pub(super) fn cancel(&self) {
        if self.active.replace(false) {
            let mut animation = self.animation.borrow_mut();
            let value = animation.state().get();
            animation.snapTo(value);
        }
    }

    pub(super) fn release(&self, value: f32, velocity: f32, width: f32, time: Option<u64>) {
        self.cancel();
        if velocity.abs() < FLING_THRESHOLD {
            return;
        }
        let velocity = velocity / width;
        let target = (value + velocity * PROJECTION_SECONDS).clamp(0.0, 1.0);
        if target == value {
            return;
        }
        self.active.set(true);
        self.reported.set(value);
        let mut animation = self.animation.borrow_mut();
        animation.snapTo(value);
        let spec = AnimationType::Spring(SpringSpec {
            damping_ratio: 1.0,
            stiffness: 25.0 * std::f32::consts::PI * std::f32::consts::PI,
            position_threshold: 0.001,
            velocity_threshold: 0.01,
            delay_millis: 20,
        });
        if let Some(time) = time.or_else(|| self.runtime.last_frame_time_nanos()) {
            animation.animate_to_with_velocity_at(target, velocity, spec, time);
        } else {
            animation.animate_to_with_velocity(target, velocity, spec);
        }
    }

    pub(super) fn sample(&self) -> Option<f32> {
        let value = self.animation.borrow().state().get().clamp(0.0, 1.0);
        self.active.get().then_some(value)
    }

    pub(super) fn publish(&self, value: f32, on_change: &dyn Fn(f32)) {
        if !self.active.get() {
            return;
        }
        let finished = !self.animation.borrow().is_running();
        if self.reported.replace(value) != value {
            on_change(value);
        }
        if finished {
            self.active.set(false);
            self.animation.borrow_mut().snapTo(self.controlled.get());
        }
    }
}

#[composable]
pub(super) fn remember_slider_fling(value: f32) -> Rc<SliderFling> {
    with_current_composer(|composer| {
        let runtime = composer.runtime_handle();
        composer
            .remember(move || Rc::new(SliderFling::new(value, runtime)))
            .with(Rc::clone)
    })
}

#[cfg(test)]
#[path = "tests/slider_motion_tests.rs"]
mod tests;
