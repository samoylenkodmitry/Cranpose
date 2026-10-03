/// Nanoseconds of work a presented frame took on the threads that make it,
/// as the scheduler's performance hints want it: the frame loop's part from
/// the frame's start until it handed the frame to rendering, and the
/// render's part from the moment it had a buffer until the present
/// returned. Waiting for a free buffer, or for the render of the frame
/// before, is not work. `None` when a timestamp is missing or out of order.
pub(crate) fn frame_work_ns(
    started_ns: i64,
    handed_off_ns: i64,
    acquired_ns: i64,
    presented_ns: i64,
) -> Option<i64> {
    let produce = handed_off_ns - started_ns;
    let render = presented_ns - acquired_ns;
    (started_ns > 0 && acquired_ns > 0 && produce >= 0 && render >= 0).then_some(produce + render)
}

#[cfg(test)]
#[path = "tests/android_frame_work_tests.rs"]
mod tests;
