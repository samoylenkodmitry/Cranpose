use cranpose_ui_graphics::{Brush, Color, DrawPrimitive, DrawScopeDefault, Rect, Size};

use super::*;

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
