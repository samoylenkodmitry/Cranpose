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

pub(crate) fn present_initial_placeholder_frame(
    surface: &wgpu::Surface<'_>,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
    context: &str,
) {
    if let SurfaceFrame::Ready(frame) = current_surface_texture(surface, context) {
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(format.remove_srgb_suffix()),
            ..Default::default()
        });
        cranpose_render_wgpu::clear_to_default_background(device, queue, &view);
        frame.present();
    }
}

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
        assert!(surface_present_required(true, false, false));
        assert!(surface_present_required(false, true, false));
        assert!(surface_present_required(false, false, true));
        assert!(!surface_present_required(false, false, false));
    }
}
