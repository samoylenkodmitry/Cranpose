use super::*;

#[test]
fn an_axis_knows_its_own_direction_and_its_cross_axis() {
    assert!(Axis::Horizontal.is_horizontal());
    assert!(!Axis::Horizontal.is_vertical());
    assert!(Axis::Vertical.is_vertical());
    assert!(!Axis::Vertical.is_horizontal());

    assert_eq!(Axis::Horizontal.cross_axis(), Axis::Vertical);
    assert_eq!(Axis::Vertical.cross_axis(), Axis::Horizontal);
    assert_eq!(Axis::Horizontal.cross_axis().cross_axis(), Axis::Horizontal);
}
