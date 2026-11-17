//! The monotonic clock of Android and Linux, as the runtime's times show it.
#![expect(
    unsafe_code,
    reason = "the extension function that converts the runtime's time into a timespec, and clock_gettime"
)]

use std::{mem, time::Duration};

use openxr as xr;

// The time since the start of the monotonic clock at the runtime's `time`.
// `XR_KHR_convert_timespec_time` maps the runtime's time to `CLOCK_MONOTONIC`, which an app reads
// with `clock_gettime` and `Instant::now`. A runtime without the extension keeps its own time.
// Android runtimes count `CLOCK_MONOTONIC` nanoseconds as their time.
pub(crate) fn monotonic(instance: &xr::Instance, time: xr::Time) -> Duration {
    let own = Duration::from_nanos(time.as_nanos().max(0) as u64);
    let Some(convert) = &instance.exts().khr_convert_timespec_time else {
        return own;
    };
    // SAFETY: the all-zero bit pattern is a valid `timespec`.
    let mut spec: libc::timespec = unsafe { mem::zeroed() };
    // SAFETY: the function pointer belongs to the instance that the extension loaded for, the
    // handle is that instance's, and `spec` outlives the call.
    let result =
        unsafe { (convert.convert_time_to_timespec_time)(instance.as_raw(), time, &mut spec) };
    if result.into_raw() < 0 {
        return own;
    }
    since_start(&spec).unwrap_or(own)
}

// The time since the start of the monotonic clock, now: `clock_gettime(CLOCK_MONOTONIC)`. It is on
// the clock of `monotonic`, so an app can set the runtime's times against its own readings.
// The call cannot fail for this clock, and `Duration::ZERO` stands for the failure that it never
// reports.
pub(crate) fn now() -> Duration {
    // SAFETY: the all-zero bit pattern is a valid `timespec`.
    let mut spec: libc::timespec = unsafe { mem::zeroed() };
    // SAFETY: `spec` outlives the call, and CLOCK_MONOTONIC is a clock of every Android and Linux
    // kernel.
    let result = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut spec) };
    if result == 0 {
        since_start(&spec).unwrap_or_default()
    } else {
        Duration::ZERO
    }
}

fn since_start(spec: &libc::timespec) -> Option<Duration> {
    Some(Duration::new(
        u64::try_from(spec.tv_sec).ok()?,
        u32::try_from(spec.tv_nsec).ok()?,
    ))
}
