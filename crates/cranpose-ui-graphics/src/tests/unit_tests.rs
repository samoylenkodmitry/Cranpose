use super::*;

/// The two directions have to be each other's inverse, or a value that
/// made a round trip through the platform -- a touch slop measured in
/// pixels, stored as `Dp`, applied back in pixels -- comes back a
/// different size on every screen but the one it was written on.
#[test]
fn density_independent_pixels_round_trip_through_a_density() {
    for scale in [1.0f32, 1.5, 2.0, 3.0] {
        let density = Density::from_scale(scale);
        let dp = Dp(24.0);
        assert_eq!(dp.to_px(density), Px(24.0 * scale));
        assert_eq!(Dp::from_px(dp.to_px(density), density), dp);
    }
}

/// Pinned at a non-1.0 density so a regression that drops the `scale`
/// factor (or silently treats `Dp` as already being in pixels) fails
/// this test rather than only showing up as a soft, mis-sized edge on a
/// real high-density screen.
#[test]
fn a_dp_length_is_pinned_at_a_non_identity_density() {
    let density = Density::from_scale(2.5);
    assert_eq!(Dp(16.0).to_px(density), Px(40.0));
    assert_eq!(Px(40.0).to_dp(density), Dp(16.0));
}

/// Text carries the user's font-scale setting as well as the screen's
/// density, and both have to survive the trip.
#[test]
fn scale_independent_pixels_round_trip_through_density_and_font_scale() {
    for scale in [1.0f32, 2.0, 3.0] {
        for font_scale in [0.85f32, 1.0, 1.3] {
            let density = Density::new(scale, font_scale);
            let sp = Sp(16.0);
            assert_eq!(sp.to_px(density), Px(16.0 * scale * font_scale));
            assert_eq!(Sp::from_px(sp.to_px(density), density), sp);
        }
    }
}

/// Pinned at a non-1.0 density and a non-1.0 font scale together, since
/// either one alone can hide a regression that mixes up which factor
/// applies to which unit.
#[test]
fn an_sp_length_is_pinned_at_a_non_identity_density_and_font_scale() {
    let density = Density::new(2.0, 1.25);
    assert_eq!(Sp(16.0).to_px(density), Px(40.0));
}

#[test]
fn dp_arithmetic_matches_plain_float_arithmetic() {
    assert_eq!(Dp(4.0) + Dp(2.0), Dp(6.0));
    assert_eq!(Dp(4.0) - Dp(2.0), Dp(2.0));
    assert_eq!(Dp(4.0) * 2.5, Dp(10.0));
    assert_eq!(Dp(10.0) / 4.0, Dp(2.5));
    assert_eq!(-Dp(4.0), Dp(-4.0));
    assert_eq!(Dp(4.0).max(Dp(9.0)), Dp(9.0));
    assert_eq!(Dp(4.0).min(Dp(9.0)), Dp(4.0));
}

#[test]
fn literal_and_extension_constructors_agree() {
    assert_eq!(Dp::from(16.0f32), Dp(16.0));
    assert_eq!(Dp::from(16), Dp(16.0));
    assert_eq!(16.0.dp(), Dp(16.0));
    assert_eq!(16.sp(), Sp(16.0));
    assert_eq!(16.px(), Px(16.0));
}

/// `impl Into<Dp>` is what lets a widget/modifier parameter accept both
/// a bare literal and an explicit `.dp()` call; this exercises exactly
/// that generic boundary rather than the concrete type.
#[test]
fn into_dp_accepts_a_bare_literal_and_an_explicit_conversion() {
    fn padding(value: impl Into<Dp>) -> Dp {
        value.into()
    }

    assert_eq!(padding(16.0), Dp(16.0));
    assert_eq!(padding(16), Dp(16.0));
    assert_eq!(padding(16.0.dp()), Dp(16.0));
}
