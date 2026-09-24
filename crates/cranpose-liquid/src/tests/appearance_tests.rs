use super::*;

#[test]
fn tint_accepts_calibration_levels_and_intermediate_values() {
    for percent in [0.0, 12.5, 20.0, 25.0, 50.0, 75.0, 100.0] {
        assert_eq!(
            GlassTintAmount::from_percent(percent).unwrap().percent(),
            percent
        );
    }
    assert_eq!(GlassTintAmount::default().percent(), 25.0);
}

#[test]
fn tint_rejects_invalid_percentages() {
    for percent in [-0.01, 100.01, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(
            GlassTintAmount::from_percent(percent),
            Err(InvalidGlassTintAmount)
        );
    }
    assert_eq!(
        InvalidGlassTintAmount.to_string(),
        "glass tint amount must be finite and between 0 and 100 percent"
    );
}
