pub(super) fn telemetry_enabled() -> bool {
    cranpose_core::env_flag!("CRANPOSE_LAZY_MEASURE_TELEMETRY")
}

#[cfg(test)]
#[path = "tests/diagnostics_tests.rs"]
mod tests;
