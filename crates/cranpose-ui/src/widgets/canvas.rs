//! Canvas composable for custom drawing via [`DrawScope`].
//!
//! Matches Jetpack Compose's `Canvas(modifier) { onDraw }`.

#![allow(non_snake_case)]

use crate::composable;
use crate::layout::policies::LeafMeasurePolicy;
use crate::modifier::Modifier;
use crate::widgets::Layout;
use cranpose_core::NodeId;
use cranpose_ui_graphics::{DrawScope, Size};

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
#[composable]
pub fn Canvas(modifier: Modifier, on_draw: impl Fn(&mut dyn DrawScope) + 'static) -> NodeId {
    let draw_modifier = modifier.draw_behind(on_draw);
    Layout(draw_modifier, LeafMeasurePolicy::new(Size::ZERO), || {})
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_ui_graphics::{Brush, Color, DrawPrimitive, DrawScopeDefault, Rect, Size};

    #[test]
    fn canvas_on_draw_closure_produces_primitives() {
        let on_draw = |scope: &mut dyn DrawScope| {
            scope.draw_rect(Brush::solid(Color(1.0, 0.0, 0.0, 1.0)));
        };
        let mut scope = DrawScopeDefault::new(Size::new(50.0, 30.0));
        on_draw(&mut scope);
        let primitives = scope.into_primitives();
        assert_eq!(primitives.len(), 1);
        match &primitives[0] {
            DrawPrimitive::Rect { rect, .. } => {
                assert_eq!(*rect, Rect::from_size(Size::new(50.0, 30.0)));
            }
            other => panic!("expected rect, got {other:?}"),
        }
    }
}
