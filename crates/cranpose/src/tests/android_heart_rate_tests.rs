use super::*;

#[test]
fn only_the_heart_rate_answer_reaches_the_heart_rate_service() {
    assert_eq!(heart_rate_answer("android.permission.CAMERA\t1\n"), None);
    assert_eq!(
        heart_rate_answer("android.permission.health.READ_HEART_RATE\t1\n"),
        Some(true)
    );
    assert_eq!(
        heart_rate_answer("android.permission.BODY_SENSORS\t0\n"),
        Some(false)
    );
}

#[test]
fn a_zero_reading_is_not_a_pulse() {
    assert_eq!(reading(0.0, 1).status, HeartRateStatus::Acquiring);
    assert_eq!(reading(72.0, 1).live_bpm(), Some(72.0));
    assert_eq!(reading(72.0, 2).status, HeartRateStatus::OffBody);
}
