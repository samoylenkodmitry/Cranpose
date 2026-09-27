use super::round_to_px;

#[test]
fn rounding_a_length_to_a_pixel_sends_an_exact_half_up_the_way_kotlin_does() {
    assert_eq!(round_to_px(0.25, 2.0), 0.5);
    assert_eq!(round_to_px(-0.25, 2.0), 0.0);
    assert_eq!((-0.5f32).round(), -1.0);
    assert_eq!(round_to_px(0.3, 0.0), 0.3);
    assert!(round_to_px(f32::NAN, 2.0).is_nan());
}
