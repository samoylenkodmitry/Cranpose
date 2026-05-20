pub(crate) enum SurfaceFrame {
    Ready(wgpu::SurfaceTexture),
    Reconfigure,
    Skip,
}

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
