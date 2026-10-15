use super::{ceil_to_px, round_to_px};

#[test]
fn rounding_a_length_to_a_pixel_sends_an_exact_half_up_the_way_kotlin_does() {
    assert_eq!(round_to_px(0.25, 2.0), 0.5);
    assert_eq!(round_to_px(-0.25, 2.0), 0.0);
    assert_eq!((-0.5f32).round(), -1.0);
    assert_eq!(round_to_px(0.3, 0.0), 0.3);
    assert!(round_to_px(f32::NAN, 2.0).is_nan());
}

#[test]
fn at_density_one_a_length_snaps_to_the_bits_the_general_rule_gives() {
    let density = 1.0f32;
    for step in -4000..=4000 {
        let value = step as f32 * 0.0625 + (step % 7) as f32 * 0.013;
        assert_eq!(
            round_to_px(value, density).to_bits(),
            ((value * density + 0.5).floor() / density).to_bits(),
            "round {value}"
        );
        assert_eq!(
            ceil_to_px(value, density).to_bits(),
            ((value * density).ceil() / density).to_bits(),
            "ceil {value}"
        );
    }
}

#[test]
fn ceiling_a_length_to_a_pixel_takes_the_next_whole_pixel_up() {
    assert_eq!(ceil_to_px(15.1, 2.0), 15.5);
    assert_eq!(ceil_to_px(15.5, 2.0), 15.5);
    assert_eq!(ceil_to_px(-0.25, 2.0), 0.0);
    assert_eq!(ceil_to_px(0.3, 0.0), 0.3);
    assert_eq!(ceil_to_px(f32::INFINITY, 2.0), f32::INFINITY);
}
