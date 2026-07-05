//! Progress indicators following Jetpack Compose's
//! `androidx.compose.material3.CircularProgressIndicator` and
//! `LinearProgressIndicator` (indeterminate variants).
//!
//! The circular indicator draws an arc that continuously sweeps around a
//! circle: the arc rotates at a constant speed while its sweep angle grows
//! and shrinks, driven by [`rememberInfiniteTransition`]. The arc itself is
//! rendered as a filled annular sector via [`VectorPath`] (there is no stroke
//! primitive in the draw pipeline).

#![allow(non_snake_case)]

use crate::composable;
use crate::modifier::Modifier;
use crate::widgets::Canvas;
use cranpose_animation::{
    infiniteRepeatable, rememberInfiniteTransition, AnimationSpec, Easing, RepeatMode, StartOffset,
};
use cranpose_core::NodeId;
use cranpose_ui_graphics::{Brush, Color, Rect, VectorPath};

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
/// [`MIN_SWEEP_DEGREES`] and [`MAX_SWEEP_DEGREES`].
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

    let sized = modifier.size_points(CIRCULAR_INDICATOR_DIAMETER, CIRCULAR_INDICATOR_DIAMETER);
    Canvas(sized, move |scope| {
        let size = scope.size();
        // Start at 12 o'clock like Compose (0 degrees points right in
        // screen coordinates, so shift back by 90 degrees).
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

    let sized = modifier.size_points(LINEAR_INDICATOR_WIDTH, LINEAR_INDICATOR_HEIGHT);
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
    // Cap the sweep just below a full turn so the arc endpoints never
    // coincide (a 360-degree SVG arc collapses to nothing).
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
mod tests {
    use super::*;
    use cranpose_core::{location_key, Composition, DefaultScheduler, MemoryApplier, Runtime};
    use std::sync::Arc;

    fn with_test_runtime<T>(f: impl FnOnce() -> T) -> T {
        let _runtime = Runtime::new(Arc::new(DefaultScheduler));
        f()
    }

    #[test]
    fn circular_arc_path_parses_and_stays_in_bounds() {
        for rotation in [0.0_f32, 45.0, 90.0, 200.0, 355.0] {
            for sweep in [MIN_SWEEP_DEGREES, 120.0, MAX_SWEEP_DEGREES] {
                let data = circular_arc_path_data(
                    CIRCULAR_INDICATOR_DIAMETER,
                    CIRCULAR_INDICATOR_DIAMETER,
                    CIRCULAR_INDICATOR_STROKE_WIDTH,
                    rotation - 90.0,
                    sweep,
                )
                .expect("arc path data");
                let path = VectorPath::parse(&data).expect("valid SVG arc path");
                assert!(!path.is_empty(), "arc path must produce geometry");
                let bounds = path.bounds();
                let eps = 0.51; // arc flattening tolerance
                assert!(
                    bounds.x >= -eps
                        && bounds.y >= -eps
                        && bounds.x + bounds.width <= CIRCULAR_INDICATOR_DIAMETER + eps
                        && bounds.y + bounds.height <= CIRCULAR_INDICATOR_DIAMETER + eps,
                    "arc (rotation {rotation}, sweep {sweep}) escapes indicator bounds: {bounds:?}"
                );
            }
        }
    }

    #[test]
    fn circular_arc_path_rotates_with_angle() {
        let at = |start: f32| {
            circular_arc_path_data(20.0, 20.0, 2.0, start, 120.0).expect("arc path data")
        };
        assert_ne!(at(0.0), at(90.0), "rotation must move the arc");
    }

    #[test]
    fn circular_arc_path_rejects_degenerate_input() {
        assert!(circular_arc_path_data(0.0, 0.0, 2.0, 0.0, 120.0).is_none());
        assert!(circular_arc_path_data(20.0, 20.0, 2.0, 0.0, 0.0).is_none());
    }

    #[test]
    fn linear_band_stays_inside_track() {
        let width = 200.0;
        let mut seen_band = false;
        for step in 0..=20 {
            let phase = step as f32 / 20.0;
            if let Some((x, band_width)) = linear_indicator_band(width, phase) {
                seen_band = true;
                assert!(x >= 0.0, "band start below 0 at phase {phase}");
                assert!(
                    x + band_width <= width + 1e-3,
                    "band escapes track at phase {phase}"
                );
                assert!(band_width > 0.0);
            }
        }
        assert!(seen_band, "band must be visible for mid phases");
        // Fully off-track at both extremes.
        assert!(linear_indicator_band(width, 0.0).is_none());
        assert!(linear_indicator_band(width, 1.0).is_none());
    }

    #[test]
    fn circular_progress_indicator_composes() {
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let mut composition = Composition::new(MemoryApplier::new());
            let result = composition.render(location_key(file!(), line!(), column!()), || {
                CircularProgressIndicator(
                    Modifier::empty(),
                    PROGRESS_INDICATOR_COLOR,
                    CIRCULAR_INDICATOR_STROKE_WIDTH,
                );
            });
            assert!(result.is_ok());
            assert!(composition.root().is_some());
        });
    }

    #[test]
    fn linear_progress_indicator_composes() {
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let mut composition = Composition::new(MemoryApplier::new());
            let result = composition.render(location_key(file!(), line!(), column!()), || {
                LinearProgressIndicator(Modifier::empty(), PROGRESS_INDICATOR_COLOR);
            });
            assert!(result.is_ok());
            assert!(composition.root().is_some());
        });
    }

    /// The spinner's draw output must change as its infinite transition is
    /// ticked by the frame clock: mount the widget, capture the Canvas draw
    /// primitives, advance the animation clock, and require different
    /// primitives from the same draw closure.
    #[test]
    fn circular_progress_indicator_animates_transition() {
        use crate::layout::MeasureLayoutOptions;
        use crate::measure_layout_with_options;

        let _app_context = crate::render_state::app_context_test_scope();
        let mut composition = Composition::new(MemoryApplier::new());
        composition
            .render(location_key(file!(), line!(), column!()), || {
                CircularProgressIndicator(
                    Modifier::empty(),
                    PROGRESS_INDICATOR_COLOR,
                    CIRCULAR_INDICATOR_STROKE_WIDTH,
                );
            })
            .expect("initial render");

        // Collect the spinner's draw commands from the laid-out tree.
        let root = composition.root().expect("composition root");
        let handle = composition.runtime_handle();

        fn collect_draw_commands(
            node: &crate::LayoutBox,
            out: &mut Vec<(crate::DrawCommand, crate::modifier::Size)>,
        ) {
            for command in node.node_data.modifier_slices().draw_commands() {
                out.push((
                    command.clone(),
                    crate::modifier::Size {
                        width: node.rect.width,
                        height: node.rect.height,
                    },
                ));
            }
            for child in &node.children {
                collect_draw_commands(child, out);
            }
        }

        let commands = {
            let mut applier = composition.applier_mut();
            applier.set_runtime_handle(handle.clone());
            let measurements = measure_layout_with_options(
                &mut applier,
                root,
                crate::Size::new(200.0, 200.0),
                MeasureLayoutOptions {
                    collect_semantics: false,
                    build_layout_tree: true,
                },
            )
            .expect("measure spinner layout");
            applier.clear_runtime_handle();

            let tree = measurements.layout_tree().expect("layout tree");
            let mut commands = Vec::new();
            collect_draw_commands(tree.root(), &mut commands);
            commands
        };
        assert!(!commands.is_empty(), "spinner must register draw commands");

        let run_commands = |commands: &[(crate::DrawCommand, crate::modifier::Size)]| {
            commands
                .iter()
                .flat_map(|(command, size)| match command {
                    crate::DrawCommand::Behind(func) => func(*size),
                    crate::DrawCommand::Overlay(func) => func(*size),
                    crate::DrawCommand::WithContent(func) => func(*size),
                })
                .collect::<Vec<_>>()
        };

        let before = run_commands(&commands);
        assert!(
            !before.is_empty(),
            "spinner draw closure must emit primitives"
        );

        // Advance the animation clock by a few frames (~1/3 of a rotation).
        let mut time = 0u64;
        for _ in 0..30 {
            time += 16_666_667;
            handle.drain_frame_callbacks(time);
            composition
                .process_invalid_scopes()
                .expect("process invalid scopes");
        }

        let after = run_commands(&commands);
        assert_ne!(
            before, after,
            "spinner draw primitives must change as the transition animates"
        );
    }
}
