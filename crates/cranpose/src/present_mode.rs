//! Present mode selection helpers for WGPU surfaces.

#[cfg(any(test, feature = "desktop-shell"))]
use cranpose_app_shell::FramePacingMode;

/// Selects the present mode based on `CRANPOSE_PRESENT_MODE` and surface capabilities.
///
/// Supported values: `auto_no_vsync`, `auto_vsync`, `fifo`, `mailbox`, `immediate`.
#[cfg_attr(target_os = "android", allow(dead_code))]
pub(crate) fn select_present_mode(caps: &wgpu::SurfaceCapabilities) -> wgpu::PresentMode {
    let requested = std::env::var("CRANPOSE_PRESENT_MODE")
        .ok()
        .and_then(|value| parse_present_mode(&value));
    select_present_mode_for_request(caps, requested)
}

/// Android presents through `Fifo`, not the cross-platform `AutoNoVsync`
/// default.
///
/// Android Vulkan surfaces advertise `[Mailbox, Fifo]`, so `AutoNoVsync`
/// resolves to `Mailbox`: neither `get_current_texture` nor `present` ever
/// blocks, and the event loop free-runs. On a Pixel 9 Pro that produced 473 fps
/// against a 60 Hz display at 107% CPU — roughly 87% of every rendered frame
/// discarded by SurfaceFlinger — and the frame's phase relative to vsync was
/// uniformly distributed across the whole 16.7 ms period, i.e. no alignment at
/// all. On slower hardware the same free-run beats against the display and the
/// presented cadence alternates between one and two vsyncs.
///
/// `Fifo` blocks the acquire until the display has consumed a buffer, which
/// both caps the work at one frame per refresh and pins the loop's phase — the
/// same thing Jetpack Compose gets from driving composition off `Choreographer`.
///
/// `debug.cranpose.present_mode` (`fifo`, `mailbox`, `immediate`, `auto_vsync`,
/// `auto_no_vsync`) overrides this on device without a rebuild.
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
            "CRANPOSE_PRESENT_MODE requested {:?}, but it is not supported; falling back to AutoNoVsync.",
            mode
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
    use cranpose_app_shell::FramePacingMode;
    use wgpu::{PresentMode, SurfaceCapabilities, TextureFormat};

    use super::{
        parse_present_mode, select_android_present_mode_for_request,
        select_present_mode_for_frame_pacing, select_present_mode_for_request,
    };

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
    fn android_defaults_to_fifo_rather_than_the_free_running_auto_no_vsync() {
        // Android Vulkan surfaces advertise exactly this pair, and AutoNoVsync
        // would resolve to Mailbox, which never blocks and lets the event loop
        // render several times per refresh.
        let caps = caps(&[PresentMode::Mailbox, PresentMode::Fifo]);

        assert_eq!(
            select_android_present_mode_for_request(&caps, None),
            PresentMode::Fifo
        );
    }

    #[test]
    fn android_falls_back_to_auto_vsync_when_fifo_is_unavailable() {
        let caps = caps(&[PresentMode::Mailbox]);

        assert_eq!(
            select_android_present_mode_for_request(&caps, None),
            PresentMode::AutoVsync
        );
    }

    #[test]
    fn android_honors_an_explicit_present_mode_request() {
        let caps = caps(&[PresentMode::Mailbox, PresentMode::Fifo]);

        assert_eq!(
            select_android_present_mode_for_request(&caps, Some(PresentMode::Mailbox)),
            PresentMode::Mailbox
        );
        assert_eq!(
            select_android_present_mode_for_request(&caps, Some(PresentMode::AutoNoVsync)),
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
