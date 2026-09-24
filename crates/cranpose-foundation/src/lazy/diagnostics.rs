pub(super) fn telemetry_enabled() -> bool {
    std::env::var_os("CRANPOSE_LAZY_MEASURE_TELEMETRY").is_some()
}

#[cfg(test)]
#[path = "tests/diagnostics_tests.rs"]
mod tests;
