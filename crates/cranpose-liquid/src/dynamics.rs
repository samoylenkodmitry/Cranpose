//! Water-droplet motion physics shared by every travelling liquid lens.
//!
//! The reference lens (example/iphone17_records, example/target/tab-swipe)
//! deforms with its motion, not with its position: cruising speed stretches
//! the bubble along the travel axis, acceleration compresses it against the
//! push, deceleration releases that compression past neutral and swells the
//! leading edge — and whatever one axis does, the orthogonal axis does in
//! reverse so the droplet keeps its area (an incompressible bubble).
//!
//! [`LiquidDynamics`] is the one frame integrator implementing that law.
//! Widgets feed it the lens ride position once per drawn frame (from inside
//! their `glass_effect_with` closure) and map the returned [`LiquidPose`]
//! onto the morph uniforms. Time comes from the runtime's animation clock —
//! never wall time — so poses stay exact under robot keyframe captures and
//! on wasm.
//!
//! Every channel relaxes exponentially, so none of them ever reaches
//! neutral on its own. Once travel, strain and swell are all within a
//! fraction of a device pixel of rest, the integrator snaps them to exactly
//! [`LiquidPose::default`] and stops producing new poses, so a settled lens
//! costs nothing per frame.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{RuntimeHandle, with_current_composer};
use cranpose_macros::composable;

use crate::material::GlassDeformation;

const STRETCH_PER_SPEED: f32 = 3.2e-4;
const STRETCH_PER_ACCEL: f32 = 3.5e-5;
/// The droplet never deforms past these bounds, however violent the fling.
/// Public so hosting nodes can budget layout headroom for the extremes.
pub const STRETCH_MIN: f32 = 0.78;
pub const STRETCH_MAX: f32 = 1.50;
const BULGE_PER_ACCELERATION: f32 = 4.5e-4;
pub const BULGE_MAX: f32 = 8.0;
const ATTACK_TAU: f32 = 0.03;
const RELEASE_TAU: f32 = 0.11;
const POINTER_VELOCITY_TAU: f32 = 0.045;
const POINTER_STOP_HORIZON_NANOS: u64 = 40_000_000;
const POINTER_COAST_TAU: f32 = 0.10;
const AXIS_MIN_SPEED: f32 = 60.0;
const REST_SPEED: f32 = 1.0;
const REST_STRETCH: f32 = 5.0e-4;
const REST_BULGE: f32 = 1.0e-2;
const DT_MIN: f32 = 1.0 / 1000.0;
const DT_MAX: f32 = 1.0 / 15.0;
const TELEPORT_SPEED: f32 = 30_000.0;

/// Motion-derived deformation of a liquid lens for one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiquidPose {
    /// Scale along the motion axis (>1 = elongated, <1 = compressed).
    pub stretch: f32,
    /// Scale across the motion axis; always `1/stretch` (area conserved).
    pub ortho: f32,
    /// Unit motion direction (math convention, +x right / +y up in caller
    /// space — callers feed whatever space they draw in).
    pub axis: (f32, f32),
    /// Leading-edge swell amplitude (dp) toward `bulge_direction` (radians).
    pub bulge_amplitude: f32,
    pub bulge_direction: f32,
    /// Smoothed travel speed (dp/s) for auxiliary channels (surface depth,
    /// wobble, chromatic fringe).
    pub speed: f32,
}

impl Default for LiquidPose {
    fn default() -> Self {
        Self {
            stretch: 1.0,
            ortho: 1.0,
            axis: (1.0, 0.0),
            bulge_amplitude: 0.0,
            bulge_direction: 0.0,
            speed: 0.0,
        }
    }
}

impl LiquidPose {
    /// The full rotating strain tensor for the shader. Applying this to the
    /// signed-distance field preserves area for horizontal, vertical and
    /// diagonal travel; projecting it into an axis-aligned width/height pair
    /// cannot preserve that invariant away from the cardinal axes.
    pub fn deformation(&self) -> GlassDeformation {
        GlassDeformation::incompressible(self.axis, self.stretch)
    }

    /// Normalized motion energy in 0..1 (`speed` against a reference fling
    /// of ~1100 dp/s) — the shared ramp for speed-driven side channels.
    pub fn energy(&self) -> f32 {
        (self.speed / 1100.0).clamp(0.0, 1.0)
    }
}

/// Per-lens frame integrator. Interior-mutable so per-frame draw closures
/// (plain `Fn`) can advance it without a `RefCell` dance.
pub struct LiquidDynamics {
    runtime: RuntimeHandle,
    last_nanos: Cell<Option<u64>>,
    last_pos: Cell<Option<(f32, f32)>>,
    velocity: Cell<(f32, f32)>,
    stretch: Cell<f32>,
    bulge_vector: Cell<(f32, f32)>,
    speed: Cell<f32>,
    axis: Cell<(f32, f32)>,
    pose: Cell<LiquidPose>,
    pointer_pose_pending: Cell<bool>,
    pointer_active: Cell<bool>,
    last_pointer_nanos: Cell<Option<u64>>,
}

impl LiquidDynamics {
    pub fn new(runtime: RuntimeHandle) -> Self {
        Self {
            runtime,
            last_nanos: Cell::new(None),
            last_pos: Cell::new(None),
            velocity: Cell::new((0.0, 0.0)),
            stretch: Cell::new(1.0),
            bulge_vector: Cell::new((0.0, 0.0)),
            speed: Cell::new(0.0),
            axis: Cell::new((1.0, 0.0)),
            pose: Cell::new(LiquidPose::default()),
            pointer_pose_pending: Cell::new(false),
            pointer_active: Cell::new(false),
            last_pointer_nanos: Cell::new(None),
        }
    }

    /// Forget all motion state (fresh grab, teleporting relayout): the next
    /// update re-anchors at rest.
    pub fn reset(&self) {
        self.last_nanos.set(None);
        self.last_pos.set(None);
        self.velocity.set((0.0, 0.0));
        self.stretch.set(1.0);
        self.bulge_vector.set((0.0, 0.0));
        self.speed.set(0.0);
        self.pose.set(LiquidPose {
            axis: self.axis.get(),
            ..LiquidPose::default()
        });
        self.pointer_pose_pending.set(false);
        self.pointer_active.set(false);
        self.last_pointer_nanos.set(None);
    }

    pub(crate) fn anchor_pointer(&self, pos: (f32, f32)) {
        self.reset();
        self.last_pos.set(Some(pos));
        let now = self.runtime.last_frame_time_nanos();
        self.last_nanos.set(now);
        self.last_pointer_nanos.set(now);
        self.pointer_active.set(true);
    }

    pub(crate) fn advance_pointer(&self, pos: (f32, f32), dt: f32) -> LiquidPose {
        let Some(last_pos) = self.last_pos.get() else {
            self.last_pos.set(Some(pos));
            return self.pose.get();
        };
        let dt = dt.clamp(DT_MIN, DT_MAX);
        let raw_velocity = ((pos.0 - last_pos.0) / dt, (pos.1 - last_pos.1) / dt);
        self.last_pos.set(Some(pos));
        let previous = self.velocity.get();
        let follow = 1.0 - (-dt / POINTER_VELOCITY_TAU).exp();
        let filtered_velocity = (
            previous.0 + (raw_velocity.0 - previous.0) * follow,
            previous.1 + (raw_velocity.1 - previous.1) * follow,
        );
        let pose = self.advance_velocity(filtered_velocity, dt);
        let now = self.runtime.last_frame_time_nanos();
        self.last_nanos.set(now);
        self.last_pointer_nanos.set(now);
        self.pointer_active.set(true);
        self.pointer_pose_pending.set(true);
        pose
    }

    pub(crate) fn release_pointer(&self) {
        self.pointer_active.set(false);
    }

    pub(crate) fn update_pointer(&self, pos: (f32, f32)) -> LiquidPose {
        if self.pointer_pose_pending.replace(false) {
            self.last_pos.set(Some(pos));
            self.last_nanos.set(self.runtime.last_frame_time_nanos());
            return self.pose.get();
        }
        let Some(now) = self.runtime.last_frame_time_nanos() else {
            self.last_pos.set(Some(pos));
            return self.pose.get();
        };
        let Some(last) = self.last_nanos.get() else {
            self.last_nanos.set(Some(now));
            self.last_pos.set(Some(pos));
            return self.pose.get();
        };
        if last == now {
            return self.pose.get();
        }
        let dt = (now.saturating_sub(last)) as f32 / 1_000_000_000.0;
        self.last_nanos.set(Some(now));
        let stationary = self.last_pos.get().is_some_and(|last_pos| {
            (last_pos.0 - pos.0).abs() < 0.001 && (last_pos.1 - pos.1).abs() < 0.001
        });
        if !stationary {
            return self.advance(pos, dt);
        }
        let within_pointer_horizon = self.pointer_active.get()
            && self
                .last_pointer_nanos
                .get()
                .is_some_and(|sample| now.saturating_sub(sample) <= POINTER_STOP_HORIZON_NANOS);
        if within_pointer_horizon {
            return self.pose.get();
        }
        let dt = dt.clamp(DT_MIN, DT_MAX);
        let decay = (-dt / POINTER_COAST_TAU).exp();
        let velocity = self.velocity.get();
        self.advance_velocity((velocity.0 * decay, velocity.1 * decay), dt)
    }

    /// Advance with the lens ride position using the runtime's animation
    /// clock. Multiple reads within one frame return the same pose.
    pub fn update(&self, pos: (f32, f32)) -> LiquidPose {
        let Some(now) = self.runtime.last_frame_time_nanos() else {
            self.last_pos.set(Some(pos));
            return self.pose.get();
        };
        match self.last_nanos.get() {
            Some(last) if last == now => {
                self.last_pos.set(Some(pos));
                self.pose.get()
            }
            Some(last) => {
                let dt = (now.saturating_sub(last)) as f32 / 1_000_000_000.0;
                self.last_nanos.set(Some(now));
                self.advance(pos, dt)
            }
            None => {
                self.last_nanos.set(Some(now));
                self.last_pos.set(Some(pos));
                self.pose.get()
            }
        }
    }

    /// Pure integration step (exposed for tests and custom clocks): advance
    /// by `dt` seconds toward `pos`.
    pub fn advance(&self, pos: (f32, f32), dt: f32) -> LiquidPose {
        let Some(last_pos) = self.last_pos.get() else {
            self.last_pos.set(Some(pos));
            return self.pose.get();
        };
        let dt = dt.clamp(DT_MIN, DT_MAX);
        let delta = (pos.0 - last_pos.0, pos.1 - last_pos.1);
        self.last_pos.set(Some(pos));

        let velocity = (delta.0 / dt, delta.1 / dt);
        let raw_speed = (velocity.0 * velocity.0 + velocity.1 * velocity.1).sqrt();
        if raw_speed > TELEPORT_SPEED {
            self.velocity.set((0.0, 0.0));
            return self.pose.get();
        }

        self.advance_velocity(velocity, dt)
    }

    fn advance_velocity(&self, velocity: (f32, f32), dt: f32) -> LiquidPose {
        let dt = dt.clamp(DT_MIN, DT_MAX);
        let raw_speed = (velocity.0 * velocity.0 + velocity.1 * velocity.1).sqrt();
        let previous_velocity = self.velocity.get();
        self.velocity.set(velocity);
        let accel = (
            (velocity.0 - previous_velocity.0) / dt,
            (velocity.1 - previous_velocity.1) / dt,
        );

        let mut axis = self.axis.get();
        if raw_speed > AXIS_MIN_SPEED {
            axis = (velocity.0 / raw_speed, velocity.1 / raw_speed);
            self.axis.set(axis);
        }
        let accel_along = accel.0 * axis.0 + accel.1 * axis.1;

        let target_stretch = (1.0 + STRETCH_PER_SPEED * raw_speed
            - STRETCH_PER_ACCEL * accel_along)
            .clamp(STRETCH_MIN, STRETCH_MAX);
        let signed_bulge = (-BULGE_PER_ACCELERATION * accel_along).clamp(-BULGE_MAX, BULGE_MAX);
        let target_bulge = (axis.0 * signed_bulge, axis.1 * signed_bulge);

        let follow = |current: f32, target: f32, neutral: f32| {
            let tau = if (target - neutral).abs() > (current - neutral).abs() {
                ATTACK_TAU
            } else {
                RELEASE_TAU
            };
            current + (target - current) * (1.0 - (-dt / tau).exp())
        };
        let stretch = follow(self.stretch.get(), target_stretch, 1.0);
        let current_bulge = self.bulge_vector.get();
        let current_bulge_length = current_bulge.0.hypot(current_bulge.1);
        let target_bulge_length = target_bulge.0.hypot(target_bulge.1);
        let bulge_tau = if target_bulge_length > current_bulge_length {
            ATTACK_TAU
        } else {
            RELEASE_TAU
        };
        let bulge_follow = 1.0 - (-dt / bulge_tau).exp();
        let bulge_vector = (
            current_bulge.0 + (target_bulge.0 - current_bulge.0) * bulge_follow,
            current_bulge.1 + (target_bulge.1 - current_bulge.1) * bulge_follow,
        );
        let speed = follow(self.speed.get(), raw_speed, 0.0);
        let at_rest = raw_speed <= REST_SPEED
            && speed <= REST_SPEED
            && (stretch - 1.0).abs() <= REST_STRETCH
            && bulge_vector.0.hypot(bulge_vector.1) <= REST_BULGE;
        let (stretch, bulge_vector, speed) = if at_rest {
            (1.0, (0.0, 0.0), 0.0)
        } else {
            (stretch, bulge_vector, speed)
        };
        let bulge = bulge_vector.0.hypot(bulge_vector.1);
        self.stretch.set(stretch);
        self.bulge_vector.set(bulge_vector);
        self.speed.set(speed);

        let bulge_direction = if bulge > 1.0e-4 {
            bulge_vector.1.atan2(bulge_vector.0)
        } else {
            axis.1.atan2(axis.0)
        };

        let pose = LiquidPose {
            stretch,
            ortho: 1.0 / stretch,
            axis,
            bulge_amplitude: bulge,
            bulge_direction,
            speed,
        };
        self.pose.set(pose);
        pose
    }

    /// Latest pose without advancing.
    pub fn pose(&self) -> LiquidPose {
        self.pose.get()
    }
}

/// Remember one [`LiquidDynamics`] for the calling composition site.
#[composable]
#[track_caller]
pub fn rememberLiquidDynamics() -> Rc<LiquidDynamics> {
    let caller = cranpose_core::caller_location_key();
    with_current_composer(|composer| {
        let runtime = composer.runtime_handle();
        composer
            .remember_at(caller, move || Rc::new(LiquidDynamics::new(runtime)))
            .with(Rc::clone)
    })
}

#[cfg(test)]
#[path = "tests/dynamics_tests.rs"]
mod tests;
