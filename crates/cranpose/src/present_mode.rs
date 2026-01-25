//! Present mode selection helpers for WGPU surfaces.

/// Selects the present mode based on `CRANPOSE_PRESENT_MODE` and surface capabilities.
///
/// Supported values: `fifo`, `mailbox`, `immediate`.
pub(crate) fn select_present_mode(caps: &wgpu::SurfaceCapabilities) -> wgpu::PresentMode {
    let requested = std::env::var("CRANPOSE_PRESENT_MODE")
        .ok()
        .and_then(|value| parse_present_mode(&value));

    if let Some(mode) = requested {
        if caps.present_modes.contains(&mode) {
            return mode;
        }
        log::warn!(
            "CRANPOSE_PRESENT_MODE requested {:?}, but it is not supported; falling back to FIFO.",
            mode
        );
    }

    if caps.present_modes.contains(&wgpu::PresentMode::Fifo) {
        wgpu::PresentMode::Fifo
    } else {
        caps.present_modes
            .first()
            .copied()
            .unwrap_or(wgpu::PresentMode::Fifo)
    }
}

fn parse_present_mode(value: &str) -> Option<wgpu::PresentMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "fifo" | "vsync" => Some(wgpu::PresentMode::Fifo),
        "mailbox" => Some(wgpu::PresentMode::Mailbox),
        "immediate" | "no_vsync" | "novsync" => Some(wgpu::PresentMode::Immediate),
        _ => None,
    }
}
