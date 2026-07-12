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

use std::cell::Cell;
use std::rc::Rc;

use cranpose_core::with_current_composer;
use cranpose_core::RuntimeHandle;
use cranpose_macros::composable;

/// Stretch gained per dp/s of travel speed (measured: the App Store bar lens
/// runs ~1.3× wide mid-flight at ~1200 dp/s).
const STRETCH_PER_SPEED: f32 = 2.5e-4;
/// Stretch removed per dp/s² of forward acceleration (and added back per
/// dp/s² of braking): launch blunts the bubble, arrival elongates it.
const STRETCH_PER_ACCEL: f32 = 1.1e-5;
/// The droplet never deforms past these bounds, however violent the fling.
/// Public so hosting nodes can budget layout headroom for the extremes.
pub const STRETCH_MIN: f32 = 0.78;
pub const STRETCH_MAX: f32 = 1.42;
/// Leading-edge swell per dp/s² of braking, and its cap in dp (public for
/// the same headroom budgeting).
const BULGE_PER_DECEL: f32 = 4.5e-4;
pub const BULGE_MAX: f32 = 8.0;
/// Low-pass time constants (s): the fluid's visual inertia is asymmetric —
/// excitation grabs the droplet fast, but it relaxes back viscously (the
/// reference arrival swell stays visible for a beat before rounding off).
const ATTACK_TAU: f32 = 0.035;
const RELEASE_TAU: f32 = 0.11;
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
    /// Smoothed travel speed (dp/s) for auxiliary channels (magnify boost,
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
    /// Project the anisotropic scale onto an axis-aligned box: the diagonal
    /// of `R·diag(stretch, ortho)·Rᵀ` for the motion axis' rotation `R`.
    /// Exact for horizontal/vertical travel, the correct tensor blend for
    /// anything between.
    pub fn size(&self, width: f32, height: f32) -> (f32, f32) {
        let cos2 = self.axis.0 * self.axis.0;
        let sin2 = self.axis.1 * self.axis.1;
        let scale_x = self.stretch * cos2 + self.ortho * sin2;
        let scale_y = self.stretch * sin2 + self.ortho * cos2;
        (width * scale_x, height * scale_y)
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
    bulge: Cell<f32>,
    speed: Cell<f32>,
    axis: Cell<(f32, f32)>,
    pose: Cell<LiquidPose>,
}

impl LiquidDynamics {
    pub fn new(runtime: RuntimeHandle) -> Self {
        Self {
            runtime,
            last_nanos: Cell::new(None),
            last_pos: Cell::new(None),
            velocity: Cell::new((0.0, 0.0)),
            stretch: Cell::new(1.0),
            bulge: Cell::new(0.0),
            speed: Cell::new(0.0),
            axis: Cell::new((1.0, 0.0)),
            pose: Cell::new(LiquidPose::default()),
        }
    }

    /// Forget all motion state (fresh grab, teleporting relayout): the next
    /// update re-anchors at rest.
    pub fn reset(&self) {
        self.last_nanos.set(None);
        self.last_pos.set(None);
        self.velocity.set((0.0, 0.0));
        self.stretch.set(1.0);
        self.bulge.set(0.0);
        self.speed.set(0.0);
        self.pose.set(LiquidPose {
            axis: self.axis.get(),
            ..LiquidPose::default()
        });
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
        let target_bulge = (BULGE_PER_DECEL * (-accel_along).max(0.0)).min(BULGE_MAX);

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
        let bulge = follow(self.bulge.get(), target_bulge, 0.0);
        let speed = follow(self.speed.get(), raw_speed, 0.0);
        self.stretch.set(stretch);
        self.bulge.set(bulge);
        self.speed.set(speed);

        let pose = LiquidPose {
            stretch,
            ortho: 1.0 / stretch,
            axis,
            bulge_amplitude: bulge,
            bulge_direction: axis.1.atan2(axis.0),
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
pub fn remember_liquid_dynamics() -> Rc<LiquidDynamics> {
    with_current_composer(|composer| {
        let runtime = composer.runtime_handle();
        composer
            .remember(move || Rc::new(LiquidDynamics::new(runtime)))
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
        assert!(
            pose.stretch > 1.2 && pose.stretch < STRETCH_MAX,
            "cruise stretch {}",
            pose.stretch
        );
        assert!((pose.stretch * pose.ortho - 1.0).abs() < 1e-4);
        assert!((pose.axis.0 - 1.0).abs() < 1e-4);
        let (w, h) = pose.size(100.0, 40.0);
        assert!(w > 120.0 && h < 40.0, "size ({w}, {h})");
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
    }

    #[test]
    fn braking_stretches_past_cruise_and_swells_leading_edge() {
        let d = dynamics();
        let cruise_pose = cruise(&d, 1200.0, 40);
        // Brake to a stop over two frames.
        let dt = 1.0 / 60.0;
        let x = d.last_pos.get().unwrap().0;
        let brake = d.advance((x + 300.0 * dt, 0.0), dt);
        assert!(
            brake.stretch > cruise_pose.stretch - 1e-3,
            "brake {} vs cruise {}",
            brake.stretch,
            cruise_pose.stretch
        );
        assert!(
            brake.bulge_amplitude > 0.5,
            "bulge {}",
            brake.bulge_amplitude
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
        assert!((pose.bulge_direction.abs() - std::f32::consts::PI).abs() < 1e-3);
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
        assert!(resumed.stretch > 1.1);
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
        let (w, h) = pose.size(100.0, 40.0);
        assert!(h > 44.0, "height {h}");
        assert!(w < 100.0, "width {w}");
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
