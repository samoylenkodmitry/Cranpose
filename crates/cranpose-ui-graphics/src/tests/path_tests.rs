use super::*;
use crate::{
    BlendMode, Brush, Color, DrawPrimitive, DrawScope, DrawScopeDefault, ImagePixelFormat, Size,
};

fn stroke(width: f32, cap: StrokeCap, join: StrokeJoin) -> Stroke {
    Stroke { width, cap, join }
}

fn lines(points: &[Point], closed: bool, stroke: Stroke) -> Vec<LineGeometry> {
    let mut out = Vec::new();
    for_each_stroke_line(points, closed, stroke, |line| out.push(line));
    out
}

#[test]
fn a_path_keeps_its_contours_and_whether_each_closes() {
    let mut path = Path::new();
    assert!(path.is_empty());
    path.move_to(Point::new(0.0, 0.0));
    path.line_to(Point::new(10.0, 0.0));
    path.line_to(Point::new(10.0, 10.0));
    path.close();
    path.line_to(Point::new(-5.0, 5.0));
    let contours = path.contours();
    assert_eq!(contours.len(), 2);
    assert_eq!(
        contours[0],
        PathContour {
            points: vec![
                Point::new(0.0, 0.0),
                Point::new(10.0, 0.0),
                Point::new(10.0, 10.0)
            ],
            closed: true,
        }
    );
    assert_eq!(
        contours[1].points,
        vec![Point::new(0.0, 0.0), Point::new(-5.0, 5.0)],
        "an edge after a close starts at the closed contour's first point"
    );
    assert!(!path.is_empty());
    assert_eq!(
        path.bounds(),
        Rect {
            x: -5.0,
            y: 0.0,
            width: 15.0,
            height: 10.0,
        }
    );
    path.reset();
    assert!(path.contours().is_empty());
    assert_eq!(path.bounds(), Rect::EMPTY);
}

#[test]
fn curves_flatten_to_points_on_the_curve() {
    let mut path = Path::new();
    path.move_to(Point::new(0.0, 0.0));
    path.quadratic_to(Point::new(50.0, 100.0), Point::new(100.0, 0.0));
    let quadratic = &path.contours()[0].points;
    assert!(quadratic.len() > 4, "a curve flattens to several edges");
    assert_eq!(quadratic.last(), Some(&Point::new(100.0, 0.0)));
    let peak = quadratic.iter().map(|point| point.y).fold(0.0, f32::max);
    assert!(
        (peak - 50.0).abs() < 1.0,
        "the quadratic peaks halfway to its control: {peak}"
    );

    let mut path = Path::new();
    path.move_to(Point::new(0.0, 0.0));
    path.cubic_to(
        Point::new(0.0, 60.0),
        Point::new(60.0, 60.0),
        Point::new(60.0, 0.0),
    );
    let cubic = &path.contours()[0].points;
    assert_eq!(cubic.first(), Some(&Point::new(0.0, 0.0)));
    assert_eq!(cubic.last(), Some(&Point::new(60.0, 0.0)));
    let peak = cubic.iter().map(|point| point.y).fold(0.0, f32::max);
    assert!(
        (peak - 45.0).abs() < 1.0,
        "the cubic peaks at three quarters: {peak}"
    );
}

#[test]
fn a_filled_path_closes_every_contour_into_a_vector_path() {
    let mut path = Path::new();
    path.move_to(Point::new(0.0, 0.0));
    path.line_to(Point::new(10.0, 0.0));
    path.line_to(Point::new(10.0, 10.0));
    let fill = path.to_vector_path(PathFillRule::EvenOdd);
    assert_eq!(fill.subpaths().len(), 1);
    assert_eq!(fill.fill_rule(), PathFillRule::EvenOdd);
    assert!(!fill.is_empty());
}

#[test]
fn dashes_follow_the_pattern_from_its_phase_across_corners() {
    let dash = DashPathEffect::new(&[4.0, 2.0], 5.0);
    assert_eq!(dash.intervals(), &[4.0, 2.0]);
    let mut dashes = Vec::new();
    dash.for_each_dash(
        &[
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 6.0),
        ],
        |piece| {
            dashes.push(piece.to_vec());
        },
    );
    assert_eq!(
        dashes,
        vec![
            vec![Point::new(1.0, 0.0), Point::new(5.0, 0.0)],
            vec![
                Point::new(7.0, 0.0),
                Point::new(10.0, 0.0),
                Point::new(10.0, 1.0)
            ],
            vec![Point::new(10.0, 3.0), Point::new(10.0, 6.0)],
        ],
        "five into the pattern the walk starts one unit into a gap; the middle dash turns the corner"
    );
}

#[test]
fn a_pattern_that_cannot_dash_draws_the_polyline_whole() {
    let points = [Point::new(0.0, 0.0), Point::new(10.0, 0.0)];
    for dash in [
        DashPathEffect::new(&[4.0], 0.0),
        DashPathEffect::new(&[0.0, 0.0], 0.0),
    ] {
        let mut pieces = Vec::new();
        dash.for_each_dash(&points, |piece| pieces.push(piece.to_vec()));
        assert_eq!(pieces, vec![points.to_vec()]);
    }
}

#[test]
fn a_round_stroke_draws_every_edge_with_round_ends() {
    let points = [
        Point::new(0.0, 0.0),
        Point::new(10.0, 0.0),
        Point::new(10.0, 10.0),
    ];
    let drawn = lines(
        &points,
        false,
        stroke(2.0, StrokeCap::Round, StrokeJoin::Round),
    );
    assert_eq!(drawn.len(), 2);
    assert!(drawn.iter().all(|line| line.cap == StrokeCap::Round));
    assert_eq!(
        (drawn[0].start, drawn[0].end),
        (Point::new(0.0, 0.0), Point::new(10.0, 0.0))
    );
}

#[test]
fn a_butt_capped_mitred_stroke_grows_its_inner_ends_by_half_the_width() {
    let points = [
        Point::new(0.0, 0.0),
        Point::new(10.0, 0.0),
        Point::new(10.0, 10.0),
    ];
    let drawn = lines(
        &points,
        false,
        stroke(2.0, StrokeCap::Butt, StrokeJoin::Miter),
    );
    assert_eq!(
        drawn
            .iter()
            .map(|line| (line.start, line.end, line.cap))
            .collect::<Vec<_>>(),
        vec![
            (Point::new(0.0, 0.0), Point::new(11.0, 0.0), StrokeCap::Butt),
            (
                Point::new(10.0, -1.0),
                Point::new(10.0, 10.0),
                StrokeCap::Butt
            ),
        ],
        "the corner is covered as a miter covers a right angle; the open ends stay butt"
    );
}

#[test]
fn a_round_join_on_a_butt_stroke_adds_a_dot_at_each_corner() {
    let points = [
        Point::new(0.0, 0.0),
        Point::new(10.0, 0.0),
        Point::new(10.0, 10.0),
    ];
    let drawn = lines(
        &points,
        false,
        stroke(2.0, StrokeCap::Butt, StrokeJoin::Round),
    );
    let dots: Vec<Point> = drawn
        .iter()
        .filter(|line| line.start == line.end)
        .map(|line| line.start)
        .collect();
    assert_eq!(dots, vec![Point::new(10.0, 0.0), Point::new(10.0, 0.0)]);
    assert!(
        drawn
            .iter()
            .filter(|line| line.start != line.end)
            .all(|line| line.cap == StrokeCap::Butt)
    );
}

#[test]
fn a_closed_contour_strokes_its_closing_edge_with_corners_all_round() {
    let points = [
        Point::new(0.0, 0.0),
        Point::new(10.0, 0.0),
        Point::new(10.0, 10.0),
    ];
    let drawn = lines(
        &points,
        true,
        stroke(2.0, StrokeCap::Butt, StrokeJoin::Round),
    );
    assert_eq!(drawn.len(), 3, "three edges, closing edge included");
    assert!(drawn.iter().all(|line| line.cap == StrokeCap::Round));
    assert_eq!(
        (drawn[2].start, drawn[2].end),
        (Point::new(10.0, 10.0), Point::new(0.0, 0.0))
    );
}

fn primitives(draw: impl FnOnce(&mut DrawScopeDefault)) -> Vec<DrawPrimitive> {
    let mut scope = DrawScopeDefault::new(Size::new(64.0, 64.0));
    draw(&mut scope);
    scope.into_primitives()
}

#[test]
fn a_drawn_path_strokes_as_lines_and_fills_as_an_image() {
    let mut path = Path::new();
    path.move_to(Point::new(4.0, 4.0));
    path.line_to(Point::new(40.0, 4.0));
    path.line_to(Point::new(40.0, 40.0));
    let brush = Brush::solid(Color::WHITE);
    let stroked = primitives(|scope| {
        scope.draw_path(
            &path,
            brush.clone(),
            DrawStyle::Stroke(stroke(2.0, StrokeCap::Round, StrokeJoin::Round)),
        );
    });
    assert_eq!(stroked.len(), 2);
    assert!(stroked.iter().all(|primitive| matches!(
        primitive,
        DrawPrimitive::Line { stroke, .. } if stroke.cap == StrokeCap::Round
    )));
    let filled = primitives(|scope| scope.draw_path(&path, brush.clone(), DrawStyle::Fill));
    assert!(matches!(filled.as_slice(), [DrawPrimitive::Image { .. }]));
    let blended = primitives(|scope| {
        scope.draw_path_blend(&path, brush.clone(), DrawStyle::Fill, BlendMode::Plus);
    });
    assert!(matches!(
        blended.as_slice(),
        [DrawPrimitive::Blend {
            blend_mode: BlendMode::Plus,
            ..
        }]
    ));
    let dashed = primitives(|scope| {
        scope.draw_path(
            &path,
            brush.clone(),
            DrawStyle::DashedStroke(
                stroke(2.0, StrokeCap::Round, StrokeJoin::Round),
                DashPathEffect::new(&[6.0, 6.0], 0.0),
            ),
        );
    });
    assert_eq!(
        dashed.len(),
        6,
        "72 units of edge in 12-unit periods: six dashes, three on each edge"
    );
}

/// The red channel and alpha the filled path's image draws at scope point
/// `(x, y)`.
fn fill_pixel(filled: &[DrawPrimitive], x: f32, y: f32) -> (u8, u8) {
    let [DrawPrimitive::Image { image, rect, .. }] = filled else {
        panic!("a fill draws one image");
    };
    let column = ((x - rect.x) / rect.width * image.width() as f32) as usize;
    let row = ((y - rect.y) / rect.height * image.height() as f32) as usize;
    let index = row * image.width() as usize + column;
    match image.format() {
        ImagePixelFormat::Rgba8 => (image.pixels()[index * 4], image.pixels()[index * 4 + 3]),
        ImagePixelFormat::Alpha8 { color } => (color[0], image.pixels()[index]),
    }
}

#[test]
fn a_gradient_filled_path_takes_each_color_where_the_gradient_puts_it() {
    let mut path = Path::new();
    path.move_to(Point::new(0.0, 0.0));
    path.line_to(Point::new(64.0, 0.0));
    path.line_to(Point::new(64.0, 64.0));
    path.line_to(Point::new(0.0, 64.0));
    // Half-transparent red at the top of the scope, fully transparent at its
    // bottom, as a chart's area fill fades.
    let brush = Brush::vertical_gradient(
        vec![Color(1.0, 0.0, 0.0, 0.5), Color(1.0, 0.0, 0.0, 0.0)],
        0.0,
        64.0,
    );
    let filled = primitives(|scope| scope.draw_path(&path, brush, DrawStyle::Fill));
    for (y, expected) in [
        (8.0, 0.5 * (1.0 - 8.0 / 64.0)),
        (32.0, 0.25),
        (56.0, 0.5 * (1.0 - 56.0 / 64.0)),
    ] {
        let (red, alpha) = fill_pixel(&filled, 32.0, y);
        assert_eq!(red, 255, "the fill stays red at y {y}");
        assert!(
            (f32::from(alpha) / 255.0 - expected).abs() < 0.03,
            "alpha at y {y} is {alpha}, the gradient's {expected:.3}"
        );
    }
}
