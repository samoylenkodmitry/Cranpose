use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_animation::{Animatable, AnimationType, spring};
use cranpose_core::{RuntimeHandle, State, with_current_composer};
use cranpose_macros::composable;
use cranpose_ui_graphics::Size;

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

struct TabShapeParameters {
    damping: f32,
    stiffness: f32,
    relaxation_rate: f32,
    viscous_response: f32,
    inertial_response: f32,
}

struct TabShapeResponse {
    relaxed_speed: Cell<f32>,
    strain: RefCell<Animatable<f32>>,
    relaxation_rate: f32,
    viscous_response: f32,
    inertial_response: f32,
    animation: AnimationType,
}

impl TabShapeResponse {
    fn new(runtime: RuntimeHandle, parameters: TabShapeParameters) -> Self {
        let TabShapeParameters {
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

pub(super) struct TabLensShape {
    runtime: RuntimeHandle,
    previous: Cell<Option<(u64, f32)>>,
    width: TabShapeResponse,
    height: TabShapeResponse,
}

impl TabLensShape {
    fn new(runtime: RuntimeHandle) -> Self {
        Self {
            width: TabShapeResponse::new(
                runtime.clone(),
                TabShapeParameters {
                    damping: 0.430087,
                    stiffness: 164.807,
                    relaxation_rate: 6.059487,
                    viscous_response: 0.00001663016,
                    inertial_response: 0.00004602218,
                },
            ),
            height: TabShapeResponse::new(
                runtime.clone(),
                TabShapeParameters {
                    damping: 0.38644,
                    stiffness: 198.37253,
                    relaxation_rate: 3.179867,
                    viscous_response: -0.00008953011,
                    inertial_response: -0.00008536202,
                },
            ),
            previous: Cell::new(None),
            runtime,
        }
    }

    pub(super) fn sample(&self, position: f32) -> Size {
        let value = Size::new(self.width.value(), self.height.value());
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
        self.height.advance(speed, dt, now);
        value
    }
}

#[composable]
pub(super) fn remember_tab_lens_shape() -> Rc<TabLensShape> {
    with_current_composer(|composer| {
        let runtime = composer.runtime_handle();
        composer
            .remember(move || Rc::new(TabLensShape::new(runtime)))
            .with(Rc::clone)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reversing_motion_preserves_the_stretch_and_rebound() {
        let runtime =
            cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
        let forward = TabLensShape::new(runtime.handle());
        let reverse = TabLensShape::new(runtime.handle());
        let mut greatest = 0.0f32;
        for frame in 0..240 {
            runtime
                .handle()
                .drain_frame_callbacks(1_000_000_000 + frame * 8_333_333);
            let position = ((frame as f32 - 96.0) * 8.0).clamp(0.0, 285.5);
            let a = forward.sample(position);
            let b = reverse.sample(285.5 - position);
            greatest = greatest.max(a.width.abs());
            assert!(
                (a.width - b.width).abs() < 0.00001 && (a.height - b.height).abs() < 0.00001,
                "direction changed strain at frame {frame}: {a:?}, {b:?}"
            );
        }
        assert!(greatest > 0.05);
    }

    #[test]
    fn contact_light_tracks_native_onset_and_keeps_the_dimmed_release() {
        let runtime =
            cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
        let motion = TabContactMotion::new(runtime.handle());
        let mut route = usize::MAX;
        let mut error = 0.0;
        let mut samples = 0;
        for row in include_str!("../../tests/fixtures/native_tab_glow.csv")
            .lines()
            .skip(1)
        {
            let fields = row
                .split(',')
                .map(|v| v.parse::<f64>().unwrap())
                .collect::<Vec<_>>();
            let next = fields[0] as usize;
            let origin = (next as u64 + 1) * 10_000_000_000;
            if next != route {
                motion.glow.borrow_mut().snapTo(0.0);
                motion.pressed(true, Some(origin));
                route = next;
            }
            runtime
                .handle()
                .drain_frame_callbacks(origin + (fields[1] * 1e9) as u64);
            error += (f64::from(motion.glow_state().get()) - fields[2]).powi(2);
            samples += 1;
        }
        assert!(samples >= 50);
        assert!((error / f64::from(samples)).sqrt() < 0.03);
        motion.crossed_tab();
        for frame in 1..=120 {
            runtime
                .handle()
                .drain_frame_callbacks(40_250_000_000 + frame * 16_666_667);
        }
        assert_eq!(motion.local_glow_factor_state().get(), 0.5);
        motion.pressed(false, Some(42_250_000_000));
        runtime.handle().drain_frame_callbacks(42_350_000_000);
        assert!((motion.glow_state().get() - 0.64).abs() < 0.03);
        assert_eq!(motion.local_glow_factor_state().get(), 0.5);
    }

    #[test]
    fn contact_clock_integrates_from_input_and_keeps_release_velocity() {
        let runtime =
            cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
        let motion = TabContactMotion::new(runtime.handle());
        runtime.handle().drain_frame_callbacks(1_000_000_000);
        motion.pressed(true, Some(1_000_000_000));
        runtime.handle().drain_frame_callbacks(1_050_000_000);
        let before = motion.lens_state().value();
        assert!(before > 0.34 && before < 0.35, "contact sample: {before}");
        assert!(motion.bar_state().value() > 0.1);
        let velocity = motion.lens.borrow().velocity();
        motion.pressed(false, Some(1_050_000_000));
        assert_eq!(motion.lens.borrow().velocity(), velocity);
        for frame in 1..=120 {
            runtime
                .handle()
                .drain_frame_callbacks(1_050_000_000 + frame * 16_666_667);
        }
        assert_eq!(motion.lens_state().value(), 0.0);
        assert_eq!(motion.bar_state().value(), 0.0);
    }

    #[test]
    fn contact_without_input_time_uses_the_shared_frame_clock() {
        let runtime =
            cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
        let motion = TabContactMotion::new(runtime.handle());
        runtime.handle().drain_frame_callbacks(1_000_000_000);
        motion.pressed(true, None);
        runtime.handle().drain_frame_callbacks(1_050_000_000);
        assert!(motion.lens_state().value() > 0.2);
        assert!(motion.bar_state().value() > 0.1);
    }

    #[test]
    fn stopped_shape_returns_to_exact_zero() {
        let runtime =
            cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
        let shape = TabLensShape::new(runtime.handle());
        shape
            .width
            .strain
            .borrow_mut()
            .animateTo(0.0005, spring(0.4, 35.0));
        for frame in 0..600 {
            runtime
                .handle()
                .drain_frame_callbacks(1_000_000_000 + frame * 16_666_667);
            shape.sample(0.0);
        }
        assert_eq!(shape.width.strain.borrow().target(), 0.0);
        assert_eq!(shape.width.strain.borrow().state().value(), 0.0);
        assert!(!shape.width.strain.borrow().is_running());
    }

    #[test]
    fn shape_response_tracks_native_reversal_microframes() {
        let runtime =
            cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
        let mut shape = TabLensShape::new(runtime.handle());
        let mut errors = [[0.0f32; 2]; 8];
        let mut counts = [0; 8];
        let mut extrema = [0.0f32; 2];
        let mut route = usize::MAX;
        let mut output = String::from("route,time,x,native_width,native_height,width,height\n");
        for line in include_str!("../../tests/fixtures/native_tab_shape.csv")
            .lines()
            .skip(1)
        {
            let fields: Vec<f64> = line.split(',').map(|v| v.parse().unwrap()).collect();
            let next = fields[0] as usize;
            if route != next {
                route = next;
                shape = TabLensShape::new(runtime.handle());
            }
            runtime.handle().drain_frame_callbacks(
                (route as u64 + 1) * 10_000_000_000 + (fields[1] * 1e9) as u64,
            );
            let actual = shape.sample(fields[2] as f32);
            output.push_str(&format!(
                "{route},{},{},{},{},{},{}\n",
                fields[1], fields[2], fields[3], fields[4], actual.width, actual.height
            ));
            for (axis, value) in [actual.width, actual.height].into_iter().enumerate() {
                errors[route][axis] += (value - fields[axis + 3] as f32).powi(2);
            }
            extrema[0] = extrema[0].max(actual.width);
            extrema[1] = extrema[1].min(actual.width);
            counts[route] += 1;
        }
        if let Ok(path) = std::env::var("CRANPOSE_OPTICAL_DEBUG") {
            std::fs::write(std::path::Path::new(&path).join("strain.csv"), output).unwrap();
        }
        assert!(counts.iter().all(|count| *count > 350));
        assert!(
            extrema[0] > 0.12 && extrema[1] < -0.12,
            "missing stretch/rebound: {extrema:?}"
        );
        for (route, axes) in errors.into_iter().enumerate() {
            for (axis, error) in axes.into_iter().enumerate() {
                let rms = (error / counts[route] as f32).sqrt();
                assert!(
                    rms < 0.025,
                    "native route {route} axis {axis} strain RMS: {rms}"
                );
            }
        }
    }
}
