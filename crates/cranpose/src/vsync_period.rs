//! The display's refresh period, as seen from the vsync callbacks a frame
//! loop asks for.

/// The refresh periods a display can plausibly have: 250 Hz down to 20 Hz.
const MIN_VSYNC_PERIOD_NS: i64 = 4_000_000;
const MAX_VSYNC_PERIOD_NS: i64 = 50_000_000;

/// Folds the gap between two vsync callbacks into the refresh-period
/// estimate `period_ns` (`0` while none is known) and returns the new one.
///
/// Callbacks come only for the vsyncs the loop asked for, so a gap can span
/// several vsyncs, and a loop paced on a period twice too long would run at
/// half rate. Callback timestamps are the vsyncs' own, so a gap that is a
/// whole number of periods is taken as that many vsyncs; only a gap that is
/// not, from a display that changed rate, replaces the estimate.
pub(crate) fn refine_vsync_period(period_ns: i64, gap_ns: i64) -> i64 {
    if period_ns > 0 {
        let vsyncs = ((gap_ns + period_ns / 2) / period_ns).max(1);
        if (gap_ns - vsyncs * period_ns).abs() <= period_ns / 20 {
            return gap_ns / vsyncs;
        }
    }
    if (MIN_VSYNC_PERIOD_NS..=MAX_VSYNC_PERIOD_NS).contains(&gap_ns) {
        gap_ns
    } else {
        period_ns
    }
}

#[cfg(test)]
#[path = "tests/vsync_period_tests.rs"]
mod tests;
