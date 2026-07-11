//! The liquid-glass text loupe: the magnifier bubble floating over a dragged
//! caret / selection handle.
//!
//! Matched against the reference recording
//! (`example/target/text-selection/`): a 117×82 dp glass capsule whose center
//! rides [`LOUPE_RISE`] dp above the grabbed line's vertical mid. It is a
//! pure backdrop lens — the shader magnifies the live scene (text, selection
//! highlight, the handle itself) 1.7× at the center with a dome falloff,
//! folding into an inverted, chromatically dispersed band at the rim (see
//! `liquid_glass.wgsl` loupe mode). The widget itself draws nothing.
//!
//! Motion, all measured from the 120 fps reference:
//! * birth: ~120 ms after the grab (the menu dissolves first); a grab
//!   released within the delay never shows a loupe;
//! * grow-in: born ON the line at ~half size, nearly round and already
//!   ~85% magnified with a visible rim — a fixed-optic lens whose shape
//!   grows — widening into the capsule with an ~8% overshoot peaking
//!   ~190 ms after birth, the rise outrunning the inflation;
//! * follow: the center trails the finger x with a ~80 ms critically damped
//!   lag (the magnified handle rides ahead of the bubble center mid-drag);
//!   the y is LOCKED to the grabbed line, never the finger;
//! * release: the bubble deflates back into the line in ~55 ms.
//!
//! Visibility (also from the recording): the loupe shows only while the
//! finger covers the text line — dragging a handle by its dot below the line
//! magnifies nothing (see [`loupe_target_for_drag`]).

#![allow(non_snake_case)]

use std::cell::RefCell;
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
/// Center magnification of the lens.
pub const LOUPE_MAGNIFICATION: f32 = 1.7;
/// Scale of the bubble at its birth (reference: the first visible frame is
/// already ~half the steady size).
const LOUPE_MIN_SCALE: f32 = 0.5;
/// Delay between the grab and the bubble's birth (reference: the menu
/// dissolves first; the bubble appears ~120 ms after the touch-down). A grab
/// released within the delay never shows a loupe.
const LOUPE_BIRTH_DELAY_MS: u64 = 120;
/// How far below the line bottom (in line heights) the finger still counts
/// as covering the line. The end/cursor dot's center sits ~0.29 line heights
/// below the bottom (16 dp dot on a 20 dp line), so a dot-center grab falls
/// outside this margin and shows no loupe — the measured behavior.
const LOUPE_LINE_GRAB_MARGIN: f32 = 0.15;

/// Grow-in: ζ≈0.63 → +8% overshoot peaking at ~200 ms after the birth,
/// settled in ~360 ms — the measured inflate.
fn loupe_grow_spring() -> AnimationType {
    spring(0.63, 310.0)
}

/// The birth-delay gate: a linear timer from the grab to the bubble's birth.
fn loupe_birth_gate() -> AnimationType {
    AnimationType::Tween(AnimationSpec::linear(LOUPE_BIRTH_DELAY_MS))
}

/// Release: the measured dissolve is a ~55 ms linear ramp — the bubble
/// shrinks mildly (to ~72%) and sinks a touch, then vanishes on a terminal
/// alpha cliff (the unmount below).
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
        })
    } else {
        None
    }
}

/// Animated loupe state living across recompositions: grow/collapse progress
/// plus the horizontal follow, and the frozen target while dissolving.
struct LoupeState {
    progress: RefCell<Animatable<f32>>,
    /// Birth-delay timer: runs 0→1 over [`LOUPE_BIRTH_DELAY_MS`] from the
    /// grab; the grow starts only once it completes.
    gate: RefCell<Animatable<f32>>,
    follow_x: RefCell<Animatable<f32>>,
    /// The last target shown; kept while the bubble deflates after release.
    shown: RefCell<Option<LoupeTarget>>,
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
            follow_x: RefCell::new(Animatable::new(0.0, runtime)),
            shown: RefCell::new(None),
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
        }
        // The grow starts only once the birth gate elapses (reading the gate
        // state subscribes this composition to its frames, so the flip
        // recomposes even during a motionless hold).
        let born = state.gate.borrow().state().value() >= 1.0;
        if born {
            let mut progress = state.progress.borrow_mut();
            if (progress.target() - 1.0).abs() > f32::EPSILON {
                progress.animateTo(1.0, loupe_grow_spring());
            }
        }
    } else {
        let mut progress = state.progress.borrow_mut();
        if progress.target() != 0.0 {
            progress.animateTo(0.0, loupe_collapse_tween());
        }
    }
    state.was_active.set(active);

    let progress_state = state.progress.borrow().state();
    let follow_state = state.follow_x.borrow().state();
    let p = progress_state.value().max(0.0);
    let Some(shown) = *state.shown.borrow() else {
        return;
    };
    if p <= 0.001 {
        // Not yet born (birth delay) — or a grab released inside the delay,
        // which never shows a loupe at all.
        if !active {
            state.shown.replace(None);
        }
        return;
    }
    if !active && p <= 0.1 {
        // Fully deflated: unmount until the next grab.
        state.shown.replace(None);
        return;
    }

    // Geometry driven by the grow progress: the bubble inflates out of the
    // grab point on the line, rising to its floating offset. The overshoot
    // (p briefly > 1) carries both the size and the rise, like the
    // reference's 378-px peak. Two reference details on top of the raw
    // spring: the newborn bubble is nearly ROUND and widens into the capsule
    // as it grows, and the rise outruns the inflation (the bubble is at
    // floating height while still filling out).
    // Two phases share the progress value but map it differently — the
    // measured grow and dissolve are different curves, not mirrors:
    // * grow: inflate from ~half size (round → capsule as it fills out),
    //   the rise outrunning the inflation;
    // * dissolve: a mild shrink toward ~72% with a slight sink, vanishing
    //   on the terminal cliff (the unmount above) like the reference.
    let (scale, aspect_t, rise_t) = if active {
        let pc = p.min(1.0);
        (
            LOUPE_MIN_SCALE + (1.0 - LOUPE_MIN_SCALE) * p,
            pc * pc,
            (1.0 - (1.0 - pc).powi(2)) + (p - pc), // ease-out + spring overshoot
        )
    } else {
        (0.72 + 0.28 * p, 1.0, 0.76 + 0.24 * p)
    };
    let height = LOUPE_HEIGHT * scale;
    let aspect = 1.08 + (LOUPE_WIDTH / LOUPE_HEIGHT - 1.08) * aspect_t;
    let width = height * aspect;
    let center_x = follow_state.value();
    let center_y = shown.line_mid_y - LOUPE_RISE * rise_t;
    // The lens looks at the line (the focus), which sits below the risen
    // center — at p=0 the focus is the bubble itself, so the newborn bubble
    // shows the unmagnified text it grows out of.
    let focus_offset_y = shown.line_mid_y - center_y;

    let spec = LiquidLoupeSpec {
        magnification: LOUPE_MAGNIFICATION,
        focus_offset: (0.0, focus_offset_y),
        progress: p.min(1.0),
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
                    shape: LayerShape::Rounded(RoundedCornerShape::uniform(1.0e6)),
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
    fn loupe_effect_scales_its_optics_with_grow_progress() {
        // The reference bubble is a fixed-optic lens whose SHAPE grows: it is
        // born already mostly magnified with a visible rim and dispersion,
        // and only the residue ramps in with the inflation.
        let newborn = LiquidLoupeSpec {
            progress: 0.0,
            ..LiquidLoupeSpec::default()
        };
        let effect = liquid_loupe_effect((LOUPE_WIDTH, LOUPE_HEIGHT), &newborn);
        let cranpose_ui_graphics::RenderEffect::Shader { shader } = effect else {
            panic!("loupe must be a bare shader effect");
        };
        let u = shader.uniforms();
        let newborn_mag = 1.0 + (LOUPE_MAGNIFICATION - 1.0) * 0.85;
        assert!(
            (u[83] - newborn_mag).abs() < 1e-3,
            "newborn magnification is ~85% of full, got {}",
            u[83]
        );
        assert!(u[86] > 0.0, "newborn dispersion already visible");

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
