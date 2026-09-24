use super::*;

#[test]
fn an_srgb_colour_is_eight_bits_and_a_half_rounds_up() {
    let mixed = Color(9.9 / 255.0, 22.5 / 255.0, 34.2 / 255.0, 1.0);
    let snapped = mixed.srgb_8bit();
    assert_eq!(
        [
            (snapped.0 * 255.0).round() as u8,
            (snapped.1 * 255.0).round() as u8,
            (snapped.2 * 255.0).round() as u8,
        ],
        [10, 23, 34]
    );
    for channel in [snapped.0, snapped.1, snapped.2, snapped.3] {
        let scaled = channel * 255.0;
        assert!(
            (scaled - scaled.round()).abs() < 1e-3,
            "{scaled} is not a whole channel value"
        );
    }
}

#[test]
fn snapping_matches_the_platforms_own_expression_over_the_whole_range() {
    for step in 0..=100_000u32 {
        let channel = step as f32 / 100_000.0;
        let platform = (channel * 255.0 + 0.5) as u32;
        let ours = (srgb_channel_8bit(channel) * 255.0).round() as u32;
        assert_eq!(platform, ours, "channel {channel}");
    }
}

#[test]
fn snapping_is_idempotent_and_leaves_exact_bytes_alone() {
    for byte in 0..=255u8 {
        let colour = Color::from_rgba_u8(byte, byte, byte, byte);
        assert_eq!(colour.srgb_8bit(), colour);
    }
    let odd = Color(0.123_456, 0.789_012, 0.5, 0.25);
    assert_eq!(odd.srgb_8bit().srgb_8bit(), odd.srgb_8bit());
}

#[test]
fn snapping_clamps_out_of_range_channels() {
    let wild = Color(-2.0, 1.5, 0.0, 1.0).srgb_8bit();
    assert_eq!(wild, Color(0.0, 1.0, 0.0, 1.0));
}
