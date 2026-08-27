//! The liquid-glass text loupe: the magnifier bubble floating over a dragged
//! caret / selection handle.
//!
//! Matched against the reference recording
//! (`example/target/text-selection/`): a 117×82 dp glass capsule whose center
//! rides [`LOUPE_RISE`] dp above the grabbed line's vertical mid. It is a
//! pure backdrop lens — the shader magnifies the live scene (text, selection
//! highlight, the handle itself) a uniform ~1.25×,
//! folding into an inverted, chromatically dispersed band at the rim (see
//! `liquid_glass.wgsl` loupe mode). The widget itself draws nothing.
//!
//! Motion follows `example/target/on-white/text-handle-bubble/`:
//! * the shell starts at the touched handle as a narrow vertical capsule;
//! * width, height, rise, refraction, magnification, chroma, and edge light
//!   increase from one continuous progress value over roughly 200 ms;
//! * every active frame uses the current finger x directly while the vertical
//!   rise supplies the stable grab offset;
//! * release retains the broad face briefly, then sinks into the handle while
//!   the shell and its optics drain over roughly 250 ms.
//!
//! Visibility (also from the recording): the loupe shows only while the
//! finger covers the text line — dragging a handle by its dot below the line
//! rises for every handle interaction (see [`loupe_target_for_drag`]).

#![allow(non_snake_case)]

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_animation::{Animatable, AnimationSpec, AnimationType, Easing, spring};
use cranpose_core::{remember, with_current_composer};
use cranpose_ui_graphics::{
    GraphicsLayer, LayerShape, LiquidLoupeSpec, Point, Rect, RoundedCornerShape, Size,
    liquid_loupe_effect,
};

use crate::{
    composable,
    modifier::Modifier,
    widgets::{
        box_widget::{Box, BoxSpec},
        popup::Popup,
    },
};

/// Bubble size in dp (reference: 350×246 px @3x).
pub const LOUPE_WIDTH: f32 = 117.0;
pub const LOUPE_HEIGHT: f32 = 82.0;
/// Bubble center height above the grabbed line's vertical mid (dp;
/// reference: 226 px @3x).
pub const LOUPE_RISE: f32 = 75.0;
/// Magnification of the lens (uniform; measured on the reference).
pub const LOUPE_MAGNIFICATION: f32 = 1.25;
// The reference loupe deflates into the line over ~100 ms (loupe-dissolve
// frames b_024..b_036 at native 120 fps).
const LOUPE_COLLAPSE_MS: u64 = 120;
fn loupe_grow_spring() -> AnimationType {
    // The reference birth carries ENERGY: the bubble overshoots and wobbles
    // a beat before settling (target loupe-grow frames) — underdamped on
    // purpose.
    spring(0.55, 320.0)
}

fn loupe_collapse_tween() -> AnimationType {
    AnimationType::Tween(AnimationSpec::tween(LOUPE_COLLAPSE_MS, Easing::EaseInOut))
}

/// What the loupe magnifies: the finger x and the grabbed line's vertical
/// mid, in window coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoupeTarget {
    pub focus_x: f32,
    pub line_mid_y: f32,
}

/// The loupe rises for EVERY handle interaction — touch-down on any handle
/// (stem, edge, or the dot hanging below the line) floats the magnifier
/// over the dragged line. The magnified line is derived from the grabbed
/// line, not the raw finger, so riding the dot below still focuses the
/// line the drag manipulates.
pub fn loupe_target_for_drag(
    finger: Point,
    line_bottom: f32,
    line_height: f32,
) -> Option<LoupeTarget> {
    let line_height = line_height.max(1.0);
    Some(LoupeTarget {
        focus_x: finger.x,
        line_mid_y: line_bottom - 0.5 * line_height,
    })
}

/// The bubble's shape/place at one instant: width and height as fractions of
/// the full capsule, and the rise as a fraction of [`LOUPE_RISE`].
#[derive(Clone, Copy, Debug, PartialEq)]
struct LoupePose {
    width_frac: f32,
    height_frac: f32,
    rise_frac: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoupePhase {
    Birth,
    Collapse,
}

/// One material coordinate drives every axis. Birth gains height first so the
/// initial shell is a narrow bulb; collapse retains more width and height until
/// its eased drain, matching the pressure response in the recording.
fn loupe_pose(progress: f32, phase: LoupePhase) -> LoupePose {
    let p = progress.max(0.0);
    let bounded = p.min(1.0);
    let (width_exponent, height_exponent, rise_exponent) = match phase {
        LoupePhase::Birth => (0.60, 0.18, 0.60),
        LoupePhase::Collapse => (0.80, 0.50, 1.0),
    };
    let width = if p <= 1.0 {
        bounded.powf(width_exponent)
    } else {
        p
    };
    LoupePose {
        width_frac: width,
        height_frac: bounded.powf(height_exponent),
        rise_frac: bounded.powf(rise_exponent),
    }
}

fn loupe_optical_activity(progress: f32) -> f32 {
    smoothstep01(progress)
}

fn smoothstep01(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Animated loupe state living across recompositions. One progress value owns
/// shape, rise, and optical opacity in both directions.
struct LoupeState {
    progress: RefCell<Animatable<f32>>,
    /// The bubble's center x trails the touch on the animation clock: a
    /// non-oscillating spring calibrated to the reference's measured trail
    /// (20-35px at ~356px/s => stiffness ~650 critically damped). The live
    /// TRAIL is the drag force: it drives the droplet's stretch.
    follow_x: RefCell<Animatable<f32>>,
    /// The last target shown; kept while the bubble deflates after release.
    shown: RefCell<Option<LoupeTarget>>,
    /// Whether the loupe was active on the previous recomposition.
    was_active: Cell<bool>,
}

/// The floating loupe. Pass the live drag target while a handle drag covers
/// the text line, `None` otherwise; the widget runs its own grow and
/// deflate motion (it stays mounted through the release collapse).
#[composable]
pub fn SelectionLoupe(target: Option<LoupeTarget>) {
    let state = remember(|| {
        let runtime = with_current_composer(|composer| composer.runtime_handle());
        Rc::new(LoupeState {
            progress: RefCell::new(Animatable::new(0.0, runtime.clone())),
            follow_x: RefCell::new(Animatable::new(f32::NAN, runtime)),
            shown: RefCell::new(None),
            was_active: Cell::new(false),
        })
    })
    .with(Rc::clone);

    let active = target.is_some();
    if let Some(t) = target {
        let fresh_grab = !state.was_active.get();
        state.shown.replace(Some(t));
        if fresh_grab {
            let mut progress = state.progress.borrow_mut();
            progress.snapTo(0.0);
            progress.animateTo(1.0, loupe_grow_spring());
        }
    } else if state.was_active.get() {
        let mut progress = state.progress.borrow_mut();
        if progress.state().value() > 0.001 {
            progress.animateTo(0.0, loupe_collapse_tween());
        } else {
            state.shown.replace(None);
        }
    }
    state.was_active.set(active);

    let progress_state = state.progress.borrow().state();
    let p = progress_state.value().max(0.0);
    let Some(shown) = *state.shown.borrow() else {
        return;
    };
    if p <= 0.001 {
        if !active {
            state.shown.replace(None);
        }
        return;
    }

    let pose = loupe_pose(
        p,
        if active {
            LoupePhase::Birth
        } else {
            LoupePhase::Collapse
        },
    );
    let optic = loupe_optical_activity(p);

    // Droplet physics: the bubble CENTER trails the touch on the
    // animation clock (non-oscillating spring; the reference measures a
    // 20-35px trail at ~356px/s and convergence with no overshoot), and
    // the live trail IS the drag force — it stretches the bubble along
    // travel and contracts it orthogonally, AREA CONSERVED
    // (w*s * h/s = w*h).
    {
        let mut follow_anim = state.follow_x.borrow_mut();
        if !follow_anim.state().value().is_finite() {
            follow_anim.snapTo(shown.focus_x);
        } else if (follow_anim.target() - shown.focus_x).abs() > f32::EPSILON {
            // Retarget on every move, carrying velocity: springs keep their
            // frame chain across retargets, so this is a true continuous
            // tracker (steady-state trail 2v/omega, ~22dp at the reference's
            // 356dp/s drag).
            let velocity = follow_anim.velocity();
            follow_anim.animate_to_with_velocity(shown.focus_x, velocity, spring(1.0, 1050.0));
        }
    }
    let follow = state.follow_x.borrow().state().value();
    let trail = shown.focus_x - follow;
    let stretch = 1.0 + (trail.abs() * 0.004).clamp(0.0, 0.12);
    let width = LOUPE_WIDTH * pose.width_frac * stretch;
    let height = LOUPE_HEIGHT * pose.height_frac / stretch;
    let center_x = follow;
    let center_y = shown.line_mid_y - LOUPE_RISE * pose.rise_frac;
    // The lens looks at the line (the focus), which sits below the risen
    // center.
    let focus_offset_y = shown.line_mid_y - center_y;

    let corner_radius = 0.5 * width.min(height);
    let spec = LiquidLoupeSpec {
        magnification: LOUPE_MAGNIFICATION,
        focus_offset: (0.0, focus_offset_y),
        corner_radius,
        activity: optic,
        ..LiquidLoupeSpec::default()
    };

    let anchor = Rect {
        x: center_x - width * 0.5,
        y: center_y - height * 0.5,
        width: 0.0,
        height: 0.0,
    };
    Popup(anchor, Point { x: 0.0, y: 0.0 }, move || {
        let spec = spec.clone();
        Box(
            Modifier::empty()
                .size(Size { width, height })
                .graphics_layer(move || GraphicsLayer {
                    backdrop_effect: Some(liquid_loupe_effect((width, height), &spec)),
                    shape: LayerShape::Rounded(RoundedCornerShape::uniform(corner_radius)),
                    clip: true,
                    ..Default::default()
                }),
            BoxSpec::default(),
            || {},
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loupe_rises_for_every_handle_interaction() {
        let line_bottom = 100.0;
        let line_height = 20.0;
        // Finger on the line: loupe up, focused on the line mid.
        let on_line = loupe_target_for_drag(Point { x: 40.0, y: 95.0 }, line_bottom, line_height)
            .expect("a finger on the line raises the loupe");
        assert_eq!(on_line.focus_x, 40.0);
        assert_eq!(on_line.line_mid_y, 90.0);
        // Finger on the dot below the line: the loupe STILL rises, focused
        // on the grabbed line (any handle touch floats the magnifier).
        let on_dot = loupe_target_for_drag(Point { x: 40.0, y: 106.0 }, line_bottom, line_height)
            .expect("a dot grab raises the loupe too");
        assert_eq!(on_dot.line_mid_y, 90.0);
        // Finger above the line (the start handle's dot): loupe up.
        assert!(
            loupe_target_for_drag(Point { x: 40.0, y: 70.0 }, line_bottom, line_height).is_some()
        );
    }

    #[test]
    fn growth_starts_at_the_handle_as_a_vertical_capsule() {
        assert_eq!(
            loupe_pose(0.0, LoupePhase::Birth),
            LoupePose {
                width_frac: 0.0,
                height_frac: 0.0,
                rise_frac: 0.0,
            }
        );
        let emerging = loupe_pose(0.20, LoupePhase::Birth);
        let width = LOUPE_WIDTH * emerging.width_frac;
        let height = LOUPE_HEIGHT * emerging.height_frac;
        assert!(
            height > width,
            "birth must be vertically elongated: {emerging:?}"
        );
        assert!(emerging.rise_frac < 0.5);

        let settled = loupe_pose(1.0, LoupePhase::Birth);
        assert_eq!(settled.width_frac, 1.0);
        assert_eq!(settled.height_frac, 1.0);
        assert_eq!(settled.rise_frac, 1.0);
    }

    #[test]
    fn width_can_overshoot_without_inflating_height_or_rise() {
        let pose = loupe_pose(1.04, LoupePhase::Birth);
        assert!((pose.width_frac - 1.04).abs() < 1.0e-6);
        assert!((pose.height_frac - 1.0).abs() < 1e-6);
        assert!((pose.rise_frac - 1.0).abs() < 1e-6);
    }

    #[test]
    fn grow_carries_energy_and_release_uses_the_measured_clock() {
        // No value pins while the look is still being matched by vision —
        // only the BEHAVIOR: the birth must be an underdamped spring (the
        // reference bubble overshoots and wobbles a beat).
        let AnimationType::Spring(grow) = loupe_grow_spring() else {
            panic!("loupe grow must use a spring");
        };
        assert!(grow.damping_ratio < 1.0, "birth must carry visible energy");
        let AnimationType::Tween(collapse) = loupe_collapse_tween() else {
            panic!("loupe collapse must use the measured linear clock");
        };
        assert_eq!(collapse.duration_millis, LOUPE_COLLAPSE_MS);
    }

    #[test]
    fn shell_and_optics_share_one_continuous_progress() {
        let early = loupe_pose(0.10, LoupePhase::Birth);
        let middle = loupe_pose(0.50, LoupePhase::Birth);
        let late = loupe_pose(0.90, LoupePhase::Birth);
        assert!(early.width_frac < middle.width_frac && middle.width_frac < late.width_frac);
        assert!(early.height_frac < middle.height_frac && middle.height_frac < late.height_frac);
        assert!(early.rise_frac < middle.rise_frac && middle.rise_frac < late.rise_frac);
        assert!(loupe_optical_activity(0.10) < loupe_optical_activity(0.50));
        assert!(loupe_optical_activity(0.50) < loupe_optical_activity(0.90));
        assert_eq!(loupe_optical_activity(1.0), 1.0);
    }

    #[test]
    fn loupe_effect_relaxes_optics_without_enabling_backdrop_blur() {
        let relaxed = LiquidLoupeSpec {
            activity: 0.65,
            ..LiquidLoupeSpec::default()
        };
        let effect = liquid_loupe_effect((LOUPE_WIDTH, LOUPE_HEIGHT), &relaxed);
        let cranpose_ui_graphics::RenderEffect::Shader { shader } = effect else {
            panic!("loupe must be a bare shader effect");
        };
        let u = shader.uniforms();
        assert!((u[9] - 0.34 * relaxed.activity).abs() < 1e-6);
        assert!((u[83] - (1.0 + (LOUPE_MAGNIFICATION - 1.0) * relaxed.activity)).abs() < 1e-6);
        assert!(
            (u[cranpose_ui_graphics::GLASS_DISPERSION_UNIFORM]
                - relaxed.dispersion * relaxed.activity)
                .abs()
                < 1e-6
        );
        assert!((u[11] - relaxed.highlight * relaxed.activity).abs() < 1e-6);
        assert_eq!(u[28], relaxed.activity);
        assert_eq!(u[90], relaxed.activity);
        assert_eq!(u[cranpose_ui_graphics::GLASS_BLUR_RADIUS_UNIFORM], 0.0);

        let grown = LiquidLoupeSpec::default();
        let effect = liquid_loupe_effect((LOUPE_WIDTH, LOUPE_HEIGHT), &grown);
        let cranpose_ui_graphics::RenderEffect::Shader { shader } = effect else {
            panic!("loupe must be a bare shader effect");
        };
        let u = shader.uniforms();
        assert_eq!(u[80], 1.0, "loupe mode on");
        assert!(
            (u[83] - LOUPE_MAGNIFICATION).abs() < 1e-6,
            "full magnification"
        );
        assert_eq!(u[81], 0.0, "focus x on the bubble center");
        assert!((u[82] - 75.0).abs() < 1e-6, "focus 75dp below the center");
        // Geometry: explicit-rect capsule covering the node, in dp.
        assert_eq!(
            &u[0..2],
            &[LOUPE_WIDTH, LOUPE_HEIGHT],
            "container = node dp"
        );
        assert_eq!(u[6], -1.0, "capsule sentinel");
        // The capture must reach the offset focus area.
        assert!(
            shader.input_padding() >= 75.0,
            "capture must cover the offset focus, got {}",
            shader.input_padding()
        );
    }
}
