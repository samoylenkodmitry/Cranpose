use super::{HorizontalAlignment, VerticalAlignment, bias_offset};

#[test]
fn alignment_places_a_child_at_its_bias_on_a_device_pixel() {
    assert_eq!(HorizontalAlignment::Start.align(100.0, 41.0, 1.0), 0.0);
    assert_eq!(
        HorizontalAlignment::CenterHorizontally.align(100.0, 41.0, 1.0),
        30.0
    );
    assert_eq!(HorizontalAlignment::End.align(100.0, 41.0, 1.0), 59.0);
    assert_eq!(
        VerticalAlignment::CenterVertically.align(100.0, 41.0, 2.0),
        29.5
    );
    assert_eq!(VerticalAlignment::Bottom.align(50.0, 10.0, 1.0), 40.0);
}

#[test]
fn a_child_larger_than_its_space_overflows_both_ends_when_centred() {
    assert_eq!(
        HorizontalAlignment::CenterHorizontally.align(40.0, 60.0, 1.0),
        -10.0
    );
    assert_eq!(VerticalAlignment::Bottom.align(40.0, 60.0, 1.0), -20.0);
    assert_eq!(VerticalAlignment::Top.align(40.0, 60.0, 1.0), 0.0);
}

#[test]
fn a_bias_between_the_edges_is_rounded_half_up() {
    assert_eq!(bias_offset(-0.5, 101.0, 0.0, 1.0), 25.0);
    assert_eq!(bias_offset(0.0, 9.0, 10.0, 1.0), 0.0);
}
