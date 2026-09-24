use super::*;

fn mask_at(mask: &[u8], width: usize, x: usize, y: usize) -> u8 {
    mask[y * width + x]
}

#[test]
fn parses_absolute_triangle() {
    let path = VectorPath::parse("M 0 0 L 10 0 L 10 10 Z").expect("valid path");
    assert_eq!(path.subpaths().len(), 1);
    assert_eq!(
        path.subpaths()[0],
        vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0)
        ]
    );
    let bounds = path.bounds();
    assert_eq!((bounds.x, bounds.y), (0.0, 0.0));
    assert_eq!((bounds.width, bounds.height), (10.0, 10.0));
}

#[test]
fn parses_relative_commands_and_h_v() {
    let path = VectorPath::parse("m 5 5 l 10 0 v 10 h -10 z").expect("valid path");
    assert_eq!(
        path.subpaths()[0],
        vec![
            Point::new(5.0, 5.0),
            Point::new(15.0, 5.0),
            Point::new(15.0, 15.0),
            Point::new(5.0, 15.0)
        ]
    );
}

#[test]
fn parses_packed_numbers_and_negative_shorthand() {
    let path = VectorPath::parse("M10-5L.5.5Z").expect("valid path");
    assert_eq!(
        path.subpaths()[0],
        vec![Point::new(10.0, -5.0), Point::new(0.5, 0.5)]
    );
}

#[test]
fn implicit_lineto_after_moveto() {
    let path = VectorPath::parse("M 0 0 10 0 10 10").expect("valid path");
    assert_eq!(path.subpaths()[0].len(), 3);
    assert_eq!(path.subpaths()[0][2], Point::new(10.0, 10.0));
}

#[test]
fn cubic_flattening_hits_endpoints() {
    let path = VectorPath::parse("M 0 0 C 0 10 10 10 10 0").expect("valid path");
    let points = &path.subpaths()[0];
    assert_eq!(points[0], Point::new(0.0, 0.0));
    assert_eq!(*points.last().unwrap(), Point::new(10.0, 0.0));
    assert!(points.len() > 4, "curve must be subdivided");
    let mid = points
        .iter()
        .min_by(|a, b| (a.x - 5.0).abs().total_cmp(&(b.x - 5.0).abs()))
        .unwrap();
    assert!(
        (mid.y - 7.5).abs() < 0.2,
        "flattened curve must pass near the true midpoint, got {mid:?}"
    );
}

#[test]
fn smooth_cubic_reflects_control_point() {
    let path = VectorPath::parse("M 0 0 C 0 5 2 5 5 5 S 10 5 10 10").expect("valid path");
    let points = &path.subpaths()[0];
    assert_eq!(*points.last().unwrap(), Point::new(10.0, 10.0));
    assert!(
        points
            .iter()
            .any(|p| (p.x - 5.0).abs() < 0.1 && (p.y - 5.0).abs() < 0.1)
    );
}

#[test]
fn quadratic_and_smooth_quadratic() {
    let path = VectorPath::parse("M 0 0 Q 5 10 10 0 T 20 0").expect("valid path");
    let points = &path.subpaths()[0];
    assert_eq!(*points.last().unwrap(), Point::new(20.0, 0.0));
    assert!(
        points
            .iter()
            .any(|p| (p.x - 5.0).abs() < 0.3 && (p.y - 5.0).abs() < 0.3)
    );
    assert!(
        points
            .iter()
            .any(|p| (p.x - 15.0).abs() < 0.3 && (p.y + 5.0).abs() < 0.3)
    );
}

#[test]
fn arc_travels_through_expected_quadrant() {
    let path = VectorPath::parse("M 0 0 A 5 5 0 0 1 10 0").expect("valid path");
    let points = &path.subpaths()[0];
    assert_eq!(*points.last().unwrap(), Point::new(10.0, 0.0));
    let lowest = points.iter().fold(0.0f32, |acc, p| acc.min(p.y));
    assert!(
        (lowest + 5.0).abs() < 0.1,
        "sweep=1 arc must pass through (5,-5), lowest y = {lowest}"
    );

    let path = VectorPath::parse("M 0 0 A 5 5 0 0 0 10 0").expect("valid path");
    let highest = path.subpaths()[0]
        .iter()
        .fold(0.0f32, |acc, p| acc.max(p.y));
    assert!(
        (highest - 5.0).abs() < 0.1,
        "sweep=0 arc must pass through (5,5), highest y = {highest}"
    );
}

#[test]
fn arc_flags_may_be_packed() {
    let spaced = VectorPath::parse("M 0 0 A 5 5 0 0 1 10 0").expect("valid path");
    let packed = VectorPath::parse("M0 0A5 5 0 0110 0").expect("valid path");
    assert_eq!(
        spaced.subpaths()[0].len(),
        packed.subpaths()[0].len(),
        "packed arc flags must parse identically"
    );
}

#[test]
fn multiple_subpaths() {
    let path =
        VectorPath::parse("M 0 0 h 4 v 4 h -4 Z M 10 10 h 4 v 4 h -4 Z").expect("valid path");
    assert_eq!(path.subpaths().len(), 2);
}

#[test]
fn rejects_garbage() {
    assert!(VectorPath::parse("this is not a path").is_err());
    assert!(
        VectorPath::parse("L 10 10").is_err(),
        "must start with moveto"
    );
    assert!(VectorPath::parse("M 10").is_err(), "missing y coordinate");
    assert!(
        VectorPath::parse("M 0 0 A 5 5 0 2 1 10 0").is_err(),
        "bad flag"
    );
    assert_eq!(
        VectorPath::parse("").unwrap_err(),
        SvgPathError::MissingMoveTo
    );
}

#[test]
fn fills_axis_aligned_rectangle() {
    let path = VectorPath::parse("M 2 2 H 8 V 8 H 2 Z").expect("valid path");
    let mask = path.coverage_mask(10, 10, Point::ZERO, 1.0);

    assert_eq!(mask_at(&mask, 10, 5, 5), 255, "interior must be opaque");
    assert_eq!(mask_at(&mask, 10, 4, 2), 255, "top edge row is inside");
    assert_eq!(mask_at(&mask, 10, 0, 0), 0, "outside must stay empty");
    assert_eq!(mask_at(&mask, 10, 9, 9), 0, "outside must stay empty");
}

#[test]
fn triangle_edge_is_antialiased() {
    let path = VectorPath::parse("M 0 0 L 8 0 L 0 8 Z").expect("valid path");
    let mask = path.coverage_mask(8, 8, Point::ZERO, 1.0);

    assert_eq!(mask_at(&mask, 8, 1, 1), 255, "deep interior is opaque");
    assert_eq!(mask_at(&mask, 8, 7, 7), 0, "far corner is empty");
    let diagonal = mask_at(&mask, 8, 4, 3);
    assert!(
        diagonal > 30 && diagonal < 225,
        "diagonal pixel should be partially covered, got {diagonal}"
    );
}

#[test]
fn even_odd_ring_has_a_hole() {
    let d = "M 0 0 H 12 V 12 H 0 Z M 4 4 H 8 V 8 H 4 Z";
    let even_odd = VectorPath::parse_with_fill_rule(d, PathFillRule::EvenOdd).expect("valid path");
    let non_zero = VectorPath::parse(d).expect("valid path");

    let even_odd_mask = even_odd.coverage_mask(12, 12, Point::ZERO, 1.0);
    let non_zero_mask = non_zero.coverage_mask(12, 12, Point::ZERO, 1.0);

    assert_eq!(mask_at(&even_odd_mask, 12, 6, 6), 0, "even-odd hole");
    assert_eq!(mask_at(&even_odd_mask, 12, 2, 6), 255, "even-odd ring");
    assert_eq!(mask_at(&non_zero_mask, 12, 6, 6), 255, "non-zero solid");
}

#[test]
fn non_zero_ring_with_reversed_inner_winding_has_a_hole() {
    let d = "M 0 0 H 12 V 12 H 0 Z M 4 4 V 8 H 8 V 4 Z";
    let path = VectorPath::parse(d).expect("valid path");
    let mask = path.coverage_mask(12, 12, Point::ZERO, 1.0);
    assert_eq!(mask_at(&mask, 12, 6, 6), 0, "reversed winding hole");
    assert_eq!(mask_at(&mask, 12, 2, 6), 255, "ring stays filled");
}

#[test]
fn circle_from_arcs_fills_center_and_respects_radius() {
    let path = VectorPath::parse("M 0 8 A 8 8 0 1 1 16 8 A 8 8 0 1 1 0 8 Z").expect("valid path");
    let mask = path.coverage_mask(16, 16, Point::ZERO, 1.0);

    assert_eq!(mask_at(&mask, 16, 8, 8), 255, "circle center is opaque");
    assert_eq!(mask_at(&mask, 16, 0, 0), 0, "circle corner is empty");
    assert_eq!(mask_at(&mask, 16, 15, 0), 0, "circle corner is empty");
    let area: f32 = mask.iter().map(|&value| value as f32 / 255.0).sum();
    let expected = std::f32::consts::PI * 8.0 * 8.0;
    assert!(
        (area - expected).abs() / expected < 0.05,
        "filled area {area} should be close to {expected}"
    );
}

#[test]
fn scale_and_origin_map_path_units_to_pixels() {
    let path = VectorPath::parse("M 10 10 H 14 V 14 H 10 Z").expect("valid path");
    let mask = path.coverage_mask(8, 8, Point::new(10.0, 10.0), 2.0);
    assert_eq!(mask_at(&mask, 8, 4, 4), 255, "scaled interior");
    let full: usize = mask.iter().filter(|&&value| value == 255).count();
    assert_eq!(full, 64, "the 8x8 pixel mask must be fully covered");
}

#[test]
fn empty_and_degenerate_paths_produce_empty_masks() {
    let path = VectorPath::parse("M 5 5 L 6 6").expect("valid path");
    assert!(path.is_empty());
    let mask = path.coverage_mask(8, 8, Point::ZERO, 1.0);
    assert!(mask.iter().all(|&value| value == 0));
}
