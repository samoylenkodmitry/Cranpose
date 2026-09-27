use super::*;

// Expected entries come from Skia's `SkTMaskGamma_build_correcting_lut` with
// the default sRGB transfer and contrast 0.5.

#[test]
fn white_text_gets_the_srgb_encoded_coverage_of_a_linear_light_blend() {
    let white = TextLuminance::of_color(Color::WHITE).correction();
    assert_eq!(white.apply(32.0 / 255.0), 99);
    assert_eq!(white.apply(64.0 / 255.0), 137);
    assert_eq!(white.apply(128.0 / 255.0), 188);
}

#[test]
fn black_text_gets_the_linear_light_blend_with_its_contrast_boost() {
    let black = TextLuminance::of_color(Color::BLACK).correction();
    assert_eq!(black.apply(32.0 / 255.0), 21);
    assert_eq!(black.apply(128.0 / 255.0), 91);
    assert_eq!(black.apply(192.0 / 255.0), 146);
}

#[test]
fn empty_and_full_coverage_are_kept_for_every_luminance() {
    for level in 0..=7u8 {
        let grey = f32::from(level) / 7.0;
        let correction = TextLuminance::of_color(Color(grey, grey, grey, 1.0)).correction();
        assert_eq!(correction.apply(0.0), 0);
        assert_eq!(correction.apply(1.0), 255);
    }
}

#[test]
fn luminance_weighs_green_over_red_and_ignores_alpha() {
    assert_eq!(
        TextLuminance::of_color(Color(1.0, 0.0, 0.0, 1.0)),
        TextLuminance(1)
    );
    assert_eq!(
        TextLuminance::of_color(Color(0.0, 1.0, 0.0, 1.0)),
        TextLuminance(5)
    );
    assert_eq!(
        TextLuminance::of_color(Color(1.0, 1.0, 1.0, 0.2)),
        TextLuminance::of_color(Color::WHITE)
    );
}

#[test]
fn a_gradient_brush_is_corrected_as_mid_grey() {
    let gradient = Brush::linear_gradient(vec![Color::WHITE, Color::BLACK]);
    assert_eq!(TextLuminance::of_brush(&gradient), TextLuminance(4));
    assert_eq!(
        TextLuminance::of_brush(&Brush::Solid(Color::WHITE)),
        TextLuminance::of_color(Color::WHITE)
    );
}

#[test]
fn unit_correction_matches_the_mask_value() {
    let white = TextLuminance::of_color(Color::WHITE).correction();
    assert_eq!(white.apply_unit(128.0 / 255.0), 188.0 / 255.0);
}
