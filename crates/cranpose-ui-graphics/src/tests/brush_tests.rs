use super::*;

fn red_to_blue() -> Vec<Color> {
    vec![Color(1.0, 0.0, 0.0, 1.0), Color(0.0, 0.0, 1.0, 1.0)]
}

fn linear_parts(brush: &Brush) -> (&Vec<Color>, &Option<Vec<f32>>, Point, Point, TileMode) {
    match brush {
        Brush::LinearGradient {
            colors,
            stops,
            start,
            end,
            tile_mode,
        } => (colors, stops, *start, *end, *tile_mode),
        other => panic!("expected LinearGradient, got {other:?}"),
    }
}

#[test]
fn an_axis_gradient_varies_along_its_own_axis_only() {
    let vertical = Brush::vertical_gradient(red_to_blue(), 10.0, 90.0);
    let (colors, stops, start, end, tile_mode) = linear_parts(&vertical);
    assert_eq!(colors, &red_to_blue());
    assert!(stops.is_none());
    assert_eq!(start, Point { x: 0.0, y: 10.0 });
    assert_eq!(end, Point { x: 0.0, y: 90.0 });
    assert_eq!(tile_mode, TileMode::Clamp);

    let horizontal = Brush::horizontal_gradient(red_to_blue(), 10.0, 90.0);
    let (_, _, start, end, _) = linear_parts(&horizontal);
    assert_eq!(start, Point { x: 10.0, y: 0.0 });
    assert_eq!(end, Point { x: 90.0, y: 0.0 });
}

#[test]
fn a_default_axis_gradient_spans_whatever_it_is_painted_into() {
    for brush in [
        Brush::vertical_gradient_default(red_to_blue()),
        Brush::horizontal_gradient_default(red_to_blue()),
    ] {
        let (_, _, start, end, tile_mode) = linear_parts(&brush);
        assert_eq!(start, Point { x: 0.0, y: 0.0 });
        assert!(end.x.is_infinite() || end.y.is_infinite());
        assert_eq!(tile_mode, TileMode::Clamp);
    }
}

#[test]
fn a_tiled_axis_gradient_carries_its_tile_mode() {
    let vertical = Brush::vertical_gradient_tiled(red_to_blue(), 0.0, 8.0, TileMode::Repeated);
    let (_, _, _, _, tile_mode) = linear_parts(&vertical);
    assert_eq!(tile_mode, TileMode::Repeated);

    let horizontal = Brush::horizontal_gradient_tiled(red_to_blue(), 0.0, 8.0, TileMode::Mirror);
    let (_, _, _, _, tile_mode) = linear_parts(&horizontal);
    assert_eq!(tile_mode, TileMode::Mirror);
}

#[test]
fn explicit_stops_travel_beside_their_colours() {
    let stops_in = vec![
        (0.0, Color(1.0, 0.0, 0.0, 1.0)),
        (0.8, Color(0.0, 1.0, 0.0, 1.0)),
        (1.0, Color(0.0, 0.0, 1.0, 1.0)),
    ];
    let expected_colors: Vec<Color> = stops_in.iter().map(|(_, color)| *color).collect();
    let expected_positions: Vec<f32> = stops_in.iter().map(|(at, _)| *at).collect();

    let vertical = Brush::vertical_gradient_stops(stops_in.clone(), 0.0, 100.0, TileMode::Clamp);
    let (colors, stops, start, end, _) = linear_parts(&vertical);
    assert_eq!(colors, &expected_colors);
    assert_eq!(stops.as_ref(), Some(&expected_positions));
    assert_eq!(start, Point { x: 0.0, y: 0.0 });
    assert_eq!(end, Point { x: 0.0, y: 100.0 });

    let horizontal =
        Brush::horizontal_gradient_stops(stops_in.clone(), 0.0, 100.0, TileMode::Clamp);
    let (colors, stops, start, end, _) = linear_parts(&horizontal);
    assert_eq!(colors, &expected_colors);
    assert_eq!(stops.as_ref(), Some(&expected_positions));
    assert_eq!(start, Point { x: 0.0, y: 0.0 });
    assert_eq!(end, Point { x: 100.0, y: 0.0 });

    let center = Point { x: 5.0, y: 5.0 };
    match Brush::radial_gradient_stops(stops_in.clone(), center, 20.0, TileMode::Repeated) {
        Brush::RadialGradient {
            colors,
            stops,
            center: at,
            radius,
            tile_mode,
        } => {
            assert_eq!(colors, expected_colors);
            assert_eq!(stops, Some(expected_positions.clone()));
            assert_eq!(at, center);
            assert_eq!(radius, 20.0);
            assert_eq!(tile_mode, TileMode::Repeated);
        }
        other => panic!("expected RadialGradient, got {other:?}"),
    }

    match Brush::sweep_gradient_stops(stops_in, center) {
        Brush::SweepGradient {
            colors,
            stops,
            center: at,
        } => {
            assert_eq!(colors, expected_colors);
            assert_eq!(stops, Some(expected_positions));
            assert_eq!(at, center);
        }
        other => panic!("expected SweepGradient, got {other:?}"),
    }
}

#[test]
fn a_tiled_radial_gradient_carries_its_tile_mode_and_geometry() {
    let center = Point { x: 12.0, y: 34.0 };
    match Brush::radial_gradient_tiled(red_to_blue(), center, 7.5, TileMode::Mirror) {
        Brush::RadialGradient {
            colors,
            stops,
            center: at,
            radius,
            tile_mode,
        } => {
            assert_eq!(colors, red_to_blue());
            assert!(stops.is_none());
            assert_eq!(at, center);
            assert_eq!(radius, 7.5);
            assert_eq!(tile_mode, TileMode::Mirror);
        }
        other => panic!("expected RadialGradient, got {other:?}"),
    }
}

#[test]
fn sweep_gradient_construction() {
    let colors = vec![Color(1.0, 0.0, 0.0, 1.0), Color(0.0, 0.0, 1.0, 1.0)];
    let center = Point { x: 50.0, y: 50.0 };
    let brush = Brush::sweep_gradient(colors.clone(), center);
    match brush {
        Brush::SweepGradient {
            colors: c,
            stops,
            center: p,
        } => {
            assert_eq!(c, colors);
            assert_eq!(p, center);
            assert!(stops.is_none());
        }
        _ => panic!("expected SweepGradient"),
    }
}

#[test]
fn brush_clone_eq() {
    let a = Brush::solid(Color(1.0, 0.0, 0.0, 1.0));
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn vertical_gradient_construction() {
    let colors = vec![Color(0.0, 0.0, 0.0, 1.0), Color(0.0, 0.0, 0.0, 0.0)];
    let brush = Brush::vertical_gradient(colors.clone(), 24.0, 64.0);
    match brush {
        Brush::LinearGradient {
            colors: c,
            stops,
            start,
            end,
            tile_mode,
        } => {
            assert_eq!(c, colors);
            assert!(stops.is_none());
            assert_eq!(tile_mode, TileMode::Clamp);
            assert_eq!(start, Point { x: 0.0, y: 24.0 });
            assert_eq!(end, Point { x: 0.0, y: 64.0 });
        }
        _ => panic!("expected LinearGradient"),
    }
}

#[test]
fn linear_gradient_defaults_to_infinite_end() {
    let brush = Brush::linear_gradient(vec![Color::BLACK, Color::WHITE]);
    match brush {
        Brush::LinearGradient { start, end, .. } => {
            assert_eq!(start, Point { x: 0.0, y: 0.0 });
            assert!(end.x.is_infinite());
            assert!(end.y.is_infinite());
        }
        _ => panic!("expected LinearGradient"),
    }
}

#[test]
fn gradient_color_stops_are_stored() {
    let brush = Brush::linear_gradient_stops(
        vec![(0.0, Color::RED), (0.6, Color::GREEN), (1.0, Color::BLUE)],
        Point { x: 0.0, y: 0.0 },
        Point { x: 20.0, y: 10.0 },
        TileMode::Mirror,
    );

    match brush {
        Brush::LinearGradient {
            colors,
            stops,
            tile_mode,
            ..
        } => {
            assert_eq!(colors, vec![Color::RED, Color::GREEN, Color::BLUE]);
            assert_eq!(stops, Some(vec![0.0, 0.6, 1.0]));
            assert_eq!(tile_mode, TileMode::Mirror);
        }
        _ => panic!("expected LinearGradient"),
    }
}
