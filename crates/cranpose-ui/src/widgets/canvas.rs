//! Canvas composable for custom drawing via [`DrawScope`].
//!
//! Matches Jetpack Compose's `Canvas(modifier) { onDraw }`.

use cranpose_core::NodeId;
use cranpose_ui_graphics::{DrawScope, Size};

use crate::{composable, layout::policies::LeafMeasurePolicy, modifier::Modifier, widgets::Layout};

/// Draws custom content through a [`DrawScope`]. The modifier sets canvas bounds.
/// The draw scope also measures and draws text with the app's fonts.
///
/// # Example
///
/// ```rust
/// use cranpose_ui::*;
///
/// #[composable]
/// fn ColorPanel() {
///     Canvas(Modifier::empty().size_points(200.0, 100.0), |scope| {
///         scope.draw_rect(Brush::solid(Color::RED));
///     });
/// }
/// ```
///
/// For text, use `DrawTextStyle`, `DrawScope::measure_text` and
/// `DrawScope::draw_text_from` to place a label within the canvas.
#[composable]
pub fn Canvas(modifier: Modifier, on_draw: impl Fn(&mut dyn DrawScope) + 'static) -> NodeId {
    let draw_modifier = modifier.draw_behind(on_draw);
    Layout(draw_modifier, LeafMeasurePolicy::new(Size::ZERO), || {})
}

#[cfg(test)]
#[path = "tests/canvas_tests.rs"]
mod tests;
