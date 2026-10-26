pub(crate) enum SurfaceFrame {
    Ready(wgpu::SurfaceTexture),
    Reconfigure,
    Skip,
}

/// The wgpu instance a native window's GPU is opened through.
#[cfg(any(feature = "desktop-shell", all(feature = "ios", target_os = "ios")))]
pub(crate) fn new_instance() -> wgpu::Instance {
    let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    instance_descriptor.backends = wgpu::Backends::all();
    // Windows stretches the last frame of a swapchain made from the window
    // itself to the window's new size until the next one arrives, so a window
    // being resized wobbles. A DirectComposition swapchain is shown at its own
    // size. `WGPU_DX12_PRESENTATION_SYSTEM=Hwnd` restores the old one.
    instance_descriptor.backend_options.dx12.presentation_system =
        wgpu::Dx12SwapchainKind::DxgiFromVisual.with_env();
    wgpu::Instance::new(instance_descriptor)
}

/// The high-performance adapter of `instance`, one that presents to
/// `surface` when one is given.
#[cfg(any(feature = "desktop-shell", all(feature = "ios", target_os = "ios")))]
pub(crate) fn request_adapter(
    instance: &wgpu::Instance,
    surface: Option<&wgpu::Surface<'_>>,
) -> Result<wgpu::Adapter, wgpu::RequestAdapterError> {
    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: surface,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
}

#[cfg(any(feature = "desktop-shell", all(feature = "ios", target_os = "ios")))]
pub(crate) fn create_wgpu_surface_and_adapter(
    window: &std::sync::Arc<dyn winit::window::Window>,
) -> Result<(wgpu::Instance, wgpu::Surface<'static>, wgpu::Adapter), crate::app_launcher::LaunchError>
{
    let instance = new_instance();
    let surface = instance
        .create_surface(std::sync::Arc::clone(window))
        .map_err(crate::app_launcher::LaunchError::SurfaceCreate)?;
    // Each image comes back with the frame it presented, so a frame draws
    // only what changed since (`render_surface_texture`).
    surface.keep_presented_images(true);
    let adapter = request_adapter(&instance, Some(&surface))
        .map_err(crate::app_launcher::LaunchError::NoAdapter)?;
    Ok((instance, surface, adapter))
}

// The configuration of a window's root surface: `format` at `width` by
// `height`, the usages a presentable root takes from `caps`, and a display
// view of the format.
pub(crate) fn root_surface_config(
    caps: &wgpu::SurfaceCapabilities,
    format: wgpu::TextureFormat,
    (width, height): (u32, u32),
    present_mode: wgpu::PresentMode,
    alpha_mode: wgpu::CompositeAlphaMode,
    desired_maximum_frame_latency: u32,
) -> wgpu::SurfaceConfiguration {
    wgpu::SurfaceConfiguration {
        usage: cranpose_render_wgpu::presentable_root_usages(caps.usages),
        format,
        width,
        height,
        present_mode,
        alpha_mode,
        color_space: wgpu::SurfaceColorSpace::Auto,
        view_formats: crate::surface_format::display_surface_view_formats(format),
        desired_maximum_frame_latency,
    }
}

// Draws the app into the surface `frame` through a view in the display
// format of the surface's `format`, at `width` by `height` pixels.
pub(crate) fn render_surface_frame(
    renderer: &mut cranpose_render_wgpu::WgpuRenderer,
    frame: &wgpu::SurfaceTexture,
    format: wgpu::TextureFormat,
    (width, height): (u32, u32),
) -> Result<(), cranpose_render_wgpu::WgpuRendererError> {
    let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
        format: Some(crate::surface_format::display_surface_view_format(format)),
        ..Default::default()
    });
    renderer.render_surface_texture(frame, &view, width, height)
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
) -> bool {
    present_initial_placeholder_frame_cleared_to(
        surface,
        device,
        queue,
        format,
        context,
        cranpose_render_wgpu::frame_clear_color(false),
    )
}

pub(crate) fn present_initial_placeholder_frame_cleared_to(
    surface: &wgpu::Surface<'_>,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
    context: &str,
    clear: wgpu::Color,
) -> bool {
    let SurfaceFrame::Ready(frame) = current_surface_texture(surface, context) else {
        return false;
    };
    let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
        format: Some(format.remove_srgb_suffix()),
        ..Default::default()
    });
    cranpose_render_wgpu::clear_to_background(device, queue, &view, clear);
    queue.present(frame);
    true
}

pub(crate) fn surface_present_required(
    surface_dirty: bool,
    update_visual_changed: bool,
    app_needs_redraw: bool,
) -> bool {
    surface_dirty || update_visual_changed || app_needs_redraw
}

#[cfg(test)]
#[path = "tests/wgpu_surface_tests.rs"]
mod tests;
