use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_animation::{Animatable, AnimationType, spring};
use cranpose_core::{RuntimeHandle, State, with_current_composer};
use cranpose_macros::composable;
use cranpose_ui_graphics::Size;

const STRAIN_LIMIT: f32 = 0.35;
pub(super) const MAX_LENS_PROJECTION: f32 = 1.0 + STRAIN_LIMIT;

pub(super) fn tab_lens_activity_motion(raised: bool) -> AnimationType {
    if raised {
        spring(1.0, 600.0)
    } else {
        spring(1.0, 650.0).with_delay(8)
    }
}

pub(super) struct TabContactMotion {
    runtime: RuntimeHandle,
    bar: RefCell<Animatable<f32>>,
    lens: RefCell<Animatable<f32>>,
    glow: RefCell<Animatable<f32>>,
    local_glow_factor: RefCell<Animatable<f32>>,
}

impl TabContactMotion {
    fn new(runtime: RuntimeHandle) -> Self {
        Self {
            bar: RefCell::new(Animatable::new(0.0, runtime.clone())),
            lens: RefCell::new(Animatable::new(0.0, runtime.clone())),
            glow: RefCell::new(Animatable::new(0.0, runtime.clone())),
            local_glow_factor: RefCell::new(Animatable::new(1.0, runtime.clone())),
            runtime,
        }
    }

    fn retarget(
        &self,
        value: &RefCell<Animatable<f32>>,
        target: f32,
        animation: AnimationType,
        event_time: Option<u64>,
    ) {
        let mut value = value.borrow_mut();
        if value.target() == target {
            return;
        }
        if let Some(time) = event_time.or_else(|| self.runtime.last_frame_time_nanos()) {
            value.animate_to_at(target, animation, time);
        } else {
            value.animateTo(target, animation);
        }
    }

    pub(super) fn pressed(&self, down: bool, event_time: Option<u64>) {
        if down {
            self.local_glow_factor.borrow_mut().snapTo(1.0);
        }
        self.retarget(
            &self.glow,
            f32::from(down),
            spring(1.0, if down { 4000.0 } else { 160.0 }),
            event_time,
        );
        self.retarget(
            &self.bar,
            f32::from(down),
            spring(0.65, 500.0).with_delay(16),
            event_time,
        );
        self.retarget(
            &self.lens,
            f32::from(down),
            tab_lens_activity_motion(down),
            event_time,
        );
    }

    pub(super) fn glow_state(&self) -> State<f32> {
        self.glow.borrow().state()
    }

    pub(super) fn local_glow_factor_state(&self) -> State<f32> {
        self.local_glow_factor.borrow().state()
    }

    pub(super) fn crossed_tab(&self) {
        self.retarget(&self.local_glow_factor, 0.5, spring(1.0, 160.0), None);
    }

    pub(super) fn bar_state(&self) -> State<f32> {
        self.bar.borrow().state()
    }

    pub(super) fn lens_state(&self) -> State<f32> {
        self.lens.borrow().state()
    }
}

#[composable]
pub(super) fn remember_tab_contact_motion() -> Rc<TabContactMotion> {
    with_current_composer(|composer| {
        let runtime = composer.runtime_handle();
        composer
            .remember(move || Rc::new(TabContactMotion::new(runtime)))
            .with(Rc::clone)
    })
}

struct ShapeParameters {
    damping: f32,
    stiffness: f32,
    relaxation_rate: f32,
    viscous_response: f32,
    inertial_response: f32,
}

struct ShapeResponse {
    relaxed_speed: Cell<f32>,
    strain: RefCell<Animatable<f32>>,
    relaxation_rate: f32,
    viscous_response: f32,
    inertial_response: f32,
    animation: AnimationType,
}

impl ShapeResponse {
    fn new(runtime: RuntimeHandle, parameters: ShapeParameters) -> Self {
        let ShapeParameters {
            damping,
            stiffness,
            relaxation_rate,
            viscous_response,
            inertial_response,
        } = parameters;
        Self {
            relaxed_speed: Cell::new(0.0),
            strain: RefCell::new(Animatable::new(0.0, runtime)),
            relaxation_rate,
            viscous_response,
            inertial_response,
            animation: AnimationType::Spring(cranpose_animation::SpringSpec {
                damping_ratio: damping,
                stiffness,
                position_threshold: 0.000001,
                velocity_threshold: 0.00001,
                ..Default::default()
            }),
        }
    }

    fn advance(&self, speed: f32, dt: f32, now: u64) {
        let relaxed =
            speed + (self.relaxed_speed.get() - speed) * (-self.relaxation_rate * dt).exp();
        let relaxed = if speed == 0.0 && relaxed < 0.0001 {
            0.0
        } else {
            relaxed
        };
        self.relaxed_speed.set(relaxed);
        let acceleration = self.relaxation_rate * (speed - relaxed);
        let target = self.viscous_response * relaxed + self.inertial_response * acceleration;
        let mut strain = self.strain.borrow_mut();
        if (strain.target() - target).abs() > 0.0000001 || (target == 0.0 && strain.target() != 0.0)
        {
            strain.animate_to_at(target, self.animation, now);
        }
    }

    fn value(&self) -> f32 {
        self.strain.borrow().state().value()
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum LensShapeKind {
    Tab,
    Thumb,
    Segment,
}

pub(super) struct LensShapeMotion {
    runtime: RuntimeHandle,
    previous: Cell<Option<(u64, f32)>>,
    width: ShapeResponse,
    height: Option<ShapeResponse>,
    height_factor: f32,
}

impl LensShapeMotion {
    fn new(runtime: RuntimeHandle, kind: LensShapeKind) -> Self {
        Self {
            width: ShapeResponse::new(
                runtime.clone(),
                if kind == LensShapeKind::Tab {
                    ShapeParameters {
                        damping: 0.430087,
                        stiffness: 164.807,
                        relaxation_rate: 6.059487,
                        viscous_response: 0.00001663016,
                        inertial_response: 0.00004602218,
                    }
                } else {
                    ShapeParameters {
                        damping: 0.24,
                        stiffness: 140.0,
                        relaxation_rate: 3.7,
                        viscous_response: 0.0,
                        inertial_response: 0.000064,
                    }
                },
            ),
            height: (kind == LensShapeKind::Tab).then(|| {
                ShapeResponse::new(
                    runtime.clone(),
                    ShapeParameters {
                        damping: 0.38644,
                        stiffness: 198.37253,
                        relaxation_rate: 3.179867,
                        viscous_response: -0.00008953011,
                        inertial_response: -0.00008536202,
                    },
                )
            }),
            height_factor: if kind == LensShapeKind::Segment {
                -1.2
            } else {
                -1.02
            },
            previous: Cell::new(None),
            runtime,
        }
    }

    pub(super) fn sample(&self, position: f32) -> Size {
        let width = self.width.value();
        let value = Size::new(
            width,
            self.height
                .as_ref()
                .map_or(width * self.height_factor, ShapeResponse::value),
        );
        let Some(now) = self.runtime.last_frame_time_nanos() else {
            return value;
        };
        let Some((last, previous_position)) = self.previous.get() else {
            self.previous.set(Some((now, position)));
            return value;
        };
        if now <= last {
            return value;
        }
        let dt = (now - last) as f32 / 1e9;
        let speed = ((position - previous_position) / dt).abs().min(8000.0);
        self.previous.set(Some((now, position)));
        self.width.advance(speed, dt, now);
        if let Some(height) = &self.height {
            height.advance(speed, dt, now);
        }
        value
    }

    pub(super) fn projection(&self, position: f32) -> (f32, f32) {
        let strain = self.sample(position);
        (
            1.0 + strain.width.clamp(-STRAIN_LIMIT, STRAIN_LIMIT),
            1.0 + strain.height.clamp(-STRAIN_LIMIT, STRAIN_LIMIT),
        )
    }
}

#[composable]
pub(super) fn remember_lens_shape(kind: LensShapeKind) -> Rc<LensShapeMotion> {
    with_current_composer(|composer| {
        let runtime = composer.runtime_handle();
        composer
            .remember(move || Rc::new(LensShapeMotion::new(runtime, kind)))
            .with(Rc::clone)
    })
}

#[cfg(test)]
#[path = "tests/lens_motion_tests.rs"]
mod tests;
