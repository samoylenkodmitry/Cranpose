#[cfg(any(test, feature = "desktop-shell"))]
use cranpose_app_shell::FramePacingMode;

#[cfg_attr(target_os = "android", allow(dead_code))]
pub(crate) fn select_present_mode(caps: &wgpu::SurfaceCapabilities) -> wgpu::PresentMode {
    let requested = std::env::var("CRANPOSE_PRESENT_MODE")
        .ok()
        .and_then(|value| parse_present_mode(&value));
    select_present_mode_for_request(caps, requested)
}

#[cfg(target_os = "android")]
pub(crate) fn select_android_present_mode(caps: &wgpu::SurfaceCapabilities) -> wgpu::PresentMode {
    let requested = crate::android_frame_telemetry::system_property("debug.cranpose.present_mode")
        .as_deref()
        .and_then(parse_present_mode);
    select_android_present_mode_for_request(caps, requested)
}

#[cfg_attr(not(any(test, target_os = "android")), allow(dead_code))]
fn select_android_present_mode_for_request(
    caps: &wgpu::SurfaceCapabilities,
    requested: Option<wgpu::PresentMode>,
) -> wgpu::PresentMode {
    if requested.is_some() {
        return select_present_mode_for_request(caps, requested);
    }
    if caps.present_modes.contains(&wgpu::PresentMode::Fifo) {
        wgpu::PresentMode::Fifo
    } else {
        wgpu::PresentMode::AutoVsync
    }
}

fn select_present_mode_for_request(
    caps: &wgpu::SurfaceCapabilities,
    requested: Option<wgpu::PresentMode>,
) -> wgpu::PresentMode {
    if let Some(mode) = requested {
        if is_auto_present_mode(mode) {
            return mode;
        }
        if caps.present_modes.contains(&mode) {
            return mode;
        }
        log::warn!(
            "CRANPOSE_PRESENT_MODE requested {mode:?}, but it is not supported; falling back to AutoNoVsync."
        );
    }

    wgpu::PresentMode::AutoNoVsync
}

#[cfg(any(test, feature = "desktop-shell"))]
pub(crate) fn select_present_mode_for_frame_pacing(
    caps: &wgpu::SurfaceCapabilities,
    mode: FramePacingMode,
) -> wgpu::PresentMode {
    match mode {
        FramePacingMode::Vsync => supported_or_auto(caps, wgpu::PresentMode::Fifo),
        FramePacingMode::Hard60 | FramePacingMode::Hard120 | FramePacingMode::NoVsync => {
            supported_or_auto(caps, wgpu::PresentMode::Immediate)
        }
    }
}

#[cfg(any(test, feature = "desktop-shell"))]
fn supported_or_auto(
    caps: &wgpu::SurfaceCapabilities,
    preferred: wgpu::PresentMode,
) -> wgpu::PresentMode {
    if caps.present_modes.contains(&preferred) {
        return preferred;
    }
    match preferred {
        wgpu::PresentMode::Fifo => wgpu::PresentMode::AutoVsync,
        _ => wgpu::PresentMode::AutoNoVsync,
    }
}

fn is_auto_present_mode(mode: wgpu::PresentMode) -> bool {
    matches!(
        mode,
        wgpu::PresentMode::AutoNoVsync | wgpu::PresentMode::AutoVsync
    )
}

#[cfg(any(test, feature = "robot"))]
pub(crate) fn resolved_present_mode(
    mode: wgpu::PresentMode,
    caps: &wgpu::SurfaceCapabilities,
) -> wgpu::PresentMode {
    let fallbacks: &[wgpu::PresentMode] = match mode {
        wgpu::PresentMode::AutoNoVsync => &[
            wgpu::PresentMode::Immediate,
            wgpu::PresentMode::Mailbox,
            wgpu::PresentMode::Fifo,
        ],
        wgpu::PresentMode::AutoVsync => &[
            wgpu::PresentMode::FifoRelaxed,
            wgpu::PresentMode::Fifo,
        ],
        _ => return mode,
    };
    fallbacks.iter().copied().find(|mode| caps.present_modes.contains(mode)).unwrap_or(mode)
}

fn parse_present_mode(value: &str) -> Option<wgpu::PresentMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto_no_vsync" | "autonovsync" | "no_vsync" | "novsync" => {
            Some(wgpu::PresentMode::AutoNoVsync)
        }
        "auto_vsync" | "autovsync" => Some(wgpu::PresentMode::AutoVsync),
        "fifo" | "vsync" => Some(wgpu::PresentMode::Fifo),
        "mailbox" => Some(wgpu::PresentMode::Mailbox),
        "immediate" => Some(wgpu::PresentMode::Immediate),
        _ => None,
    }
}

#[cfg(test)]
#[path = "tests/present_mode_tests.rs"]
mod tests;
