pub(crate) enum SurfaceFrame {
    Ready(wgpu::SurfaceTexture),
    Reconfigure,
    Skip,
}

#[cfg(any(feature = "desktop-shell", all(feature = "ios", target_os = "ios")))]
pub(crate) fn create_wgpu_surface_and_adapter(
    window: &std::sync::Arc<dyn winit::window::Window>,
) -> Result<(wgpu::Instance, wgpu::Surface<'static>, wgpu::Adapter), crate::app_launcher::LaunchError>
{
    let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    instance_descriptor.backends = wgpu::Backends::all();
    let instance = wgpu::Instance::new(instance_descriptor);

    let surface = instance
        .create_surface(window.clone())
        .map_err(crate::app_launcher::LaunchError::SurfaceCreate)?;

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
    }))
    .map_err(crate::app_launcher::LaunchError::NoAdapter)?;

    Ok((instance, surface, adapter))
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
