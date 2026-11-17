use std::time::Duration;

use super::*;

#[test]
fn references_round_trip_and_an_unknown_code_is_the_arbitrary_heading() {
    for reference in [
        MotionReference::ArbitraryHeading,
        MotionReference::MagneticNorth,
    ] {
        assert_eq!(reference_of(reference_code(reference)), reference);
    }
    assert_eq!(reference_of(-1), MotionReference::ArbitraryHeading);
    assert_eq!(reference_of(7), MotionReference::ArbitraryHeading);
}

#[test]
fn a_period_goes_to_java_in_microseconds_and_never_as_the_fastest_rate() {
    assert_eq!(period_micros(MotionRate::Fastest), 0);
    assert_eq!(
        period_micros(MotionRate::Period(Duration::from_millis(20))),
        20_000
    );
    assert_eq!(period_micros(MotionRate::Period(Duration::ZERO)), 1);
    assert_eq!(
        period_micros(MotionRate::Period(Duration::from_nanos(10))),
        1
    );
    assert_eq!(
        period_micros(MotionRate::Period(Duration::from_secs(10_000))),
        i32::MAX
    );
}
