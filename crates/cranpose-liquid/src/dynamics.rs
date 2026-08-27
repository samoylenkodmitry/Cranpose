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

use std::{cell::Cell, rc::Rc};

use cranpose_core::{RuntimeHandle, with_current_composer};
use cranpose_macros::composable;

use crate::material::GlassDeformation;

/// Stretch gained per dp/s of travel speed. The target redistributes volume
/// visibly but remains a landscape lens through launch and cruise.
const STRETCH_PER_SPEED: f32 = 3.2e-4;
/// Stretch removed per dp/s² of forward acceleration (and added back per
/// dp/s² of braking): launch blunts the bubble, arrival elongates it.
const STRETCH_PER_ACCEL: f32 = 3.5e-5;
/// The droplet never deforms past these bounds, however violent the fling.
/// Public so hosting nodes can budget layout headroom for the extremes.
pub const STRETCH_MIN: f32 = 0.78;
pub const STRETCH_MAX: f32 = 1.50;
/// Perimeter displacement per dp/s² of acceleration. Launch inertia trails
/// opposite the acceleration; braking inertia runs ahead of the bubble.
const BULGE_PER_ACCELERATION: f32 = 4.5e-4;
pub const BULGE_MAX: f32 = 8.0;
/// Low-pass time constants (s): the fluid's visual inertia is asymmetric —
/// excitation grabs the droplet fast, but it relaxes back viscously (the
/// reference arrival swell stays visible for a beat before rounding off).
const ATTACK_TAU: f32 = 0.03;
const RELEASE_TAU: f32 = 0.11;
/// Direct input arrives independently from rendering. Preserve the last
/// observed finger velocity across short gaps instead of interpreting every
/// intervening render as an instantaneous stop.
const POINTER_VELOCITY_TAU: f32 = 0.045;
const POINTER_STOP_HORIZON_NANOS: u64 = 40_000_000;
const POINTER_COAST_TAU: f32 = 0.10;
/// Below this speed (dp/s) the motion axis holds its last direction, so a
/// settling bubble relaxes in place instead of flipping its axis on noise.
const AXIS_MIN_SPEED: f32 = 60.0;
/// Frame deltas are clamped here (s): a paused scene must not integrate one
/// giant step, and a double-pumped frame must not divide by ~zero.
const DT_MIN: f32 = 1.0 / 1000.0;
const DT_MAX: f32 = 1.0 / 15.0;
/// A position step implying more than this speed (dp/s) is a teleport
/// (window relayout, snap), not motion: state re-anchors without energy.
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
                // Same frame (second glass closure read): no time passed.
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
        // Signed acceleration along travel: positive while gaining speed.
        let accel_along = accel.0 * axis.0 + accel.1 * axis.1;

        let target_stretch = (1.0 + STRETCH_PER_SPEED * raw_speed
            - STRETCH_PER_ACCEL * accel_along)
            .clamp(STRETCH_MIN, STRETCH_MAX);
        let signed_bulge = (-BULGE_PER_ACCELERATION * accel_along).clamp(-BULGE_MAX, BULGE_MAX);
        let target_bulge = (axis.0 * signed_bulge, axis.1 * signed_bulge);

        // Excitation (moving away from neutral) is fast; relaxation back is
        // viscous, so a brake swell lingers a beat like the reference.
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
        let bulge = bulge_vector.0.hypot(bulge_vector.1);
        let speed = follow(self.speed.get(), raw_speed, 0.0);
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
mod tests {
    use super::*;

    fn dynamics() -> LiquidDynamics {
        let runtime =
            cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
        LiquidDynamics::new(runtime.handle())
    }

    fn settle(d: &LiquidDynamics, pos: (f32, f32), frames: usize) -> LiquidPose {
        let mut pose = d.pose();
        for _ in 0..frames {
            pose = d.advance(pos, 1.0 / 60.0);
        }
        pose
    }

    /// Drive at constant velocity and return the steady pose.
    fn cruise(d: &LiquidDynamics, speed_dp_s: f32, frames: usize) -> LiquidPose {
        let dt = 1.0 / 60.0;
        let mut x = 0.0;
        let mut pose = d.pose();
        for _ in 0..frames {
            x += speed_dp_s * dt;
            pose = d.advance((x, 0.0), dt);
        }
        pose
    }

    #[test]
    fn constant_speed_elongates_along_axis_and_conserves_area() {
        let d = dynamics();
        let pose = cruise(&d, 1200.0, 40);
        assert!((1.36..1.40).contains(&pose.stretch));
        assert!((pose.stretch * pose.ortho - 1.0).abs() < 1e-4);
        assert!((pose.axis.0 - 1.0).abs() < 1e-4);
    }

    #[test]
    fn subpixel_pointer_jitter_cannot_invent_extreme_strain() {
        let d = dynamics();
        d.anchor_pointer((0.0, 0.0));
        let mut pose = d.pose();
        for sample in 1..=12 {
            pose = d.advance_pointer((sample as f32 * 0.20, 0.0), 1.0 / 60.0);
        }
        assert!(
            (pose.stretch - 1.0).abs() < 0.025,
            "subpixel travel must stay near equilibrium: {pose:?}"
        );
        assert!((pose.stretch * pose.ortho - 1.0).abs() < 1e-4);
    }

    #[test]
    fn launch_compresses_before_cruise_stretch_wins() {
        let d = dynamics();
        d.advance((0.0, 0.0), 1.0 / 60.0);
        // Hard launch: 0 → 1500 dp/s in one frame (a = 90k dp/s²) — the
        // instantaneous target is compression-dominated.
        let dt = 1.0 / 60.0;
        let pose = d.advance((1500.0 * dt, 0.0), dt);
        assert!(pose.stretch < 1.0, "launch stretch {}", pose.stretch);
        assert!(pose.ortho > 1.0, "launch ortho {}", pose.ortho);
    }

    #[test]
    fn launch_acceleration_leaves_a_persistent_trailing_material_wake() {
        let d = dynamics();
        let dt = 1.0 / 60.0;
        d.advance((0.0, 0.0), dt);
        let launch = d.advance((1200.0 * dt, 0.0), dt);
        assert!(
            launch.bulge_amplitude > 0.5,
            "launch must displace liquid toward the trailing edge: {launch:?}"
        );
        assert!(
            (launch.bulge_direction.abs() - std::f32::consts::PI).abs() < 0.1,
            "rightward acceleration must trail to the left: {launch:?}"
        );

        let cruise = d.advance((2400.0 * dt, 0.0), dt);
        assert!(
            cruise.bulge_amplitude > 0.25,
            "the material wake must survive beyond one pointer sample: {launch:?} -> {cruise:?}"
        );
        assert!(
            (cruise.bulge_direction.abs() - std::f32::consts::PI).abs() < 0.2,
            "the remembered wake cannot flip on the next sample: {cruise:?}"
        );
    }

    #[test]
    fn direct_drag_cadence_remains_launch_compressed() {
        let d = dynamics();
        d.anchor_pointer((0.0, 0.0));
        d.advance_pointer((-20.0, 0.0), 0.08);
        let pose = d.advance_pointer((-40.0, 0.0), 0.03);
        assert!(
            pose.stretch <= 0.92,
            "the target's two-event launch must compress along travel: {pose:?}"
        );
        assert!(
            pose.ortho >= 1.08,
            "launch must expand across travel: {pose:?}"
        );
        assert!((pose.stretch * pose.ortho - 1.0).abs() < 1e-4);
    }

    #[test]
    fn braking_decompresses_past_cruise_and_swells_leading_edge() {
        let d = dynamics();
        let cruise_pose = cruise(&d, 1200.0, 40);
        // Brake to a stop over two frames.
        let dt = 1.0 / 60.0;
        let x = d.last_pos.get().unwrap().0;
        let brake = d.advance((x + 300.0 * dt, 0.0), dt);
        assert!(
            brake.stretch > cruise_pose.stretch + 0.04,
            "brake {} vs cruise {}",
            brake.stretch,
            cruise_pose.stretch
        );
        assert!(
            brake.bulge_amplitude > 0.5,
            "bulge {}",
            brake.bulge_amplitude
        );
        assert!(
            brake.ortho < cruise_pose.ortho,
            "brake ortho {}",
            brake.ortho
        );
        // Leading edge = travel direction (+x).
        assert!(brake.bulge_direction.abs() < 1e-3);
    }

    #[test]
    fn rest_decays_to_identity() {
        let d = dynamics();
        cruise(&d, 1200.0, 40);
        let pose = settle(&d, d.last_pos.get().unwrap(), 60);
        assert!(
            (pose.stretch - 1.0).abs() < 0.02,
            "stretch {}",
            pose.stretch
        );
        assert!(pose.bulge_amplitude < 0.2);
        assert!(pose.speed < 15.0);
    }

    #[test]
    fn axis_follows_motion_direction_and_holds_at_rest() {
        let d = dynamics();
        let dt = 1.0 / 60.0;
        d.advance((0.0, 0.0), dt);
        let mut x = 0.0;
        let mut pose = d.pose();
        for _ in 0..10 {
            x -= 900.0 * dt;
            pose = d.advance((x, 0.0), dt);
        }
        assert!((pose.axis.0 + 1.0).abs() < 1e-4, "axis {:?}", pose.axis);
        assert!(
            pose.bulge_direction.abs() < 0.1,
            "leftward launch inertia must trail toward +x: {pose:?}"
        );
        // Stopping keeps the last axis instead of flipping on noise.
        let held = settle(&d, (x, 0.0), 30);
        assert!((held.axis.0 + 1.0).abs() < 1e-4);
    }

    #[test]
    fn frame_rate_independent_cruise() {
        let d60 = dynamics();
        let d120 = dynamics();
        let cruise60 = cruise(&d60, 1000.0, 30);
        let dt = 1.0 / 120.0;
        let mut x = 0.0;
        let mut cruise120 = d120.pose();
        for _ in 0..60 {
            x += 1000.0 * dt;
            cruise120 = d120.advance((x, 0.0), dt);
        }
        assert!(
            (cruise60.stretch - cruise120.stretch).abs() < 0.02,
            "60Hz {} vs 120Hz {}",
            cruise60.stretch,
            cruise120.stretch
        );
    }

    #[test]
    fn teleport_re_anchors_without_energy() {
        let d = dynamics();
        d.advance((0.0, 0.0), 1.0 / 60.0);
        let pose = d.advance((4000.0, 0.0), 1.0 / 60.0);
        assert_eq!(pose, d.pose());
        assert!(
            (pose.stretch - 1.0).abs() < 1e-4,
            "teleport {}",
            pose.stretch
        );
        // Motion resumes cleanly from the new anchor.
        let resumed = cruise(&d, 800.0, 30);
        assert!(resumed.stretch > 1.01);
    }

    #[test]
    fn vertical_travel_stretches_height_not_width() {
        let d = dynamics();
        let dt = 1.0 / 60.0;
        let mut y = 0.0;
        d.advance((0.0, 0.0), dt);
        let mut pose = d.pose();
        for _ in 0..30 {
            y += 1000.0 * dt;
            pose = d.advance((0.0, y), dt);
        }
        assert!(
            (1.30..1.34).contains(&pose.stretch),
            "stretch {}",
            pose.stretch
        );
        assert!((pose.stretch * pose.ortho - 1.0).abs() < 1e-4);
    }

    #[test]
    fn reset_forgets_motion() {
        let d = dynamics();
        cruise(&d, 1200.0, 40);
        d.reset();
        assert_eq!(d.pose().stretch, 1.0);
        let pose = d.advance((500.0, 0.0), 1.0 / 60.0);
        assert!((pose.stretch - 1.0).abs() < 1e-4);
    }
}
