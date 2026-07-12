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
//! Motion, all measured from the 120 fps reference:
//! * birth: ~120 ms after the grab (the menu dissolves first); a grab
//!   released within the delay never shows a loupe;
//! * grow-in: born a near-square squircle — already ~93% of the final
//!   HEIGHT but only ~63% of the WIDTH — low over the line (~82% risen)
//!   with FULL optics (magnified text, dot and rim all present on the
//!   first visible frame). The width springs out to the capsule with a
//!   ~+6% overshoot peaking ~200 ms after birth; the rise eases out
//!   (τ ≈ 95 ms) with no overshoot;
//! * follow: the center trails the finger x with a ~80 ms critically damped
//!   lag (the magnified handle rides ahead of the bubble center mid-drag);
//!   the y is LOCKED to the grabbed line, never the finger;
//! * release: pixel-still for the first ~8 ms, then a fast shrink (to ~3/4
//!   of the released size by +25 ms) with a slight sink, fading THROUGH its
//!   own optics — magnification and rim die together, so the lens reads
//!   translucent mid-fade and is gone by ~55 ms.
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
use cranpose_animation::{spring, Animatable, AnimationSpec, AnimationType};
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
/// The birth pose (reference `loupe-grow/a_068`, the first visible frame):
/// a near-square squircle — width well short of the capsule, height nearly
/// full, most of the rise already done.
const LOUPE_BIRTH_WIDTH_FRAC: f32 = 0.63;
const LOUPE_BIRTH_HEIGHT_FRAC: f32 = 0.93;
const LOUPE_BIRTH_RISE_FRAC: f32 = 0.82;
/// Dissolve: the fraction of the released size the bubble shrinks toward,
/// the fraction of [`LOUPE_RISE`] it plunges back toward the line, and the
/// content-alpha floor it fades to (all measured: by +25 ms the reference
/// has sunk ~26 dp and dimmed its whole content to ~65-70%, holding there
/// until the terminal vanish).
const LOUPE_DISSOLVE_SHRINK: f32 = 0.30;
const LOUPE_DISSOLVE_SINK: f32 = 0.35;
const LOUPE_DISSOLVE_ALPHA_FLOOR: f32 = 0.65;
/// Delay between the grab and the bubble's birth (reference: the menu
/// dissolves first; the bubble appears ~120 ms after the touch-down). A grab
/// released within the delay never shows a loupe.
const LOUPE_BIRTH_DELAY_MS: u64 = 120;
/// How far below the line bottom (in line heights) the finger still counts
/// as covering the line. The end/cursor dot's center sits ~0.29 line heights
/// below the bottom (16 dp dot on a 20 dp line), so a dot-center grab falls
/// outside this margin and shows no loupe — the measured behavior.
const LOUPE_LINE_GRAB_MARGIN: f32 = 0.15;

/// Width grow-in: ζ≈0.5 → the width overshoots ~+6% of the capsule width
/// (16% of the birth→full step) peaking ~200 ms after birth — both measured
/// on the reference inflate.
fn loupe_grow_spring() -> AnimationType {
    spring(0.5, 310.0)
}

/// Rise: overdamped (no overshoot), an ~95 ms-τ ease-out — the reference top
/// edge climbs monotonically, half done ~65 ms after birth, settled ~350 ms.
fn loupe_rise_spring() -> AnimationType {
    spring(2.0, 1550.0)
}

/// The birth-delay gate: a linear timer from the grab to the bubble's birth.
fn loupe_birth_gate() -> AnimationType {
    AnimationType::Tween(AnimationSpec::linear(LOUPE_BIRTH_DELAY_MS))
}

/// Release: the ~55 ms measured fade. The raw value runs release→0; the
/// pose mapping (see [`dissolve_pose`]) holds still through the first ~15%,
/// then shrinks fast and fades through the optics.
fn loupe_collapse_tween() -> AnimationType {
    AnimationType::Tween(AnimationSpec::linear(55))
}

/// Horizontal follow: critically damped, τ≈80 ms — the bubble trails a
/// ~356 px/s drag by ~30 px like the reference.
fn loupe_follow_spring() -> AnimationType {
    spring(1.0, 625.0)
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

/// Grow mapping: the width carries the spring (including its overshoot past
/// 1), the height clamps it (the reference capsule never grows taller than
/// final), the rise runs on its own overdamped driver.
fn grow_pose(p: f32, rise: f32) -> LoupePose {
    let pc = p.clamp(0.0, 1.0);
    LoupePose {
        width_frac: LOUPE_BIRTH_WIDTH_FRAC + (1.0 - LOUPE_BIRTH_WIDTH_FRAC) * p.max(0.0),
        height_frac: LOUPE_BIRTH_HEIGHT_FRAC + (1.0 - LOUPE_BIRTH_HEIGHT_FRAC) * pc,
        rise_frac: LOUPE_BIRTH_RISE_FRAC + (1.0 - LOUPE_BIRTH_RISE_FRAC) * rise.clamp(0.0, 1.0),
    }
}

/// Dissolve mapping over `q` (1 at the release, 0 fully gone), RELATIVE to
/// the pose the bubble was released from — releasing a newborn shrinks the
/// newborn, not a full capsule. Returns the pose plus the CONTENT ALPHA
/// (the spec `progress`): the reference holds pixel-still through the
/// first ~15% (≈8 ms), then drops fast — by +25 ms it has shrunk toward
/// ~3/4, PLUNGED ~a third of its rise back toward the line, and dimmed its
/// whole content (glyphs, rim, fold together) to the alpha floor, holding
/// that translucency until the terminal vanish at ~55 ms. The optics never
/// animate; only geometry and the content blend do.
fn dissolve_pose(released: LoupePose, q: f32) -> (LoupePose, f32) {
    let d = 1.0 - q.clamp(0.0, 1.0);
    let u = ((d - 0.15) / 0.85).clamp(0.0, 1.0);
    let shrink = (u * 1.9).min(1.0);
    // The sink and the fade both complete EARLY (by ~+25 ms) — the last
    // stretch of the tween only finishes the shrink before the unmount.
    let plunge = (u * 2.8).min(1.0);
    let factor = 1.0 - LOUPE_DISSOLVE_SHRINK * shrink;
    let pose = LoupePose {
        width_frac: released.width_frac * factor,
        height_frac: released.height_frac * factor,
        rise_frac: released.rise_frac - LOUPE_DISSOLVE_SINK * plunge,
    };
    (pose, 1.0 - (1.0 - LOUPE_DISSOLVE_ALPHA_FLOOR) * plunge)
}

/// Animated loupe state living across recompositions: grow/collapse progress
/// plus the rise and horizontal follow, and the frozen target/pose while
/// dissolving.
struct LoupeState {
    progress: RefCell<Animatable<f32>>,
    /// Birth-delay timer: runs 0→1 over [`LOUPE_BIRTH_DELAY_MS`] from the
    /// grab; the grow starts only once it completes.
    gate: RefCell<Animatable<f32>>,
    /// Rise driver, 0→1 from the birth (separate from the width spring: the
    /// reference rise never overshoots while the width does).
    rise: RefCell<Animatable<f32>>,
    follow_x: RefCell<Animatable<f32>>,
    /// The last target shown; kept while the bubble deflates after release.
    shown: RefCell<Option<LoupeTarget>>,
    /// The pose and progress value the bubble was released from — the
    /// dissolve shrinks THAT pose, whatever its size (releasing a newborn
    /// must not snap it to a full capsule first).
    released: Cell<Option<(f32, LoupePose)>>,
    /// Whether the loupe was active on the previous recomposition (drives
    /// snap-vs-glide on a fresh grab).
    was_active: std::cell::Cell<bool>,
}

/// The floating loupe. Pass the live drag target while a handle drag covers
/// the text line, `None` otherwise; the widget runs its own grow, follow and
/// deflate motion (it stays mounted through the release collapse).
#[composable]
pub fn SelectionLoupe(target: Option<LoupeTarget>) {
    let state = remember(|| {
        let runtime = with_current_composer(|composer| composer.runtime_handle());
        Rc::new(LoupeState {
            progress: RefCell::new(Animatable::new(0.0, runtime.clone())),
            gate: RefCell::new(Animatable::new(0.0, runtime.clone())),
            rise: RefCell::new(Animatable::new(0.0, runtime.clone())),
            follow_x: RefCell::new(Animatable::new(0.0, runtime)),
            shown: RefCell::new(None),
            released: Cell::new(None),
            was_active: std::cell::Cell::new(false),
        })
    })
    .with(Rc::clone);

    let active = target.is_some();
    if let Some(t) = target {
        let fresh_grab = !state.was_active.get();
        {
            let mut follow = state.follow_x.borrow_mut();
            if fresh_grab {
                // A new grab: the bubble is born AT the grab point — no glide
                // from wherever the previous loupe died.
                follow.snapTo(t.focus_x);
            } else if (follow.target() - t.focus_x).abs() > f32::EPSILON {
                follow.animateTo(t.focus_x, loupe_follow_spring());
            }
        }
        state.shown.replace(Some(t));
        if fresh_grab {
            let mut gate = state.gate.borrow_mut();
            gate.snapTo(0.0);
            gate.animateTo(1.0, loupe_birth_gate());
            state.progress.borrow_mut().snapTo(0.0);
            state.rise.borrow_mut().snapTo(0.0);
            state.released.set(None);
        }
        // The grow starts only once the birth gate elapses (reading the gate
        // state subscribes this composition to its frames, so the flip
        // recomposes even during a motionless hold).
        let born = state.gate.borrow().state().value() >= 1.0;
        if born {
            let mut progress = state.progress.borrow_mut();
            if (progress.target() - 1.0).abs() > f32::EPSILON {
                progress.animateTo(1.0, loupe_grow_spring());
                state.rise.borrow_mut().animateTo(1.0, loupe_rise_spring());
            }
        }
    } else {
        let mut progress = state.progress.borrow_mut();
        if progress.target() != 0.0 {
            // Freeze the pose at the release: the dissolve shrinks it
            // relatively, and the rise driver must stop mid-flight instead
            // of climbing under the fade.
            let p_rel = progress.state().value().max(1.0e-3);
            let mut rise = state.rise.borrow_mut();
            let rise_rel = rise.state().value();
            rise.snapTo(rise_rel);
            state
                .released
                .set(Some((p_rel, grow_pose(p_rel, rise_rel))));
            progress.animateTo(0.0, loupe_collapse_tween());
        }
    }
    state.was_active.set(active);

    let progress_state = state.progress.borrow().state();
    let rise_state = state.rise.borrow().state();
    let follow_state = state.follow_x.borrow().state();
    let p = progress_state.value().max(0.0);
    let Some(shown) = *state.shown.borrow() else {
        return;
    };
    let born = active && state.gate.borrow().state().value() >= 1.0;
    if p <= 0.001 && !born && state.released.get().is_none() {
        // Not yet born (birth delay) — or a grab released inside the delay,
        // which never shows a loupe at all. Once BORN, p = 0 is the birth
        // pose itself (63% wide, full optics) and must render — the
        // reference's first frame is clearly visible.
        if !active {
            state.shown.replace(None);
        }
        return;
    }

    let (pose, optic) = if active {
        (grow_pose(p, rise_state.value()), 1.0)
    } else {
        let Some((p_rel, released)) = state.released.get() else {
            state.shown.replace(None);
            return;
        };
        let q = (p / p_rel).clamp(0.0, 1.0);
        if q <= 0.02 {
            // Fully faded: unmount until the next grab.
            state.shown.replace(None);
            state.released.set(None);
            return;
        }
        dissolve_pose(released, q)
    };

    let width = LOUPE_WIDTH * pose.width_frac;
    let height = LOUPE_HEIGHT * pose.height_frac;
    let center_x = follow_state.value();
    let center_y = shown.line_mid_y - LOUPE_RISE * pose.rise_frac;
    // The lens looks at the line (the focus), which sits below the risen
    // center.
    let focus_offset_y = shown.line_mid_y - center_y;

    // The newborn reference silhouette is a flat-topped SQUIRCLE, rounding
    // into the full capsule as the width fills out (a stadium at birth
    // width ~= height reads as a plain circle, which the reference never
    // shows). The grow spring drives the morph; dissolve keeps the capsule.
    let corner_frac = if active {
        0.38 + (0.5 - 0.38) * (p * 2.0).clamp(0.0, 1.0)
    } else {
        0.5
    };
    let corner_radius = height * corner_frac;
    let spec = LiquidLoupeSpec {
        magnification: LOUPE_MAGNIFICATION,
        focus_offset: (0.0, focus_offset_y),
        seam_lift: shown.dot_clearance,
        corner_radius,
        // The optics are constant for the lens's whole life; the dissolve
        // fades the CONTENT (a true translucency blend in the shader).
        progress: optic,
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
    fn birth_pose_is_a_low_near_square_squircle() {
        // Reference a_068: ~63% width, ~93% height, ~82% risen — NOT a
        // half-scale miniature (the uniform-scale model read as a blob a
        // third of the final size).
        let pose = grow_pose(0.0, 0.0);
        assert!((pose.width_frac - LOUPE_BIRTH_WIDTH_FRAC).abs() < 1e-6);
        assert!((pose.height_frac - LOUPE_BIRTH_HEIGHT_FRAC).abs() < 1e-6);
        assert!((pose.rise_frac - LOUPE_BIRTH_RISE_FRAC).abs() < 1e-6);
        // Near-square: the birth aspect is within a few percent of 1.
        let aspect = (LOUPE_WIDTH * pose.width_frac) / (LOUPE_HEIGHT * pose.height_frac);
        assert!((0.9..=1.05).contains(&aspect), "birth aspect {aspect}");
    }

    #[test]
    fn grow_width_carries_the_overshoot_and_height_clamps_it() {
        // The ζ=0.5 width spring peaks ~16% past the step → ~+6% capsule
        // width, like the reference's ~375px peak; the height must NOT
        // follow past full.
        let pose = grow_pose(1.16, 1.0);
        assert!(pose.width_frac > 1.05 && pose.width_frac < 1.07);
        assert!((pose.height_frac - 1.0).abs() < 1e-6);
        assert!((pose.rise_frac - 1.0).abs() < 1e-6);
    }

    #[test]
    fn dissolve_holds_still_then_plunges_shrinks_and_dims() {
        let released = grow_pose(1.0, 1.0);
        // First ~15% of the fade (≈8 ms): pixel-still, fully opaque — the
        // reference's +8 ms frame is identical to the held one.
        let (pose, alpha) = dissolve_pose(released, 0.9);
        assert_eq!(pose, released);
        assert_eq!(alpha, 1.0);
        // Mid-fade (+25 ms ≈ q 0.55): shrunk toward ~3/4, PLUNGED about a
        // third of the rise back toward the line, and already dimmed to
        // the measured ~65% content alpha.
        let (pose, alpha) = dissolve_pose(released, 0.55);
        assert!(
            pose.width_frac < 0.85 && pose.width_frac > 0.75,
            "mid-fade width {}",
            pose.width_frac
        );
        assert!(
            released.rise_frac - pose.rise_frac > 0.30,
            "the bubble plunges toward the line by +25 ms, sank {}",
            released.rise_frac - pose.rise_frac
        );
        assert!(
            (alpha - LOUPE_DISSOLVE_ALPHA_FLOOR).abs() < 0.02,
            "content dimmed to the floor by +25 ms, got {alpha}"
        );
        // Late (+42 ms ≈ q 0.24): shrink floor reached, translucency and
        // sink HOLD (the reference plateaus until the terminal vanish).
        let (pose, alpha) = dissolve_pose(released, 0.24);
        assert!((pose.width_frac - 0.70).abs() < 0.02);
        assert!((alpha - LOUPE_DISSOLVE_ALPHA_FLOOR).abs() < 1e-6);
        assert!((released.rise_frac - pose.rise_frac - LOUPE_DISSOLVE_SINK).abs() < 1e-6);
    }

    #[test]
    fn dissolving_a_newborn_shrinks_the_newborn_pose() {
        // Regression: the old absolute mapping snapped a newborn release to
        // 70% of a FULL capsule (bigger than the newborn itself) and then
        // unmounted instantly because the raw progress sat under the
        // absolute cliff. The dissolve must scale the released pose.
        let newborn = grow_pose(0.02, 0.05);
        let (pose, optic) = dissolve_pose(newborn, 1.0);
        assert_eq!(
            pose, newborn,
            "the fade starts exactly at the released pose"
        );
        assert_eq!(optic, 1.0);
        let (pose, _) = dissolve_pose(newborn, 0.3);
        assert!(pose.width_frac < newborn.width_frac);
        assert!(pose.width_frac >= newborn.width_frac * (1.0 - LOUPE_DISSOLVE_SHRINK) - 1e-6);
    }

    #[test]
    fn loupe_effect_keeps_optics_constant_and_fades_content_alpha() {
        // The lens is FIXED-OPTIC for its whole life; `progress` is the
        // CONTENT ALPHA (uniform 90) — the dissolve blends the whole lens
        // output toward the plain backdrop while magnification, rim and
        // dispersion stay at full power (the reference's magnified glyphs
        // stay magnified while dimming).
        let dimmed = LiquidLoupeSpec {
            progress: 0.65,
            ..LiquidLoupeSpec::default()
        };
        let effect = liquid_loupe_effect((LOUPE_WIDTH, LOUPE_HEIGHT), &dimmed);
        let cranpose_ui_graphics::RenderEffect::Shader { shader } = effect else {
            panic!("loupe must be a bare shader effect");
        };
        let u = shader.uniforms();
        assert!(
            (u[83] - LOUPE_MAGNIFICATION).abs() < 1e-6,
            "magnification never animates, got {}",
            u[83]
        );
        assert!(u[86] > 0.0, "dispersion never animates");
        assert!(
            (u[90] - 0.65).abs() < 1e-6,
            "content alpha rides uniform 90"
        );
        assert!(
            u[87] > 20.0,
            "the fold floor clears the handle dot, got {}",
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
