//! The platform's monotonic clock, as the runtime's times show it.
#![expect(
    unsafe_code,
    reason = "the extension function that converts the runtime's time into a timespec"
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
    if result.into_raw() < 0 || spec.tv_sec < 0 || spec.tv_nsec < 0 {
        return own;
    }
    Duration::new(spec.tv_sec as u64, spec.tv_nsec as u32)
}
