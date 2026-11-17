use cranpose_ui_graphics::Point;

#[test]
fn a_point_is_as_far_from_the_origin_as_its_length() {
    assert_eq!(Point::new(3.0, 4.0).distance(), 5.0);
    assert_eq!(Point::new(-3.0, -4.0).distance(), 5.0);
    assert_eq!(Point::new(0.0, -2.5).distance(), 2.5);
    assert_eq!(Point::ZERO.distance(), 0.0);
}

#[test]
fn the_distance_between_two_points_is_the_same_from_either_end() {
    let a = Point::new(1.5, -2.0);
    let b = Point::new(-4.5, 6.0);

    assert_eq!(a - b, Point::new(6.0, -8.0));
    assert_eq!((a - b).distance(), 10.0);
    assert_eq!((b - a).distance(), 10.0);
}

#[test]
fn the_distance_of_a_point_that_is_not_finite_is_not_finite() {
    assert!(Point::new(f32::NAN, 1.0).distance().is_nan());
    assert_eq!(Point::new(f32::INFINITY, 1.0).distance(), f32::INFINITY);
}
