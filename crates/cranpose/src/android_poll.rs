use std::time::Duration;

#[inline]
pub(crate) fn poll_timeout(timeout: Option<Duration>) -> Option<Duration> {
    timeout.map(|duration| {
        let milliseconds =
            duration.as_millis() + u128::from(!duration.subsec_nanos().is_multiple_of(1_000_000));
        Duration::from_millis(milliseconds.min(i32::MAX as u128) as u64)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finite_deadlines_do_not_become_early_polls() {
        for (nanos, milliseconds) in [
            (1, 1),
            (999_999, 1),
            (1_000_000, 1),
            (1_000_001, 2),
            (16_666_667, 17),
        ] {
            assert_eq!(
                poll_timeout(Some(Duration::from_nanos(nanos))),
                Some(Duration::from_millis(milliseconds)),
                "deadline at {nanos} ns",
            );
        }
    }

    #[test]
    fn immediate_and_unbounded_polls_remain_distinct() {
        assert_eq!(poll_timeout(None), None);
        assert_eq!(poll_timeout(Some(Duration::ZERO)), Some(Duration::ZERO));
    }

    #[test]
    fn distant_deadlines_fit_the_platforms_signed_timeout() {
        let maximum = Duration::from_millis(i32::MAX as u64);
        for duration in [maximum, maximum + Duration::from_nanos(1), Duration::MAX] {
            assert_eq!(poll_timeout(Some(duration)), Some(maximum));
        }
    }
}
