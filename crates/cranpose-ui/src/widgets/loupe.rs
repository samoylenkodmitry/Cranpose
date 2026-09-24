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
const LOUPE_COLLAPSE_MS: u64 = 120;
fn loupe_grow_spring() -> AnimationType {
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
    follow_x: RefCell<Animatable<f32>>,
    shown: RefCell<Option<LoupeTarget>>,
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

    {
        let mut follow_anim = state.follow_x.borrow_mut();
        if !follow_anim.state().value().is_finite() {
            follow_anim.snapTo(shown.focus_x);
        } else if (follow_anim.target() - shown.focus_x).abs() > f32::EPSILON {
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
#[path = "tests/loupe_tests.rs"]
mod tests;
