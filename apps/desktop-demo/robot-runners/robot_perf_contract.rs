pub(crate) fn hardware_performance_contracts_enabled() -> bool {
    if env_flag("CRANPOSE_ROBOT_FORCE_HARDWARE_PERF_CONTRACTS") {
        return true;
    }
    !env_flag("LIBGL_ALWAYS_SOFTWARE") && !env_flag("CRANPOSE_ROBOT_SOFTWARE_RENDERER")
}

pub(crate) fn log_software_renderer_budget(label: &str) {
    println!(
        "{label}: strict hardware frame-rate budget not enforced for software-renderer robot run; visual and render-architecture assertions remain active"
    );
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}
