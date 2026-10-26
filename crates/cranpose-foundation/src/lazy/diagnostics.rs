pub(super) fn telemetry_enabled() -> bool {
    cranpose_core::DIAGNOSTICS.lazy_measure_telemetry.is_set()
}

#[cfg(test)]
#[path = "tests/diagnostics_tests.rs"]
mod tests;
