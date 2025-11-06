#![deny(missing_docs)]

//! High level utilities for running Compose applications with minimal boilerplate.

#[cfg(not(feature = "desktop"))]
compile_error!("compose-app must be built with the `desktop` feature enabled.");

#[cfg(not(any(feature = "renderer-pixels", feature = "renderer-wgpu")))]
compile_error!("compose-app requires either the `renderer-pixels` or `renderer-wgpu` feature.");

use compose_app_shell::{default_root_key, AppShell};
use compose_platform_desktop_winit::DesktopWinitPlatform;

#[cfg(feature = "renderer-pixels")]
use compose_render_pixels::{draw_scene, PixelsRenderer};
#[cfg(feature = "renderer-pixels")]
use pixels::{Pixels, SurfaceTexture};

#[cfg(feature = "renderer-wgpu")]
use compose_render_wgpu::WgpuRenderer;

use std::sync::Arc;

use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoopBuilder};
use winit::window::WindowBuilder;

/// Builder used to configure and launch a Compose application.
#[derive(Debug, Clone, Default)]
pub struct ComposeAppBuilder {
    options: ComposeAppOptions,
}

impl ComposeAppBuilder {
    /// Creates a new builder with default configuration.
    #[allow(non_snake_case)]
    pub fn New() -> Self {
        Self::default()
    }

    /// Sets the window title for the application.
    #[allow(non_snake_case)]
    pub fn Title(mut self, title: impl Into<String>) -> Self {
        self.options.title = title.into();
        self
    }

    /// Sets the initial logical size of the application window.
    #[allow(non_snake_case)]
    pub fn Size(mut self, width: u32, height: u32) -> Self {
        self.options.initial_size = (width, height);
        self
    }

    /// Runs the application using the configured options and provided Compose content.
    #[allow(non_snake_case)]
    pub fn Run(self, content: impl FnMut() + 'static) -> ! {
        run_app(self.options, content)
    }

    #[doc(hidden)]
    pub fn new() -> Self {
        Self::New()
    }

    #[doc(hidden)]
    pub fn title(self, title: impl Into<String>) -> Self {
        self.Title(title)
    }

    #[doc(hidden)]
    pub fn size(self, width: u32, height: u32) -> Self {
        self.Size(width, height)
    }

    #[doc(hidden)]
    pub fn run(self, content: impl FnMut() + 'static) -> ! {
        self.Run(content)
    }
}

/// Options used to configure the Compose application window.
#[derive(Debug, Clone)]
pub struct ComposeAppOptions {
    title: String,
    initial_size: (u32, u32),
}

impl Default for ComposeAppOptions {
    fn default() -> Self {
        Self {
            title: "Compose App".to_string(),
            initial_size: (800, 600),
        }
    }
}

impl ComposeAppOptions {
    /// Sets the title used for the application window.
    #[allow(non_snake_case)]
    pub fn WithTitle(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Sets the initial window size in logical pixels.
    #[allow(non_snake_case)]
    pub fn WithSize(mut self, width: u32, height: u32) -> Self {
        self.initial_size = (width, height);
        self
    }

    #[doc(hidden)]
    pub fn with_title(self, title: impl Into<String>) -> Self {
        self.WithTitle(title)
    }

    #[doc(hidden)]
    pub fn with_size(self, width: u32, height: u32) -> Self {
        self.WithSize(width, height)
    }
}

/// Launches a Compose application using the default options.
#[allow(non_snake_case)]
pub fn ComposeApp(content: impl FnMut() + 'static) -> ! {
    ComposeAppBuilder::New().Run(content)
}

/// Launches a Compose application using the provided options.
#[allow(non_snake_case)]
pub fn ComposeAppWithOptions(options: ComposeAppOptions, content: impl FnMut() + 'static) -> ! {
    run_app(options, content)
}

/// Alias with Kotlin-inspired casing for use in DSL-like code.
#[allow(non_snake_case)]
#[doc(hidden)]
pub fn composeApp(content: impl FnMut() + 'static) -> ! {
    ComposeApp(content)
}

#[doc(hidden)]
pub fn compose_app(content: impl FnMut() + 'static) -> ! {
    ComposeApp(content)
}

#[doc(hidden)]
pub fn compose_app_with_options(options: ComposeAppOptions, content: impl FnMut() + 'static) -> ! {
    ComposeAppWithOptions(options, content)
}

/// Macro helper that allows calling [`ComposeApp`] using a block without a closure wrapper.
#[macro_export]
macro_rules! ComposeApp {
    (options: $options:expr, { $($body:tt)* }) => {
        $crate::ComposeAppWithOptions($options, || { $($body)* })
    };
    (options: $options:expr, $body:block) => {
        $crate::ComposeAppWithOptions($options, || $body)
    };
    ({ $($body:tt)* }) => {
        $crate::ComposeApp(|| { $($body)* })
    };
    ($body:block) => {
        $crate::ComposeApp(|| $body)
    };
    ($($body:tt)*) => {
        $crate::ComposeApp(|| { $($body)* })
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! composeApp {
    ($($body:tt)*) => {
        $crate::ComposeApp!($($body)*)
    };
}

fn run_app(options: ComposeAppOptions, content: impl FnMut() + 'static) -> ! {
    #[cfg(feature = "renderer-wgpu")]
    {
        run_wgpu_app(&options, content)
    }
    #[cfg(all(feature = "renderer-pixels", not(feature = "renderer-wgpu")))]
    {
        run_pixels_app(&options, content)
    }
}

#[cfg(feature = "renderer-pixels")]
fn run_pixels_app(options: &ComposeAppOptions, content: impl FnMut() + 'static) -> ! {
    let event_loop = EventLoopBuilder::new().build().expect("failed to create event loop");
    let frame_proxy = event_loop.create_proxy();

    let initial_width = options.initial_size.0;
    let initial_height = options.initial_size.1;

    let window = Arc::new(WindowBuilder::new()
        .with_title(options.title.clone())
        .with_inner_size(LogicalSize::new(
            initial_width as f64,
            initial_height as f64,
        ))
        .build(&event_loop)
        .expect("failed to create window"));

    let size = window.inner_size();
    let surface_texture = SurfaceTexture::new(size.width, size.height, window.as_ref());
    let mut pixels = Pixels::new(initial_width, initial_height, surface_texture)
        .expect("failed to create pixel buffer");

    let renderer = PixelsRenderer::new();
    let mut app = AppShell::new(renderer, default_root_key(), content);
    let mut platform = DesktopWinitPlatform::default();
    // Defer updating the platform scale factor until winit notifies us of a
    // change. Using the window's current scale factor here causes pointer
    // coordinates to be scaled twice on high-DPI setups, which breaks
    // hit-testing. The `ScaleFactorChanged` event below keeps the platform in
    // sync instead.

    app.set_frame_waker({
        let proxy = frame_proxy.clone();
        move || {
            let _ = proxy.send_event(());
        }
    });

    app.set_buffer_size(initial_width, initial_height);
    app.set_viewport(size.width as f32, size.height as f32);

    let window_for_event_loop = window.clone();
    let _ = event_loop.run(move |event, elwt| {
        elwt.set_control_flow(ControlFlow::Wait);
        match event {
            Event::WindowEvent { window_id, event } if window_id == window_for_event_loop.id() => match event {
                WindowEvent::CloseRequested => {
                    elwt.exit();
                }
                WindowEvent::Resized(new_size) => {
                    if let Err(err) = pixels.resize_surface(new_size.width, new_size.height) {
                        log::error!("failed to resize surface: {err}");
                        return;
                    }
                    if let Err(err) = pixels.resize_buffer(new_size.width, new_size.height) {
                        log::error!("failed to resize buffer: {err}");
                        return;
                    }
                    app.set_buffer_size(new_size.width, new_size.height);
                    app.set_viewport(new_size.width as f32, new_size.height as f32);
                }
                WindowEvent::ScaleFactorChanged {
                    scale_factor,
                    ..
                } => {
                    platform.set_scale_factor(scale_factor);
                    let new_size = window_for_event_loop.inner_size();
                    if let Err(err) =
                        pixels.resize_surface(new_size.width, new_size.height)
                    {
                        log::error!("failed to resize surface: {err}");
                        return;
                    }
                    if let Err(err) =
                        pixels.resize_buffer(new_size.width, new_size.height)
                    {
                        log::error!("failed to resize buffer: {err}");
                        return;
                    }
                    app.set_buffer_size(new_size.width, new_size.height);
                    app.set_viewport(new_size.width as f32, new_size.height as f32);
                }
                WindowEvent::CursorMoved { position, .. } => {
                    let logical = platform.pointer_position(position);
                    app.set_cursor(logical.x, logical.y);
                    if app.should_render() {
                        app.update();
                        window_for_event_loop.request_redraw();
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

                    let frame = pixels.frame_mut();
                    let (buffer_width, buffer_height) = app.buffer_size();
                    draw_scene(frame, buffer_width, buffer_height, app.scene());
                    if let Err(err) = pixels.render() {
                        log::error!("pixels render failed: {err}");
                    }
                }
                _ => {}
            },
            Event::AboutToWait | Event::UserEvent(()) => {
                if app.should_render() {
                    window_for_event_loop.request_redraw();
                    elwt.set_control_flow(ControlFlow::Poll);
                }
            }
            _ => {}
        }
    });

    std::process::exit(0);
}

#[cfg(feature = "renderer-wgpu")]
fn run_wgpu_app(options: &ComposeAppOptions, content: impl FnMut() + 'static) -> ! {
    let event_loop = EventLoopBuilder::new().build().expect("failed to create event loop");
    let frame_proxy = event_loop.create_proxy();

    let initial_width = options.initial_size.0;
    let initial_height = options.initial_size.1;

    let window = Arc::new(WindowBuilder::new()
        .with_title(options.title.clone())
        .with_inner_size(LogicalSize::new(
            initial_width as f64,
            initial_height as f64,
        ))
        .build(&event_loop)
        .expect("failed to create window"));

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

    // Create renderer
    let mut renderer = WgpuRenderer::new();
    renderer.init_gpu(Arc::new(device), Arc::new(queue), surface_format);

    let mut app = AppShell::new(renderer, default_root_key(), content);
    let mut platform = DesktopWinitPlatform::default();

    app.set_frame_waker({
        let proxy = frame_proxy.clone();
        move || {
            let _ = proxy.send_event(());
        }
    });

    app.set_buffer_size(initial_width, initial_height);
    app.set_viewport(size.width as f32, size.height as f32);

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
                        app.set_buffer_size(new_size.width, new_size.height);
                        app.set_viewport(new_size.width as f32, new_size.height as f32);
                    }
                }
                WindowEvent::ScaleFactorChanged {
                    scale_factor,
                    ..
                } => {
                    platform.set_scale_factor(scale_factor);
                    let new_size = window.inner_size();
                    if new_size.width > 0 && new_size.height > 0 {
                        surface_config.width = new_size.width;
                        surface_config.height = new_size.height;
                        let device = app.renderer().device();
                        surface.configure(device, &surface_config);
                        app.set_buffer_size(new_size.width, new_size.height);
                        app.set_viewport(new_size.width as f32, new_size.height as f32);
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

                    if let Err(err) = app.renderer().render(&view, surface_config.width, surface_config.height) {
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

    std::process::exit(0);
}
