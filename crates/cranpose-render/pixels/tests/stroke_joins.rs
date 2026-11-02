// A translucent stroked path paints each pixel it covers once, as Compose
// strokes a path: its joins are no darker than its segments.

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_render_pixels::PixelsRenderer;
use cranpose_ui::{Box, BoxSpec, Modifier};
use cranpose_ui_graphics::{Brush, Color, DrawStyle, Path, Point, Stroke, StrokeCap, StrokeJoin};

const SIDE: u32 = 120;

thread_local! {
    static STROKE: std::cell::Cell<Stroke> = const { std::cell::Cell::new(Stroke::new(1.0)) };
    static POINTS: std::cell::RefCell<Vec<Point>> = const { std::cell::RefCell::new(Vec::new()) };
}

// The pixel at (`x`, `y`) of a white square on which a half transparent
// blue polyline through `points` is stroked with `stroke`.
fn stroked_path_pixel(points: &[Point], stroke: Stroke, (x, y): (u32, u32)) -> [u8; 4] {
    STROKE.with(|cell| cell.set(stroke));
    POINTS.with(|cell| *cell.borrow_mut() = points.to_vec());
    let mut shell = AppShell::new_with_size(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        || {
            Box(
                Modifier::empty().fill_max_size().draw_behind(|scope| {
                    scope.draw_rect(Brush::solid(Color::WHITE));
                    let mut path = Path::new();
                    POINTS.with(|points| {
                        let points = points.borrow();
                        path.move_to(points[0]);
                        for &point in &points[1..] {
                            path.line_to(point);
                        }
                    });
                    scope.draw_path(
                        &path,
                        Brush::solid(Color(0.0, 0.0, 1.0, 0.5)),
                        DrawStyle::Stroke(STROKE.with(std::cell::Cell::get)),
                    );
                }),
                BoxSpec::default(),
                || {},
            );
        },
        (SIDE, SIDE),
        (SIDE as f32, SIDE as f32),
    );
    shell.update();
    let mut frame = vec![0u8; (SIDE * SIDE * 4) as usize];
    shell
        .primary()
        .with_renderer(|renderer| renderer.draw(&mut frame, SIDE, SIDE));
    let at = ((y * SIDE + x) * 4) as usize;
    [frame[at], frame[at + 1], frame[at + 2], frame[at + 3]]
}

// Strokes a V with its corner at (60, 100) and compares the corner, which
// both of its edges cover, with the middle of the first edge.
fn assert_join_matches_segment(stroke: Stroke) {
    let v = [
        Point::new(20.0, 30.0),
        Point::new(60.0, 100.0),
        Point::new(100.0, 30.0),
    ];
    let segment = stroked_path_pixel(&v, stroke, (40, 65));
    let corner = stroked_path_pixel(&v, stroke, (60, 98));
    assert_eq!(
        corner, segment,
        "the corner was painted again: {corner:?} against {segment:?}"
    );
}

#[test]
fn a_translucent_mitered_path_paints_its_corner_once() {
    assert_join_matches_segment(Stroke::new(12.0));
}

#[test]
fn a_translucent_rounded_path_paints_its_corner_once() {
    assert_join_matches_segment(
        Stroke::new(12.0)
            .with_join(StrokeJoin::Round)
            .with_cap(StrokeCap::Round),
    );
}

#[test]
fn a_translucent_beveled_path_paints_its_corner_once() {
    assert_join_matches_segment(Stroke::new(12.0).with_join(StrokeJoin::Bevel));
}

// A corner whose edges run across and down: a fill of the outline there
// cannot be cut into slices, and the stroke falls back to a coverage mask,
// which paints the corner once as well.
#[test]
fn a_translucent_right_angle_paints_its_corner_once() {
    let pixel = |at| {
        stroked_path_pixel(
            &[
                Point::new(20.0, 20.0),
                Point::new(100.0, 20.0),
                Point::new(100.0, 100.0),
            ],
            Stroke::new(12.0),
            at,
        )
    };
    let segment = pixel((60, 20));
    let corner = pixel((100, 20));
    assert_eq!(
        corner, segment,
        "the corner was painted again: {corner:?} against {segment:?}"
    );
}
