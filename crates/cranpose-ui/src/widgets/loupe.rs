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
//! * grow-in: born a near-square squircle — already ~92% of the final
//!   HEIGHT but only ~59% of the WIDTH — low over the line (~68% risen)
//!   with FULL optics (magnified text, dot and rim all present on the
//!   first visible frame). The width springs out to the capsule with a
//!   ~+6% overshoot peaking ~200 ms after birth; the rise eases out
//!   (τ ≈ 95 ms) with no overshoot;
//! * follow: every active frame uses the current finger x directly; only the
//!   vertical birth rise is animated. The y is locked to the grabbed line;
//! * release: the shell immediately redistributes into a taller near-circle,
//!   then shrinks with a slight sink, fading THROUGH its
//!   own optics. The sampled handle holds through the first redistribution,
//!   then contracts on its measured curve; the whole bubble is gone by
//!   ~55 ms.
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
const LOUPE_BIRTH_WIDTH_FRAC: f32 = 0.585;
const LOUPE_BIRTH_HEIGHT_FRAC: f32 = 0.915;
const LOUPE_BIRTH_RISE_FRAC: f32 = 0.68;
/// Dissolve: the slight fraction of [`LOUPE_RISE`] the bubble sinks toward
/// the line and the content-alpha floor it reaches near the terminal vanish.
/// Width returns toward the birth squircle ahead of height, then both axes
/// disappear together in the terminal beat.
const LOUPE_DISSOLVE_ALPHA_FLOOR: f32 = 0.05;
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
    spring(0.5, 270.0)
}

/// Rise: overdamped (no overshoot), an ~86 ms-τ ease-out — the reference top
/// edge climbs monotonically, half done ~60 ms after birth, and is flat by
/// the ~+300 ms mark (a 95 ms tail still crept visibly at +380).
fn loupe_rise_spring() -> AnimationType {
    spring(2.0, 2200.0)
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
    let height_progress = (p * 5.0).clamp(0.0, 1.0);
    let width_progress = if p <= 1.0 { p.max(0.0).powf(0.65) } else { p };
    LoupePose {
        width_frac: LOUPE_BIRTH_WIDTH_FRAC + (1.0 - LOUPE_BIRTH_WIDTH_FRAC) * width_progress,
        height_frac: LOUPE_BIRTH_HEIGHT_FRAC + (1.0 - LOUPE_BIRTH_HEIGHT_FRAC) * height_progress,
        rise_frac: LOUPE_BIRTH_RISE_FRAC + (1.0 - LOUPE_BIRTH_RISE_FRAC) * rise.clamp(0.0, 1.0),
    }
}

/// Dissolve mapping over `q` (1 at the release, 0 fully gone), RELATIVE to
/// the pose the bubble was released from — releasing a newborn shrinks the
/// newborn, not a full capsule. Returns the pose plus the CONTENT ALPHA
/// (the spec `progress`): the reference first redistributes width into height,
/// then collapses both axes toward the line.
/// The sampled optical image holds through the shell's initial redistribution,
/// then contracts on its independently measured dissolve curve.
fn dissolve_pose(released: LoupePose, q: f32) -> (LoupePose, f32) {
    let d = 1.0 - q.clamp(0.0, 1.0);
    let redistribute = smoothstep01(d / (8.0 / 55.0));
    let middle = smoothstep01((d - 8.0 / 55.0) / (17.0 / 55.0));
    let late = smoothstep01((d - 25.0 / 55.0) / (17.0 / 55.0));
    let terminal = smoothstep01((d - 42.0 / 55.0) / (13.0 / 55.0));
    let redistributed_w = 1.0 + (0.906 - 1.0) * redistribute;
    let redistributed_h = 1.0 + (1.155 - 1.0) * redistribute;
    let middle_w = redistributed_w + (0.632 - redistributed_w) * middle;
    let middle_h = redistributed_h + (0.902 - redistributed_h) * middle;
    let late_w = middle_w + (0.462 - middle_w) * late;
    let late_h = middle_h + (0.488 - middle_h) * late;
    let width_scale = late_w * (1.0 - terminal);
    let height_scale = late_h * (1.0 - terminal);
    let collapse = smoothstep01((d - 8.0 / 55.0) / (34.0 / 55.0));
    let pose = LoupePose {
        width_frac: released.width_frac * width_scale,
        height_frac: released.height_frac * height_scale,
        rise_frac: released.rise_frac * (1.0 - 0.15 * collapse) * (1.0 - terminal),
    };
    let fade = smoothstep01((d - 25.0 / 55.0) / (30.0 / 55.0));
    (pose, 1.0 - (1.0 - LOUPE_DISSOLVE_ALPHA_FLOOR) * fade)
}

fn dissolve_optic_magnification(released: LoupePose, pose: LoupePose) -> f32 {
    let width_scale = pose.width_frac / released.width_frac.max(1.0e-4);
    let optic_scale = if width_scale >= 0.906 {
        1.0
    } else if width_scale >= 0.632 {
        let phase = smoothstep01((width_scale - 0.632) / (0.906 - 0.632));
        0.79 + (1.0 - 0.79) * phase
    } else if width_scale >= 0.462 {
        let phase = smoothstep01((width_scale - 0.462) / (0.632 - 0.462));
        0.516 + (0.79 - 0.516) * phase
    } else {
        0.516 * width_scale / 0.462
    };
    (LOUPE_MAGNIFICATION * optic_scale).clamp(0.35, LOUPE_MAGNIFICATION)
}

fn smoothstep01(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Animated loupe state living across recompositions: grow/collapse progress
/// plus the rise, and the frozen target/pose while
/// dissolving.
struct LoupeState {
    progress: RefCell<Animatable<f32>>,
    /// Birth-delay timer: runs 0→1 over [`LOUPE_BIRTH_DELAY_MS`] from the
    /// grab; the grow starts only once it completes.
    gate: RefCell<Animatable<f32>>,
    /// Rise driver, 0→1 from the birth (separate from the width spring: the
    /// reference rise never overshoots while the width does).
    rise: RefCell<Animatable<f32>>,
    /// The last target shown; kept while the bubble deflates after release.
    shown: RefCell<Option<LoupeTarget>>,
    /// The pose and progress value the bubble was released from — the
    /// dissolve shrinks THAT pose, whatever its size (releasing a newborn
    /// must not snap it to a full capsule first).
    released: Cell<Option<(f32, LoupePose)>>,
    /// Whether the loupe was active on the previous recomposition.
    was_active: std::cell::Cell<bool>,
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
            gate: RefCell::new(Animatable::new(0.0, runtime.clone())),
            rise: RefCell::new(Animatable::new(0.0, runtime)),
            shown: RefCell::new(None),
            released: Cell::new(None),
            was_active: std::cell::Cell::new(false),
        })
    })
    .with(Rc::clone);

    let active = target.is_some();
    if let Some(t) = target {
        let fresh_grab = !state.was_active.get();
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
    let p = progress_state.value().max(0.0);
    let Some(shown) = *state.shown.borrow() else {
        return;
    };
    let born = active && state.gate.borrow().state().value() >= 1.0;
    if p <= 0.001 && !born && state.released.get().is_none() {
        // Not yet born (birth delay) — or a grab released inside the delay,
        // which never shows a loupe at all. Once BORN, p = 0 is the birth
        // pose itself (59% wide, full optics) and must render — the
        // reference's first frame is clearly visible.
        if !active {
            state.shown.replace(None);
        }
        return;
    }

    let (pose, optic, magnification) = if active {
        (grow_pose(p, rise_state.value()), 1.0, LOUPE_MAGNIFICATION)
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
        let (pose, optic) = dissolve_pose(released, q);
        let magnification = dissolve_optic_magnification(released, pose);
        (pose, optic, magnification)
    };

    let width = LOUPE_WIDTH * pose.width_frac;
    let height = LOUPE_HEIGHT * pose.height_frac;
    let center_x = shown.focus_x;
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
        magnification,
        focus_offset: (0.0, focus_offset_y),
        seam_lift: shown.dot_clearance,
        corner_radius,
        // Alpha fades the whole optical image; magnification contracts that
        // image with the dissolving shell.
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
        // Reference birth: ~59% width, ~92% height, ~68% risen — not a
        // half-scale miniature (the uniform-scale model read as a blob a
        // third of the final size).
        let pose = grow_pose(0.0, 0.0);
        assert!((pose.width_frac - LOUPE_BIRTH_WIDTH_FRAC).abs() < 1e-6);
        assert!((pose.height_frac - LOUPE_BIRTH_HEIGHT_FRAC).abs() < 1e-6);
        assert!((pose.rise_frac - LOUPE_BIRTH_RISE_FRAC).abs() < 1e-6);
        assert!(
            (0.66..=0.70).contains(&pose.rise_frac),
            "birth must sit low enough to leave a visible rise, got {}",
            pose.rise_frac
        );
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
    fn dissolve_holds_then_contracts_the_shell_and_optics_by_55ms() {
        let released = grow_pose(1.0, 1.0);
        let (pose, alpha) = dissolve_pose(released, 1.0 - 8.0 / 55.0);
        assert!((pose.width_frac - 0.906).abs() < 0.01);
        assert!((pose.height_frac - 1.155).abs() < 0.01);
        assert_eq!(alpha, 1.0);
        let magnification = dissolve_optic_magnification(released, pose);
        assert!((magnification - LOUPE_MAGNIFICATION).abs() < 1e-4);
        let (pose, alpha) = dissolve_pose(released, 1.0 - 25.0 / 55.0);
        assert!((0.62..0.65).contains(&pose.width_frac));
        assert!((0.89..0.92).contains(&pose.height_frac));
        assert!(alpha > 0.95);
        let magnification = dissolve_optic_magnification(released, pose);
        assert!(
            (magnification / LOUPE_MAGNIFICATION - 0.79).abs() < 0.02,
            "the sampled handle must retain 79% of its displayed size at 25ms, got {magnification}"
        );
        let (pose, alpha) = dissolve_pose(released, 1.0 - 42.0 / 55.0);
        assert!((0.45..0.48).contains(&pose.width_frac));
        assert!((0.47..0.51).contains(&pose.height_frac));
        assert!(alpha < 0.45);
        let magnification = dissolve_optic_magnification(released, pose);
        assert!(
            (magnification / LOUPE_MAGNIFICATION - 0.516).abs() < 0.02,
            "the sampled handle must retain 51.6% of its displayed size at 42ms, got {magnification}"
        );
        let (pose, _) = dissolve_pose(released, 0.0);
        assert!(pose.width_frac < 0.02 && pose.height_frac < 0.02);
    }

    #[test]
    fn grow_and_release_clocks_match_the_reference_timeline() {
        let AnimationType::Spring(grow) = loupe_grow_spring() else {
            panic!("loupe grow must retain spring overshoot");
        };
        assert_eq!(grow.damping_ratio, 0.5);
        assert!((240.0..=300.0).contains(&grow.stiffness));
        let AnimationType::Tween(collapse) = loupe_collapse_tween() else {
            panic!("loupe collapse must use the measured linear clock");
        };
        assert_eq!(collapse.duration_millis, 55);
    }

    #[test]
    fn dissolving_a_newborn_shrinks_the_newborn_pose() {
        // Regression: an absolute mapping snapped a newborn release to
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
        assert!(pose.width_frac > newborn.width_frac * 0.25);
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
        assert!(
            (0.18..=0.22).contains(&u[86]),
            "fold dispersion must stay within the measured 3-5px fringe, got {}",
            u[86]
        );
        assert!(u[11] >= 4.8, "neutral rim highlight is too weak: {}", u[11]);
        assert!(
            (0.58..=0.62).contains(&u[84]),
            "the legible fold must own the outer 40% of long-edge depth, got band start {}",
            u[84]
        );
        assert!(
            (u[90] - 0.65).abs() < 1e-6,
            "content alpha rides uniform 90"
        );
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
