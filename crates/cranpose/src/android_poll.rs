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
#[path = "tests/android_poll_tests.rs"]
mod tests;
