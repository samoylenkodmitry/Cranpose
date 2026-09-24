pub(crate) fn android_uses_present_thread(requested: Option<&str>, available_cores: usize) -> bool {
    match requested.map(str::trim) {
        Some("1" | "true" | "on") => true,
        Some("0" | "false" | "off") => false,
        _ => available_cores >= 4,
    }
}

#[cfg(test)]
#[path = "tests/android_present_thread_tests.rs"]
mod tests;
