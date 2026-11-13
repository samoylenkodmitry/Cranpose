use std::sync::Arc;

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use winit::window::Window;

use super::request_redraw_once;
use crate::{app_launcher::AppSettings, window_present::WindowPresent};

// The primary window's surface, its configuration and the viewport a
// headless run asks for in place of the window's.
pub(super) struct PrimarySurface<'a> {
    pub(super) window: &'a Arc<dyn Window>,
    pub(super) present: &'a WindowPresent,
    pub(super) config: &'a mut wgpu::SurfaceConfiguration,
    pub(super) viewport_override: Option<(f32, f32)>,
}

impl PrimarySurface<'_> {
    // Configures the surface at `width` by `height` and gives `app` the
    // viewport that size shows.
    pub(super) fn configure(&mut self, app: &mut AppShell<WgpuRenderer>, width: u32, height: u32) {
        let viewport = viewport_for_surface_size(
            self.viewport_override,
            width,
            height,
            self.window.scale_factor(),
        );
        configure_app_surface_size(app, self.present, self.config, width, height, viewport);
    }
}

fn configure_app_surface_size(
    app: &mut AppShell<WgpuRenderer>,
    present: &WindowPresent,
    surface_config: &mut wgpu::SurfaceConfiguration,
    width: u32,
    height: u32,
    viewport: (f32, f32),
) {
    if width == 0 || height == 0 {
        return;
    }
    let previous = (surface_config.width, surface_config.height);
    surface_config.width = width;
    surface_config.height = height;
    if !present.configure(app.renderer(), surface_config) {
        (surface_config.width, surface_config.height) = previous;
        return;
    }
    update_app_viewport(app, width, height, viewport);
}

fn update_app_viewport(
    app: &mut AppShell<WgpuRenderer>,
    width: u32,
    height: u32,
    viewport: (f32, f32),
) {
    let (logical_width, logical_height) = viewport;
    app.set_buffer_size(width, height);
    app.set_viewport(logical_width, logical_height);
    cranpose_services::publish_host_surface_size(
        cranpose_services::host_surface::HostSurfaceSize {
            width: logical_width,
            height: logical_height,
            scale: if logical_width > 0.0 {
                width as f32 / logical_width
            } else {
                1.0
            },
        },
    );
}

pub(super) fn surface_logical_viewport_size(
    width: u32,
    height: u32,
    scale_factor: f64,
) -> (f32, f32) {
    (
        width as f32 / scale_factor as f32,
        height as f32 / scale_factor as f32,
    )
}

pub(super) fn headless_requested_viewport(settings: &AppSettings) -> Option<(f32, f32)> {
    settings.headless.then_some((
        settings.initial_width.max(1) as f32,
        settings.initial_height.max(1) as f32,
    ))
}

fn viewport_for_surface_size(
    requested_viewport: Option<(f32, f32)>,
    width: u32,
    height: u32,
    scale_factor: f64,
) -> (f32, f32) {
    requested_viewport.unwrap_or_else(|| surface_logical_viewport_size(width, height, scale_factor))
}

pub(super) fn apply_primary_surface_resize(
    app: &mut AppShell<WgpuRenderer>,
    primary: &mut PrimarySurface<'_>,
    width: u32,
    height: u32,
    primary_surface_dirty: &mut bool,
    primary_redraw_pending: &mut bool,
) {
    if width == 0 || height == 0 {
        return;
    }
    primary.configure(app, width, height);
    *primary_surface_dirty = true;
    *primary_redraw_pending = false;
    request_redraw_once(primary.window, primary_redraw_pending);
}

pub(super) fn primary_viewport_for_surface_size(
    settings: &AppSettings,
    width: u32,
    height: u32,
    scale_factor: f64,
) -> (f32, f32) {
    viewport_for_surface_size(
        headless_requested_viewport(settings),
        width,
        height,
        scale_factor,
    )
}
