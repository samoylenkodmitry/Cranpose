//! Progress indicators following Jetpack Compose's
//! `androidx.compose.material3.CircularProgressIndicator` and
//! `LinearProgressIndicator` (indeterminate variants).
//!
//! The circular indicator draws an arc that continuously sweeps around a
//! circle: the arc rotates at a constant speed while its sweep angle grows
//! and shrinks, driven by [`rememberInfiniteTransition`]. The arc itself is
//! rendered as a filled annular sector via [`VectorPath`] (there is no stroke
//! primitive in the draw pipeline).

use cranpose_animation::{
    AnimationSpec, Easing, RepeatMode, StartOffset, infiniteRepeatable, rememberInfiniteTransition,
};
use cranpose_core::NodeId;
use cranpose_ui_graphics::{Brush, Color, Rect, VectorPath};

use crate::{composable, modifier::Modifier, widgets::Canvas};

/// Default diameter of [`CircularProgressIndicator`] in dp.
pub const CIRCULAR_INDICATOR_DIAMETER: f32 = 20.0;

/// Default stroke width of [`CircularProgressIndicator`] in dp.
///
/// Matches the Material proportion (4dp stroke at 40dp diameter).
pub const CIRCULAR_INDICATOR_STROKE_WIDTH: f32 = 2.0;

/// Default color for progress indicators (Material blue).
pub const PROGRESS_INDICATOR_COLOR: Color = Color(0.101, 0.462, 0.909, 1.0);

/// Default size of [`LinearProgressIndicator`] in dp.
pub const LINEAR_INDICATOR_WIDTH: f32 = 240.0;
/// Default height of [`LinearProgressIndicator`] in dp.
pub const LINEAR_INDICATOR_HEIGHT: f32 = 4.0;

/// Duration of one full rotation of the circular indicator, in ms.
const ROTATION_DURATION_MS: u64 = 1332;
/// Duration of one grow/shrink cycle of the arc sweep, in ms.
const SWEEP_DURATION_MS: u64 = 666;
/// Minimum sweep of the arc in degrees.
const MIN_SWEEP_DEGREES: f32 = 30.0;
/// Maximum sweep of the arc in degrees.
const MAX_SWEEP_DEGREES: f32 = 270.0;
/// Duration of one slide of the linear indicator band, in ms.
const LINEAR_SLIDE_DURATION_MS: u64 = 1200;
/// Fraction of the track occupied by the moving band.
const LINEAR_BAND_FRACTION: f32 = 0.4;
/// Track alpha relative to the indicator color.
const LINEAR_TRACK_ALPHA: f32 = 0.24;

/// An indeterminate circular progress indicator (spinner).
///
/// Follows Jetpack Compose's `CircularProgressIndicator`: an arc sweeps
/// around a circle forever, rotating while its length pulses between
/// `MIN_SWEEP_DEGREES` and `MAX_SWEEP_DEGREES`.
///
/// # Arguments
///
/// * `modifier` - Modifiers for styling and layout. The indicator applies a
///   default size of [`CIRCULAR_INDICATOR_DIAMETER`] dp which outer size
///   modifiers can override.
/// * `color` - Arc color (see [`PROGRESS_INDICATOR_COLOR`] for the default).
/// * `stroke_width` - Arc thickness in dp
///   (see [`CIRCULAR_INDICATOR_STROKE_WIDTH`] for the default).
///
/// # Example
///
/// ```rust,ignore
/// CircularProgressIndicator(
///     Modifier::empty(),
///     PROGRESS_INDICATOR_COLOR,
///     CIRCULAR_INDICATOR_STROKE_WIDTH,
/// );
/// ```
#[composable]
pub fn CircularProgressIndicator(modifier: Modifier, color: Color, stroke_width: f32) -> NodeId {
    let transition = rememberInfiniteTransition("circular_progress_indicator");
    let rotation = transition.animateFloat(
        0.0,
        360.0,
        infiniteRepeatable(
            AnimationSpec::linear(ROTATION_DURATION_MS),
            RepeatMode::Restart,
            StartOffset::default(),
        ),
        "circular_progress_rotation",
    );
    let sweep = transition.animateFloat(
        MIN_SWEEP_DEGREES,
        MAX_SWEEP_DEGREES,
        infiniteRepeatable(
            AnimationSpec::tween(SWEEP_DURATION_MS, Easing::EaseInOut),
            RepeatMode::Reverse,
            StartOffset::default(),
        ),
        "circular_progress_sweep",
    );

    let sized = modifier
        .size_points(CIRCULAR_INDICATOR_DIAMETER, CIRCULAR_INDICATOR_DIAMETER)
        .semantics(busy_semantics);
    Canvas(sized, move |scope| {
        let size = scope.size();
        let start_angle = rotation.get() - 90.0;
        let sweep_angle = sweep.get();
        if let Some(data) = circular_arc_path_data(
            size.width,
            size.height,
            stroke_width,
            start_angle,
            sweep_angle,
        ) {
            if let Ok(path) = VectorPath::parse(&data) {
                scope.draw_vector_path(&path, Brush::solid(color));
            }
        }
    })
}

/// An indeterminate linear progress indicator.
///
/// A band slides repeatedly across a dimmed track, following Jetpack
/// Compose's `LinearProgressIndicator` (simplified single-band variant).
///
/// # Arguments
///
/// * `modifier` - Modifiers for styling and layout. The indicator applies a
///   default size of [`LINEAR_INDICATOR_WIDTH`] x [`LINEAR_INDICATOR_HEIGHT`]
///   dp which outer size modifiers can override.
/// * `color` - Band color; the track uses the same color dimmed.
#[composable]
pub fn LinearProgressIndicator(modifier: Modifier, color: Color) -> NodeId {
    let transition = rememberInfiniteTransition("linear_progress_indicator");
    let phase = transition.animateFloat(
        0.0,
        1.0,
        infiniteRepeatable(
            AnimationSpec::tween(LINEAR_SLIDE_DURATION_MS, Easing::FastOutSlowInEasing),
            RepeatMode::Restart,
            StartOffset::default(),
        ),
        "linear_progress_phase",
    );

    let sized = modifier
        .size_points(LINEAR_INDICATOR_WIDTH, LINEAR_INDICATOR_HEIGHT)
        .semantics(busy_semantics);
    Canvas(sized, move |scope| {
        let size = scope.size();
        let track = Color(color.0, color.1, color.2, color.3 * LINEAR_TRACK_ALPHA);
        scope.draw_rect(Brush::solid(track));
        if let Some((x, width)) = linear_indicator_band(size.width, phase.get()) {
            scope.draw_rect_at(
                Rect {
                    x,
                    y: 0.0,
                    width,
                    height: size.height,
                },
                Brush::solid(color),
            );
        }
    })
}

/// Builds SVG path data for a filled annular arc (donut segment) centered in
/// a `width` x `height` box.
///
/// Angles are in degrees; 0 degrees points right (+X) and angles grow
/// clockwise in screen coordinates. Returns `None` when there is nothing to
/// draw (degenerate size or sweep).
pub(crate) fn circular_arc_path_data(
    width: f32,
    height: f32,
    stroke_width: f32,
    start_angle_deg: f32,
    sweep_angle_deg: f32,
) -> Option<String> {
    let outer_r = width.min(height) * 0.5;
    if outer_r <= 0.0 {
        return None;
    }
    let sweep = sweep_angle_deg.clamp(0.0, 359.9);
    if sweep <= 0.0 {
        return None;
    }
    let stroke = stroke_width.clamp(0.1, outer_r);
    let inner_r = (outer_r - stroke).max(0.0);
    let cx = width * 0.5;
    let cy = height * 0.5;
    let a0 = start_angle_deg.to_radians();
    let a1 = (start_angle_deg + sweep).to_radians();
    let (ox0, oy0) = (cx + outer_r * a0.cos(), cy + outer_r * a0.sin());
    let (ox1, oy1) = (cx + outer_r * a1.cos(), cy + outer_r * a1.sin());
    let (ix0, iy0) = (cx + inner_r * a0.cos(), cy + inner_r * a0.sin());
    let (ix1, iy1) = (cx + inner_r * a1.cos(), cy + inner_r * a1.sin());
    let large_arc = if sweep > 180.0 { 1 } else { 0 };
    Some(format!(
        "M {ox0:.4} {oy0:.4} \
         A {outer_r:.4} {outer_r:.4} 0 {large_arc} 1 {ox1:.4} {oy1:.4} \
         L {ix1:.4} {iy1:.4} \
         A {inner_r:.4} {inner_r:.4} 0 {large_arc} 0 {ix0:.4} {iy0:.4} Z"
    ))
}

/// Returns `(x, width)` of the indeterminate linear band clamped inside
/// `[0, width]`, or `None` when the band is fully off-track.
///
/// `phase` runs from 0.0 (band fully off the left edge) to 1.0 (band fully
/// off the right edge).
pub(crate) fn linear_indicator_band(width: f32, phase: f32) -> Option<(f32, f32)> {
    if width <= 0.0 {
        return None;
    }
    let band_width = width * LINEAR_BAND_FRACTION;
    let x = phase * (width + band_width) - band_width;
    let x0 = x.max(0.0);
    let x1 = (x + band_width).min(width);
    (x1 > x0).then_some((x0, x1 - x0))
}

#[cfg(test)]
#[path = "tests/progress_indicator_tests.rs"]
mod tests;

/// What a screen reader says at an indicator with no value: that the app is
/// busy. Compose's `progressSemantics()` with no arguments does the same, and
/// without it a spinner is a silent drawing a blind user walks past.
fn busy_semantics(config: &mut cranpose_foundation::SemanticsConfiguration) {
    config.content_description = Some("Loading".into());
    config.role = Some(cranpose_foundation::SemanticsWidgetRole::ProgressBar);
}

#[cfg(test)]
#[path = "tests/progress_indicator_busy_semantics_tests.rs"]
mod busy_semantics_tests;
