// Whether a device's frames present on a thread of their own. Android,
// desktop windows and iOS take the same rule: four cores or more leave a
// core for the present thread while the UI prepares the next frame. The
// CRANPOSE_PRESENT_THREAD diagnostic decides when set, to compare both.

pub(crate) fn uses_present_thread(requested: Option<&str>, available_cores: usize) -> bool {
    match requested.map(str::trim) {
        Some("1" | "true" | "on") => true,
        Some("0" | "false" | "off") => false,
        _ => available_cores >= 4,
    }
}

// The rule on this device.
pub(crate) fn presents_on_thread() -> bool {
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    cranpose_core::DIAGNOSTICS
        .present_thread
        .with(|requested| uses_present_thread(requested, cores))
}

#[cfg(test)]
#[path = "tests/present_thread_tests.rs"]
mod tests;
