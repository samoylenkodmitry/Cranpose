use super::*;

const PLATFORM_124: [(f32, f32); 10] = [
    (8.0, 9.92),
    (10.0, 12.4),
    (12.0, 14.88),
    (14.0, 17.84),
    (16.0, 19.36),
    (18.0, 20.88),
    (20.0, 22.88),
    (24.0, 25.92),
    (30.0, 30.0),
    (100.0, 100.0),
];

fn platform_124() -> FontScaleCurve {
    FontScaleCurve::from_samples(1.24, &PLATFORM_124)
}

#[test]
fn a_curve_with_no_knots_multiplies() {
    let curve = FontScaleCurve::linear(1.24);
    assert!(curve.is_linear());
    assert_eq!(curve.sp_to_dp(13.0), 13.0 * 1.24);
    assert_eq!(curve.sp_to_dp(0.4), 0.4 * 1.24);
    assert_eq!(curve.scale(), 1.24);
}

#[test]
fn the_identity_curve_leaves_a_size_alone() {
    assert!(FontScaleCurve::linear(1.0).is_identity());
    assert!(!FontScaleCurve::linear(1.24).is_identity());
    assert!(!platform_124().is_identity());
    let flat: Vec<(f32, f32)> = (1..=40).map(|sp| (sp as f32, sp as f32)).collect();
    assert!(FontScaleCurve::from_samples(1.0, &flat).is_identity());
}

#[test]
fn the_platform_curve_is_reproduced_at_every_size_the_platform_was_asked() {
    let curve = platform_124();
    for (sp, expected_px) in [
        (0.4f32, 0.992f32),
        (12.0, 29.76),
        (13.0, 32.72),
        (14.0, 35.68),
        (15.0, 37.2),
        (16.0, 38.72),
        (18.0, 41.76),
        (19.0, 43.76),
        (20.0, 45.76),
        (24.0, 51.84),
        (30.0, 60.0),
        (40.0, 80.0),
        (100.0, 200.0),
    ] {
        let px = curve.sp_to_dp(sp) * 2.0;
        assert!(
            (px - expected_px).abs() < 1.0e-3,
            "{sp}sp resolved to {px}px where the platform answered {expected_px}px",
        );
    }
}

#[test]
fn the_thirteen_sp_secondary_label_is_where_multiplying_goes_wrong() {
    let curve = platform_124();
    assert!((curve.sp_to_dp(13.0) * 2.0 - 32.72).abs() < 1.0e-3);
    assert!((FontScaleCurve::linear(1.24).sp_to_dp(13.0) * 2.0 - 32.24).abs() < 1.0e-3);
}

#[test]
fn collinear_samples_are_dropped_and_the_curve_still_answers_the_same() {
    let curve = platform_124();
    assert_eq!(curve.knot_count(), 8);
    let dense: Vec<(f32, f32)> = (1..=120)
        .map(|sp| {
            let sp = sp as f32;
            (sp, curve.sp_to_dp(sp))
        })
        .collect();
    let resampled = FontScaleCurve::from_samples(1.24, &dense);
    for step in 1..=1200 {
        let sp = step as f32 * 0.1;
        assert!(
            (resampled.sp_to_dp(sp) - curve.sp_to_dp(sp)).abs() < 1.0e-3,
            "{sp}sp: {} against {}",
            resampled.sp_to_dp(sp),
            curve.sp_to_dp(sp),
        );
    }
}

#[test]
fn samples_the_platform_could_not_have_produced_fall_back_to_multiplying() {
    for samples in [
        &[][..],
        &[(12.0, 14.88)][..],
        &[(12.0, 14.88), (12.0, 15.0)][..],
        &[(14.0, 17.84), (12.0, 14.88)][..],
        &[(12.0, f32::NAN), (14.0, 17.84)][..],
        &[(12.0, 0.0), (14.0, 17.84)][..],
    ] {
        let curve = FontScaleCurve::from_samples(1.24, samples);
        assert!(curve.is_linear(), "{samples:?} was accepted");
        assert_eq!(curve.sp_to_dp(13.0), 13.0 * 1.24);
    }
}

#[test]
fn more_bends_than_there_are_knots_falls_back_rather_than_truncating() {
    let zigzag: Vec<(f32, f32)> = (1..=40)
        .map(|sp| {
            let sp = sp as f32;
            (sp, sp * if sp as i32 % 2 == 0 { 1.5 } else { 1.2 })
        })
        .collect();
    assert!(FontScaleCurve::from_samples(1.24, &zigzag).is_linear());
}

#[test]
fn a_negative_size_keeps_its_sign() {
    let curve = platform_124();
    assert!((curve.sp_to_dp(-13.0) + curve.sp_to_dp(13.0)).abs() < 1.0e-6);
    assert!(FontScaleCurve::linear(1.24).sp_to_dp(-13.0) < 0.0);
}

#[test]
fn a_size_that_is_not_a_number_comes_back_unchanged() {
    let curve = platform_124();
    assert!(curve.sp_to_dp(f32::NAN).is_nan());
    assert_eq!(curve.sp_to_dp(f32::INFINITY), f32::INFINITY);
}

#[test]
fn curves_that_convert_differently_do_not_share_a_fingerprint() {
    let platform = platform_124();
    assert_ne!(
        platform.fingerprint(),
        FontScaleCurve::linear(1.24).fingerprint()
    );
    assert_ne!(
        FontScaleCurve::linear(1.0).fingerprint(),
        FontScaleCurve::linear(1.24).fingerprint()
    );
    assert_eq!(platform.fingerprint(), platform_124().fingerprint());
}
