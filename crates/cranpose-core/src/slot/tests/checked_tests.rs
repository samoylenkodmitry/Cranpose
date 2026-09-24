use super::*;

#[test]
fn checked_usize_to_u32_accepts_u32_max() {
    assert_eq!(
        checked_usize_to_u32(u32::MAX as usize, "test field"),
        u32::MAX
    );
}

#[test]
#[should_panic(expected = "test field exceeds u32 storage limit")]
fn checked_usize_to_u32_rejects_overflow() {
    checked_usize_to_u32(u32::MAX as usize + 1, "test field");
}

#[test]
fn checked_u32_delta_applies_positive_and_negative_deltas() {
    assert_eq!(
        checked_u32_delta(
            10,
            CheckedU32Delta::from_i64(5, "test field"),
            0,
            "test field"
        ),
        15
    );
    assert_eq!(
        checked_u32_delta(
            10,
            CheckedU32Delta::from_i64(-5, "test field"),
            0,
            "test field"
        ),
        5
    );
}

#[test]
fn try_checked_u32_delta_reports_overflow_without_panicking() {
    assert_eq!(
        try_checked_u32_delta(u32::MAX, CheckedU32Delta::from_i64(1, "test field"), 0),
        None
    );
    assert_eq!(
        try_checked_u32_delta(0, CheckedU32Delta::from_i64(-1, "test field"), 0),
        None
    );
    assert_eq!(
        try_checked_u32_delta(1, CheckedU32Delta::from_i64(-1, "test field"), 1),
        None
    );
    assert_eq!(
        try_checked_u32_delta(1, CheckedU32Delta::from_i64(1, "test field"), 0),
        Some(2)
    );
}

#[test]
#[should_panic(expected = "test field cannot become smaller than 1")]
fn checked_u32_delta_rejects_values_below_minimum() {
    checked_u32_delta(
        1,
        CheckedU32Delta::from_i64(-1, "test field"),
        1,
        "test field",
    );
}
