//! Present mode selection helpers for WGPU surfaces.

#[cfg(any(test, feature = "desktop"))]
use cranpose_app_shell::FramePacingMode;

/// Selects the present mode based on `CRANPOSE_PRESENT_MODE` and surface capabilities.
///
/// Supported values: `auto_no_vsync`, `auto_vsync`, `fifo`, `mailbox`, `immediate`.
pub(crate) fn select_present_mode(caps: &wgpu::SurfaceCapabilities) -> wgpu::PresentMode {
    let requested = std::env::var("CRANPOSE_PRESENT_MODE")
        .ok()
        .and_then(|value| parse_present_mode(&value));
    select_present_mode_for_request(caps, requested)
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
            "CRANPOSE_PRESENT_MODE requested {:?}, but it is not supported; falling back to AutoNoVsync.",
            mode
        );
    }

    wgpu::PresentMode::AutoNoVsync
}

#[cfg(any(test, feature = "desktop"))]
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

#[cfg(any(test, feature = "desktop"))]
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
mod tests {
    use super::{
        parse_present_mode, select_present_mode_for_frame_pacing, select_present_mode_for_request,
    };
    use cranpose_app_shell::FramePacingMode;
    use wgpu::{PresentMode, SurfaceCapabilities, TextureFormat};

    fn caps(present_modes: &[PresentMode]) -> SurfaceCapabilities {
        SurfaceCapabilities {
            formats: vec![TextureFormat::Bgra8UnormSrgb],
            present_modes: present_modes.to_vec(),
            ..Default::default()
        }
    }

    #[test]
    fn default_prefers_no_vsync_even_when_fifo_is_available() {
        let caps = caps(&[PresentMode::Fifo]);

        assert_eq!(
            select_present_mode_for_request(&caps, None),
            PresentMode::AutoNoVsync
        );
    }

    #[test]
    fn explicit_supported_present_mode_is_honored() {
        let caps = caps(&[PresentMode::Fifo, PresentMode::Immediate]);

        assert_eq!(
            select_present_mode_for_request(&caps, Some(PresentMode::Fifo)),
            PresentMode::Fifo
        );
        assert_eq!(
            select_present_mode_for_request(&caps, Some(PresentMode::Immediate)),
            PresentMode::Immediate
        );
    }

    #[test]
    fn explicit_auto_modes_do_not_need_surface_capability_entries() {
        let caps = caps(&[PresentMode::Fifo]);

        assert_eq!(
            select_present_mode_for_request(&caps, Some(PresentMode::AutoNoVsync)),
            PresentMode::AutoNoVsync
        );
        assert_eq!(
            select_present_mode_for_request(&caps, Some(PresentMode::AutoVsync)),
            PresentMode::AutoVsync
        );
    }

    #[test]
    fn unsupported_explicit_mode_falls_back_to_no_vsync() {
        let caps = caps(&[PresentMode::Fifo]);

        assert_eq!(
            select_present_mode_for_request(&caps, Some(PresentMode::Immediate)),
            PresentMode::AutoNoVsync
        );
    }

    #[test]
    fn parses_present_mode_aliases() {
        assert_eq!(
            parse_present_mode("no_vsync"),
            Some(PresentMode::AutoNoVsync)
        );
        assert_eq!(
            parse_present_mode("auto_vsync"),
            Some(PresentMode::AutoVsync)
        );
        assert_eq!(parse_present_mode("vsync"), Some(PresentMode::Fifo));
        assert_eq!(parse_present_mode("mailbox"), Some(PresentMode::Mailbox));
        assert_eq!(
            parse_present_mode("immediate"),
            Some(PresentMode::Immediate)
        );
        assert_eq!(parse_present_mode("unknown"), None);
    }

    #[test]
    fn frame_pacing_maps_vsync_and_no_vsync_to_surface_modes() {
        let caps = caps(&[PresentMode::Fifo, PresentMode::Immediate]);

        assert_eq!(
            select_present_mode_for_frame_pacing(&caps, FramePacingMode::Vsync),
            PresentMode::Fifo
        );
        assert_eq!(
            select_present_mode_for_frame_pacing(&caps, FramePacingMode::NoVsync),
            PresentMode::Immediate
        );
        assert_eq!(
            select_present_mode_for_frame_pacing(&caps, FramePacingMode::Hard60),
            PresentMode::Immediate
        );
        assert_eq!(
            select_present_mode_for_frame_pacing(&caps, FramePacingMode::Hard120),
            PresentMode::Immediate
        );
    }

    #[test]
    fn frame_pacing_falls_back_to_auto_modes_when_explicit_modes_are_unavailable() {
        let caps = caps(&[]);

        assert_eq!(
            select_present_mode_for_frame_pacing(&caps, FramePacingMode::Vsync),
            PresentMode::AutoVsync
        );
        assert_eq!(
            select_present_mode_for_frame_pacing(&caps, FramePacingMode::NoVsync),
            PresentMode::AutoNoVsync
        );
    }
}
