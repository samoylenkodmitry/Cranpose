pub(crate) fn android_uses_present_thread(requested: Option<&str>, available_cores: usize) -> bool {
    match requested.map(str::trim) {
        Some("1" | "true" | "on") => true,
        Some("0" | "false" | "off") => false,
        _ => available_cores >= 4,
    }
}

// Whether this device presents on a thread of its own: the
// CRANPOSE_PRESENT_THREAD diagnostic decides when set, else the core count.
#[cfg(target_os = "android")]
pub(crate) fn android_present_thread(available_cores: usize) -> bool {
    cranpose_core::DIAGNOSTICS
        .present_thread
        .with(|requested| android_uses_present_thread(requested, available_cores))
}

#[cfg(test)]
#[path = "tests/present_thread_tests.rs"]
mod tests;
