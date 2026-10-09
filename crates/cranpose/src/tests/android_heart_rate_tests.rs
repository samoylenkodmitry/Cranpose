use super::*;

#[test]
fn a_zero_reading_is_not_a_pulse() {
    assert_eq!(reading(0.0, 1).status, HeartRateStatus::Acquiring);
    assert_eq!(reading(72.0, 1).live_bpm(), Some(72.0));
    assert_eq!(reading(72.0, 2).status, HeartRateStatus::OffBody);
}
