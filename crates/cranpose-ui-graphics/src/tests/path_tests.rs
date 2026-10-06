use super::*;
use crate::{BlendMode, Brush, Color, DrawPrimitive, DrawScope, DrawScopeDefault, Size, Trapezoid};

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
fn a_drawn_path_strokes_as_lines_and_fills_as_slices() {
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
    assert!(matches!(
        filled.as_slice(),
        [DrawPrimitive::Trapezoid { .. }]
    ));
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

/// The slices a filled path records, with the rect each brush resolves
/// against.
fn fill_slices(path: &Path, brush: Brush) -> Vec<(Rect, Trapezoid)> {
    primitives(|scope| scope.draw_path(path, brush, DrawStyle::Fill))
        .into_iter()
        .map(|primitive| match primitive {
            DrawPrimitive::Trapezoid {
                rect, trapezoid, ..
            } => (rect, trapezoid),
            other => panic!("a fill that slices records slices only, not {other:?}"),
        })
        .collect()
}

fn polygon(points: &[(f32, f32)]) -> Path {
    let mut path = Path::new();
    let mut points = points.iter().map(|&(x, y)| Point::new(x, y));
    if let Some(first) = points.next() {
        path.move_to(first);
    }
    for point in points {
        path.line_to(point);
    }
    path.close();
    path
}

fn slice_area(slices: &[(Rect, Trapezoid)]) -> f32 {
    slices
        .iter()
        .map(|(_, slice)| {
            let width = slice.right - slice.left;
            width * ((slice.bottom[0] - slice.top[0]) + (slice.bottom[1] - slice.top[1])) * 0.5
        })
        .sum()
}

/// How much of the pixel at `point` the slices cover together.
fn covered(slices: &[(Rect, Trapezoid)], point: Point) -> f32 {
    slices.iter().map(|(_, slice)| slice.coverage(point)).sum()
}

#[test]
fn a_filled_sparkline_slices_into_columns_that_open_only_at_its_outline() {
    let heights = [30.0, 12.0, 26.0, 4.0, 18.0, 22.0, 9.0];
    let mut points = vec![(0.0, 40.0)];
    points.extend(
        heights
            .iter()
            .enumerate()
            .map(|(index, &y)| (index as f32 * 10.0, y)),
    );
    points.push((60.0, 40.0));
    let slices = fill_slices(&polygon(&points), Brush::solid(Color::WHITE));
    assert_eq!(
        slices.len(),
        heights.len() - 1,
        "one slice between each two points"
    );
    let expected: f32 = heights
        .windows(2)
        .map(|pair| 10.0 * (40.0 - (pair[0] + pair[1]) * 0.5))
        .sum();
    assert!(
        (slice_area(&slices) - expected).abs() < 1.0e-3,
        "the slices cover the area under the line: {} of {expected}",
        slice_area(&slices)
    );
    for (index, (rect, slice)) in slices.iter().enumerate() {
        assert_eq!(
            (slice.open_left, slice.open_right),
            (index == 0, index == slices.len() - 1),
            "slice {index}: only the outline's upright ends are open: {slice:?}"
        );
        assert_eq!(
            *rect,
            Rect::from_size(Size::new(64.0, 64.0)),
            "a path's brush resolves against its scope"
        );
    }
    for (x, y, inside) in [(15.0, 35.0, 1.0), (15.0, 2.0, 0.0), (45.0, 39.0, 1.0)] {
        assert_eq!(
            covered(&slices, Point::new(x, y)),
            inside,
            "pixel at ({x}, {y})"
        );
    }
    // A pixel centred on a shared side belongs to the slice on its right.
    assert_eq!(covered(&slices, Point::new(20.0, 35.0)), 1.0);
    // An open side shares its pixel with the outside.
    assert_eq!(covered(&slices, Point::new(0.0, 35.0)), 0.5);
}

#[test]
fn a_path_that_crosses_itself_fills_where_it_winds() {
    // A bow tie: its two diagonals cross at the centre.
    let slices = fill_slices(
        &polygon(&[(0.0, 0.0), (40.0, 40.0), (40.0, 0.0), (0.0, 40.0)]),
        Brush::solid(Color::WHITE),
    );
    assert!(
        (slice_area(&slices) - 800.0).abs() < 1.0e-2,
        "two triangles of 400 each, not {}",
        slice_area(&slices)
    );
    for (x, y, inside) in [
        (5.0, 20.0, 1.0),
        (35.0, 20.0, 1.0),
        (20.0, 5.0, 0.0),
        (20.0, 35.0, 0.0),
    ] {
        assert_eq!(
            covered(&slices, Point::new(x, y)),
            inside,
            "pixel at ({x}, {y})"
        );
    }
}

#[test]
fn a_contour_wound_against_the_outline_cuts_a_hole() {
    let mut path = polygon(&[(0.0, 0.0), (48.0, 0.0), (48.0, 48.0), (0.0, 48.0)]);
    // A diamond wound the other way.
    path.move_to(Point::new(24.0, 8.0));
    path.line_to(Point::new(8.0, 24.0));
    path.line_to(Point::new(24.0, 40.0));
    path.line_to(Point::new(40.0, 24.0));
    path.close();
    let slices = fill_slices(&path, Brush::solid(Color::WHITE));
    assert!(
        (slice_area(&slices) - (48.0 * 48.0 - 512.0)).abs() < 1.0e-2,
        "the square less the diamond's 512, not {}",
        slice_area(&slices)
    );
    assert_eq!(covered(&slices, Point::new(24.5, 24.5)), 0.0, "the hole");
    assert_eq!(covered(&slices, Point::new(4.5, 24.5)), 1.0, "the frame");
}

#[test]
fn a_path_with_an_upright_edge_inside_its_fill_keeps_its_mask() {
    // A step: the fill on the left reaches higher than on the right, so the
    // upright edge between them meets only part of the column beside it.
    let step = polygon(&[
        (0.0, 0.0),
        (20.0, 0.0),
        (20.0, 10.0),
        (40.0, 10.0),
        (40.0, 30.0),
        (0.0, 30.0),
    ]);
    let filled =
        primitives(|scope| scope.draw_path(&step, Brush::solid(Color::WHITE), DrawStyle::Fill));
    assert!(
        matches!(filled.as_slice(), [DrawPrimitive::Image { .. }]),
        "the step is rasterized as before: {filled:?}"
    );
}

#[test]
fn a_fill_with_no_area_records_nothing() {
    let flat = polygon(&[(0.0, 10.0), (20.0, 10.0), (40.0, 10.0)]);
    assert!(fill_slices(&flat, Brush::solid(Color::WHITE)).is_empty());
    assert!(fill_slices(&Path::new(), Brush::solid(Color::WHITE)).is_empty());
}
