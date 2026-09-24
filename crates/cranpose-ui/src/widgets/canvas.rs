//! Canvas composable for custom drawing via [`DrawScope`].
//!
//! Matches Jetpack Compose's `Canvas(modifier) { onDraw }`.

use cranpose_core::NodeId;
use cranpose_ui_graphics::{DrawScope, Size};

use crate::{composable, layout::policies::LeafMeasurePolicy, modifier::Modifier, widgets::Layout};

/// A composable that draws custom content using a [`DrawScope`].
///
/// ```ignore
/// Canvas(
///     Modifier::empty().size_points(200.0, 100.0),
///     |scope| {
///         scope.draw_rect(Brush::solid(Color::RED));
///     },
/// );
/// ```
///
/// The scope draws text too, against the app's fonts and through the same
/// glyph atlas the `Text` composable uses — a canvas never needs its own font.
/// Alignment happens inside the box you pass, and
/// [`DrawScope::measure_text`] reports exactly the box a draw will fill, so a
/// caller can position text itself:
///
/// ```ignore
/// Canvas(Modifier::empty().fill_max_size(), |scope| {
///     let style = DrawTextStyle::new(24.0)
///         .with_weight(FontWeight::BOLD)
///         .with_align(TextAlign::Center);
///     // Centered in the whole canvas.
///     scope.draw_text(Brush::solid(Color::WHITE), "GAME OVER", &style);
///     // Or placed by hand from the measurement.
///     let measured = scope.measure_text("SCORE 1234", &style);
///     scope.draw_text_from(
///         Point::new(scope.size().width - measured.size.width - 8.0, 8.0),
///         Brush::solid(Color::WHITE),
///         "SCORE 1234",
///         &style,
///     );
/// });
/// ```
#[composable]
pub fn Canvas(modifier: Modifier, on_draw: impl Fn(&mut dyn DrawScope) + 'static) -> NodeId {
    let draw_modifier = modifier.draw_behind(on_draw);
    Layout(draw_modifier, LeafMeasurePolicy::new(Size::ZERO), || {})
}

#[cfg(test)]
#[path = "tests/canvas_tests.rs"]
mod tests;
