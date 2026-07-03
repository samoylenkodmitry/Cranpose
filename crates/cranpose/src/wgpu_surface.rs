pub(crate) enum SurfaceFrame {
    Ready(wgpu::SurfaceTexture),
    Reconfigure,
    Skip,
}

/// Decides whether the platform render loop must acquire and present a surface
/// frame this tick.
///
/// `surface_dirty` is the platform-owned flag that is set when the surface is
/// (re)created and cleared after a successful present. It forces the very first
/// present after surface creation even when `update()` reports no visual work -
/// the freshly built scene from `AppShell` construction has never reached the
/// swapchain yet. Without it a render loop that only checks `update_visual_changed`
/// leaves the canvas blank until an unrelated event marks the tree dirty (the
/// web "white until scroll" bug).
pub(crate) fn current_surface_texture(surface: &wgpu::Surface<'_>, context: &str) -> SurfaceFrame {
    match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(frame) => SurfaceFrame::Ready(frame),
        wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
            log::debug!("{context} surface suboptimal, rendering current frame");
            SurfaceFrame::Ready(frame)
        }
        wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
            SurfaceFrame::Reconfigure
        }
        wgpu::CurrentSurfaceTexture::Timeout => {
            log::debug!("{context} surface timeout, skipping frame");
            SurfaceFrame::Skip
        }
        wgpu::CurrentSurfaceTexture::Occluded => {
            log::debug!("{context} surface occluded, skipping frame");
            SurfaceFrame::Skip
        }
        wgpu::CurrentSurfaceTexture::Validation => {
            log::error!("{context} surface validation error, skipping frame");
            SurfaceFrame::Skip
        }
    }
}

/// True when a frame must actually be presented. Skipping idle frames
/// keeps vsync-off runs from spinning; presenting on visual changes
/// prevents the blank-until-event bug.
pub(crate) fn surface_present_required(
    surface_dirty: bool,
    update_visual_changed: bool,
    app_needs_redraw: bool,
) -> bool {
    surface_dirty || update_visual_changed || app_needs_redraw
}

#[cfg(test)]
mod tests {
    use super::surface_present_required;

    #[test]
    fn present_is_required_until_surface_is_clean() {
        // Forced initial present after surface creation, even with no visual work.
        assert!(surface_present_required(true, false, false));
        // Visual changes and pending redraws always present.
        assert!(surface_present_required(false, true, false));
        assert!(surface_present_required(false, false, true));
        // A fully idle, already-presented surface must not re-render.
        assert!(!surface_present_required(false, false, false));
    }
}
