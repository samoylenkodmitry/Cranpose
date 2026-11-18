//! The performance counter of Windows, as the runtime's times show it.
#![expect(
    unsafe_code,
    reason = "the extension function that converts the runtime's time into a performance counter, and the counter's functions"
)]

use std::time::Duration;

use openxr as xr;
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

// The time since the start of the performance counter at the runtime's `time`.
// `XR_KHR_win32_convert_performance_counter_time` maps the runtime's time to that counter, which
// an app reads with `QueryPerformanceCounter`. A runtime without the extension keeps its own time.
pub(crate) fn monotonic(instance: &xr::Instance, time: xr::Time) -> Duration {
    let own = Duration::from_nanos(time.as_nanos().max(0).cast_unsigned());
    let Some(convert) = &instance.exts().khr_win32_convert_performance_counter_time else {
        return own;
    };
    let mut counter = 0_i64;
    // SAFETY: the function pointer belongs to the instance that the extension loaded for, the
    // handle is that instance's, and `counter` outlives the call.
    let result = unsafe {
        (convert.convert_time_to_win32_performance_counter)(instance.as_raw(), time, &mut counter)
    };
    if result.into_raw() < 0 {
        return own;
    }
    since_start(counter).unwrap_or(own)
}

// The time since the start of the performance counter, now. It is on the clock of `monotonic`, so
// an app can set the runtime's times against its own readings. The call cannot fail on Windows XP
// and later, and `Duration::ZERO` stands for the failure that it never reports.
pub(crate) fn now() -> Duration {
    let mut counter = 0_i64;
    // SAFETY: `counter` outlives the call.
    let read = unsafe { QueryPerformanceCounter(&mut counter) };
    if read == 0 {
        return Duration::ZERO;
    }
    since_start(counter).unwrap_or_default()
}

// The duration of `counter` ticks. The whole seconds and the rest of a second convert apart, so
// that no product overflows.
fn since_start(counter: i64) -> Option<Duration> {
    let ticks = u64::try_from(counter).ok()?;
    let frequency = frequency()?;
    let rest = u128::from(ticks % frequency) * 1_000_000_000 / u128::from(frequency);
    Some(Duration::new(ticks / frequency, u32::try_from(rest).ok()?))
}

// The ticks of the counter in a second. It is fixed at boot.
fn frequency() -> Option<u64> {
    let mut frequency = 0_i64;
    // SAFETY: `frequency` outlives the call.
    let read = unsafe { QueryPerformanceFrequency(&mut frequency) };
    u64::try_from(frequency)
        .ok()
        .filter(|ticks| read != 0 && *ticks > 0)
}
