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
//! magnifies nothing (see [`loupe_target_for_drag`]).

#![allow(non_snake_case)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::composable;
use crate::modifier::Modifier;
use crate::widgets::box_widget::{Box, BoxSpec};
use crate::widgets::popup::Popup;
use cranpose_animation::{spring, Animatable, AnimationSpec, AnimationType, Easing};
use cranpose_core::{remember, with_current_composer};
use cranpose_ui_graphics::{
    liquid_loupe_effect, GraphicsLayer, LayerShape, LiquidLoupeSpec, Point, Rect,
    RoundedCornerShape, Size,
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
/// How far below the line bottom (in line heights) the finger still counts
/// as covering the line. The end/cursor dot's center sits ~0.29 line heights
/// below the bottom (16 dp dot on a 20 dp line), so a dot-center grab falls
/// outside this margin and shows no loupe — the measured behavior.
const LOUPE_LINE_GRAB_MARGIN: f32 = 0.15;

fn loupe_grow_spring() -> AnimationType {
    spring(0.85, 270.0)
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
    /// Fold floor: how far below the line mid the dragged handle's dot
    /// reaches (dp) — the lens's mirror band must sample past it.
    pub dot_clearance: f32,
}

/// The measured visibility rule: the loupe shows while the finger covers the
/// dragged line (grabbing the stem/edge on the line, or the start handle's
/// dot above it), and hides when the drag rides the dot BELOW the line —
/// there the finger obscures nothing.
pub fn loupe_target_for_drag(
    finger: Point,
    line_bottom: f32,
    line_height: f32,
) -> Option<LoupeTarget> {
    let line_height = line_height.max(1.0);
    if finger.y <= line_bottom + LOUPE_LINE_GRAB_MARGIN * line_height {
        Some(LoupeTarget {
            focus_x: finger.x,
            line_mid_y: line_bottom - 0.5 * line_height,
            // The end/cursor dot hangs (2·radius − overlap) below the line
            // box; the mirror must clear its bottom (plus an AA margin).
            dot_clearance: 0.5 * line_height + 2.0 * crate::text_selection::HANDLE_RADIUS
                - crate::text_selection::HANDLE_DOT_LINE_OVERLAP
                + 1.0,
        })
    } else {
        None
    }
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
    /// Low-passed horizontal travel (dp/frame) — the loupe is a droplet and
    /// stretches along its motion instead of gliding rigidly.
    travel: Cell<f32>,
    last_focus_x: Cell<f32>,
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
            progress: RefCell::new(Animatable::new(0.0, runtime)),
            travel: Cell::new(0.0),
            last_focus_x: Cell::new(f32::NAN),
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

    // Droplet motion: stretch along travel with the low-passed drag speed
    // (the reference loupe visibly deforms while following, area conserved).
    let last_x = state.last_focus_x.get();
    let dx = if last_x.is_finite() {
        shown.focus_x - last_x
    } else {
        0.0
    };
    state.last_focus_x.set(shown.focus_x);
    let travel = state.travel.get() * 0.72 + dx * 0.28;
    state.travel.set(travel);
    let stretch = 1.0 + (travel.abs() * 0.022).clamp(0.0, 0.10);
    let width = LOUPE_WIDTH * pose.width_frac * stretch;
    let height = LOUPE_HEIGHT * pose.height_frac / stretch.sqrt();
    let center_x = shown.focus_x;
    let center_y = shown.line_mid_y - LOUPE_RISE * pose.rise_frac;
    // The lens looks at the line (the focus), which sits below the risen
    // center.
    let focus_offset_y = shown.line_mid_y - center_y;

    let corner_radius = 0.5 * width.min(height);
    let spec = LiquidLoupeSpec {
        magnification: LOUPE_MAGNIFICATION,
        focus_offset: (0.0, focus_offset_y),
        seam_lift: shown.dot_clearance,
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
    fn loupe_shows_only_while_the_finger_covers_the_line() {
        let line_bottom = 100.0;
        let line_height = 20.0;
        // Finger on the line: loupe up, focused on the line mid.
        let on_line = loupe_target_for_drag(Point { x: 40.0, y: 95.0 }, line_bottom, line_height)
            .expect("a finger on the line raises the loupe");
        assert_eq!(on_line.focus_x, 40.0);
        assert_eq!(on_line.line_mid_y, 90.0);
        // Finger slightly under the line bottom (within the margin): still up.
        assert!(
            loupe_target_for_drag(Point { x: 40.0, y: 102.0 }, line_bottom, line_height).is_some()
        );
        // Finger on the dot below the line (dot center ≈ bottom + 6dp on a
        // 20dp line): no loupe — the reference drags the end handle by its
        // dot with nothing magnified.
        assert!(
            loupe_target_for_drag(Point { x: 40.0, y: 106.0 }, line_bottom, line_height).is_none()
        );
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
    fn grow_and_release_clocks_match_the_on_white_timeline() {
        let AnimationType::Spring(grow) = loupe_grow_spring() else {
            panic!("loupe grow must use a spring");
        };
        assert_eq!(grow.damping_ratio, 0.85);
        assert_eq!(grow.stiffness, 270.0);
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
        assert_eq!(u[84], relaxed.band_start);
        assert!(
            (24.0..=29.0).contains(&u[87]),
            "the center seam must clear the source handle dot, got {}",
            u[87]
        );

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
        // The capture must reach the focus area plus the fold overshoot.
        assert!(
            shader.input_padding() >= 75.0,
            "capture must cover the offset focus, got {}",
            shader.input_padding()
        );
    }
}
