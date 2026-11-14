// The design's stroked icons (24 x 24 view box): home, search, cart, back,
// and the quantity stepper's plus and minus.

use std::f32::consts::PI;

use cranpose::{
    Brush, Canvas, Color, DrawScope, DrawStyle, Modifier, Path, Point, Stroke, StrokeCap,
    StrokeJoin, composable,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IconKind {
    Home,
    Search,
    Cart,
    Back,
    Plus,
    Minus,
}

#[composable]
pub(crate) fn StrokeIcon(kind: IconKind, size: f32, line_width: f32, color: Color) {
    Canvas(Modifier::empty().size_points(size, size), move |scope| {
        draw(scope, kind, size / 24.0, line_width, color);
    });
}

fn draw(scope: &mut dyn DrawScope, kind: IconKind, scale: f32, line_width: f32, color: Color) {
    let point = |x: f32, y: f32| Point::new(x * scale, y * scale);
    let stroke = Stroke::new(line_width)
        .with_cap(StrokeCap::Round)
        .with_join(StrokeJoin::Round);
    let brush = Brush::solid(color);
    let mut path = Path::new();
    match kind {
        // M3 10.5 12 3l9 7.5V20a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1z
        IconKind::Home => {
            path.move_to(point(3.0, 10.5));
            path.line_to(point(12.0, 3.0));
            path.line_to(point(21.0, 10.5));
            path.line_to(point(21.0, 20.0));
            path.quadratic_to(point(21.0, 21.0), point(20.0, 21.0));
            path.line_to(point(15.0, 21.0));
            path.line_to(point(15.0, 15.0));
            path.line_to(point(9.0, 15.0));
            path.line_to(point(9.0, 21.0));
            path.line_to(point(4.0, 21.0));
            path.quadratic_to(point(3.0, 21.0), point(3.0, 20.0));
            path.close();
        }
        // A circle at (11, 11) of radius 7, then m20 20-3.5-3.5
        IconKind::Search => {
            scope.draw_circle_stroked(brush.clone(), point(11.0, 11.0), 7.0 * scale, stroke);
            path.move_to(point(20.0, 20.0));
            path.line_to(point(16.5, 16.5));
        }
        // M6 7h12l-1 13H7z, then the handle M9 7a3 3 0 0 1 6 0
        IconKind::Cart => {
            path.move_to(point(6.0, 7.0));
            path.line_to(point(18.0, 7.0));
            path.line_to(point(17.0, 20.0));
            path.line_to(point(7.0, 20.0));
            path.close();
            scope.draw_arc(brush.clone(), point(12.0, 7.0), 3.0 * scale, PI, PI, stroke);
        }
        // m15 5-7 7 7 7
        IconKind::Back => {
            path.move_to(point(15.0, 5.0));
            path.line_to(point(8.0, 12.0));
            path.line_to(point(15.0, 19.0));
        }
        // M12 5v14M5 12h14
        IconKind::Plus => {
            path.move_to(point(12.0, 5.0));
            path.line_to(point(12.0, 19.0));
            path.move_to(point(5.0, 12.0));
            path.line_to(point(19.0, 12.0));
        }
        // M5 12h14
        IconKind::Minus => {
            path.move_to(point(5.0, 12.0));
            path.line_to(point(19.0, 12.0));
        }
    }
    scope.draw_path(&path, brush, DrawStyle::Stroke(stroke));
}
