//! Desktop runtime for Compose applications.
//!
//! This module provides the desktop event loop implementation using winit.

use crate::launcher::AppSettings;
use compose_app_shell::{default_root_key, AppShell};
use compose_platform_desktop_winit::DesktopWinitPlatform;
use compose_render_wgpu::WgpuRenderer;
use std::sync::Arc;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoopBuilder};
use winit::window::WindowBuilder;

/// Runs a desktop Compose application with wgpu rendering.
///
/// Called by `AppLauncher::run_desktop()`. This is the framework-level
/// entrypoint that manages the desktop event loop and rendering.
///
/// **Note:** Applications should use `AppLauncher` instead of calling this directly.
pub fn run(settings: AppSettings, content: impl FnMut() + 'static) -> ! {
    let event_loop = EventLoopBuilder::new()
        .build()
        .expect("failed to create event loop");
    let frame_proxy = event_loop.create_proxy();

    let initial_width = settings.initial_width;
    let initial_height = settings.initial_height;

    let window = Arc::new(
        WindowBuilder::new()
            .with_title(settings.window_title)
            .with_inner_size(LogicalSize::new(
                initial_width as f64,
                initial_height as f64,
            ))
            .build(&event_loop)
            .expect("failed to create window"),
    );

    // Initialize WGPU
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });

    let surface = instance
        .create_surface(window.clone())
        .expect("failed to create surface");

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
    }))
    .expect("failed to find suitable adapter");

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("Main Device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
        },
        None,
    ))
    .expect("failed to create device");

    let size = window.inner_size();
    let surface_caps = surface.get_capabilities(&adapter);
    let surface_format = surface_caps
        .formats
        .iter()
        .copied()
        .find(|f| f.is_srgb())
        .unwrap_or(surface_caps.formats[0]);

    let mut surface_config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width: size.width,
        height: size.height,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: surface_caps.alpha_modes[0],
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };

    surface.configure(&device, &surface_config);

    // Create renderer and set initial scale
    let mut renderer = WgpuRenderer::new();
    renderer.init_gpu(Arc::new(device), Arc::new(queue), surface_format);
    let initial_scale = window.scale_factor();
    renderer.set_root_scale(initial_scale as f32);

    let mut app = AppShell::new(renderer, default_root_key(), content);
    let mut platform = DesktopWinitPlatform::default();
    platform.set_scale_factor(initial_scale);

    app.set_frame_waker({
        let proxy = frame_proxy.clone();
        move || {
            let _ = proxy.send_event(());
        }
    });

    // Set buffer_size to physical pixels and viewport to logical dp
    app.set_buffer_size(size.width, size.height);
    let logical_width = size.width as f32 / initial_scale as f32;
    let logical_height = size.height as f32 / initial_scale as f32;
    app.set_viewport(logical_width, logical_height);

    let _ = event_loop.run(move |event, elwt| {
        elwt.set_control_flow(ControlFlow::Wait);
        match event {
            Event::WindowEvent { window_id, event } if window_id == window.id() => match event {
                WindowEvent::CloseRequested => {
                    elwt.exit();
                }
                WindowEvent::Resized(new_size) => {
                    if new_size.width > 0 && new_size.height > 0 {
                        surface_config.width = new_size.width;
                        surface_config.height = new_size.height;
                        let device = app.renderer().device();
                        surface.configure(device, &surface_config);

                        let scale_factor = window.scale_factor();
                        let logical_width = new_size.width as f32 / scale_factor as f32;
                        let logical_height = new_size.height as f32 / scale_factor as f32;

                        app.set_buffer_size(new_size.width, new_size.height);
                        app.set_viewport(logical_width, logical_height);
                    }
                }
                WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                    platform.set_scale_factor(scale_factor);
                    app.renderer().set_root_scale(scale_factor as f32);

                    let new_size = window.inner_size();
                    if new_size.width > 0 && new_size.height > 0 {
                        surface_config.width = new_size.width;
                        surface_config.height = new_size.height;
                        let device = app.renderer().device();
                        surface.configure(device, &surface_config);

                        let logical_width = new_size.width as f32 / scale_factor as f32;
                        let logical_height = new_size.height as f32 / scale_factor as f32;

                        app.set_buffer_size(new_size.width, new_size.height);
                        app.set_viewport(logical_width, logical_height);
                    }
                }
                WindowEvent::CursorMoved { position, .. } => {
                    let logical = platform.pointer_position(position);
                    app.set_cursor(logical.x, logical.y);
                    if app.should_render() {
                        app.update();
                        window.request_redraw();
                    }
                }
                WindowEvent::MouseInput {
                    state,
                    button: MouseButton::Left,
                    ..
                } => match state {
                    ElementState::Pressed => app.pointer_pressed(),
                    ElementState::Released => app.pointer_released(),
                },
                WindowEvent::KeyboardInput { event, .. } => {
                    use winit::keyboard::{KeyCode, PhysicalKey};
                    if event.state == ElementState::Pressed {
                        if let PhysicalKey::Code(KeyCode::KeyD) = event.physical_key {
                            app.log_debug_info();
                        }
                    }
                }
                WindowEvent::RedrawRequested => {
                    app.update();

                    let output = match surface.get_current_texture() {
                        Ok(output) => output,
                        Err(err) => {
                            log::error!("failed to get surface texture: {err}");
                            return;
                        }
                    };

                    let view = output
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());

                    if let Err(err) =
                        app.renderer()
                            .render(&view, surface_config.width, surface_config.height)
                    {
                        log::error!("render failed: {err:?}");
                        return;
                    }

                    output.present();
                }
                _ => {}
            },
            Event::AboutToWait | Event::UserEvent(()) => {
                if app.should_render() {
                    window.request_redraw();
                    elwt.set_control_flow(ControlFlow::Poll);
                }
            }
            _ => {}
        }
    });

    std::process::exit(0)
}
