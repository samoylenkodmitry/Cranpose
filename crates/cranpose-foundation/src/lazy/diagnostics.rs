pub(super) fn telemetry_enabled() -> bool {
    std::env::var_os("CRANPOSE_LAZY_MEASURE_TELEMETRY").is_some()
}

#[cfg(test)]
mod tests {
    #[test]
    fn lazy_measure_telemetry_has_no_process_global_state() {
        let source = include_str!("diagnostics.rs");
        let atomic_counter = ["Atomic", "U64"].concat();
        let once_lock = ["Once", "Lock"].concat();
        let telemetry_static = ["static ", "TELEMETRY_ENABLED"].concat();
        let measure_cycle_static = ["static ", "MEASURE_CYCLE_COUNTER"].concat();
        let item_pass_static = ["static ", "ITEM_MEASURE_PASS_COUNTER"].concat();

        assert!(
            !source.contains(&telemetry_static)
                && !source.contains(&measure_cycle_static)
                && !source.contains(&item_pass_static)
                && !source.contains(&once_lock)
                && !source.contains(&atomic_counter),
            "lazy measurement telemetry must be owned by lazy-list runtime state, not process globals"
        );
    }
}
