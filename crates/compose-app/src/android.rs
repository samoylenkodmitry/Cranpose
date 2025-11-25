//! Android runtime for Compose applications.
//!
//! This module provides the Android event loop implementation with proper
//! lifecycle management, input handling, and rendering coordination.

use crate::launcher::AppSettings;
use compose_app_shell::{default_root_key, AppShell};
use compose_platform_android::AndroidPlatform;
use compose_render_wgpu::{RendererConfig, WgpuRenderer};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// Surface state tuple containing all wgpu resources and the app shell.
type SurfaceState = (
    wgpu::Surface<'static>,
    Arc<wgpu::Device>,
    Arc<wgpu::Queue>,
    wgpu::SurfaceConfiguration,
    AppShell<WgpuRenderer>,
);

/// Get display density placeholder.
/// TODO: Wire proper density from Java/Kotlin side via extern "C" function.
fn get_display_density() -> f32 {
    // For now treat everything as 1.0 scale on Android.
    // When we wire a proper Java → Rust bridge for DisplayMetrics,
    // we can replace this.
    1.0
}

/// Runs an Android Compose application with wgpu rendering.
///
/// Called by `AppLauncher::run_android()`. This is the framework-level
/// entrypoint that manages the Android lifecycle and event loop.
///
/// **Note:** Applications should use `AppLauncher` instead of calling this directly.
pub fn run(
    app: android_activity::AndroidApp,
    _settings: AppSettings,
    content: impl FnMut() + 'static,
) {
    use android_activity::{input::MotionAction, InputStatus, MainEvent, PollEvent};

    // Wrap content in Option so we can move it out when creating AppShell
    let mut content = Some(content);

    // Initialize logging
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("ComposeRS")
            .with_filter(
                android_logger::FilterBuilder::new()
                    .filter_level(log::LevelFilter::Info)
                    .filter_module("wgpu_core", log::LevelFilter::Warn)
                    .filter_module("wgpu_hal", log::LevelFilter::Warn)
                    .filter_module("naga", log::LevelFilter::Warn)
                    .build(),
            ),
    );

    log::info!("Starting Compose Android Application");

    // Frame wake flag for event-driven rendering
    let need_frame = Arc::new(AtomicBool::new(false));

    // Configure renderer for Android quirks
    let renderer_config = RendererConfig {
        force_atlas_recreation: false,
        base_scale_factor: 1.0,
        debug_text_logging: false,
    };

    // Initialize wgpu instance with GL and Vulkan backends
    // GL works better on emulators, but Vulkan is preferred on real devices
    let backends = wgpu::Backends::GL | wgpu::Backends::VULKAN;

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        ..Default::default()
    });

    // Platform abstraction for density/pointer conversion
    let mut android_platform = AndroidPlatform::new();

    // Surface state (initialized when window is ready)
    let mut surface_state: Option<SurfaceState> = None;

    let mut window_size = (0u32, 0u32);
    let mut needs_redraw = false;

    // Track if we just did a recomposition in WindowResized to avoid duplicate update()
    let mut skip_next_update = false;

    // Main event loop
    loop {
        app.poll_events(Some(std::time::Duration::from_millis(1)), |event| {
            match event {
                PollEvent::Main(main_event) => match main_event {
                    MainEvent::InitWindow { .. } => {
                        log::info!("Window initialized, setting up rendering");

                        if let Some(native_window) = app.native_window() {
                            // Get actual window dimensions
                            let width = native_window.width() as u32;
                            let height = native_window.height() as u32;
                            window_size = (width, height);

                            use raw_window_handle::{
                                AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle,
                                RawWindowHandle,
                            };

                            // Create surface from the Android window
                            let surface = unsafe {
                                let window_handle = AndroidNdkWindowHandle::new(
                                    std::ptr::NonNull::new(native_window.ptr().as_ptr() as *mut _)
                                        .expect("Null window pointer"),
                                );
                                let display_handle = AndroidDisplayHandle::new();

                                let raw_window_handle = RawWindowHandle::AndroidNdk(window_handle);
                                let raw_display_handle = RawDisplayHandle::Android(display_handle);

                                let target = wgpu::SurfaceTargetUnsafe::RawHandle {
                                    raw_display_handle,
                                    raw_window_handle,
                                };

                                instance
                                    .create_surface_unsafe(target)
                                    .expect("Failed to create surface")
                            };

                            // Request adapter
                            let adapter = pollster::block_on(instance.request_adapter(
                                &wgpu::RequestAdapterOptions {
                                    power_preference: wgpu::PowerPreference::HighPerformance,
                                    compatible_surface: Some(&surface),
                                    force_fallback_adapter: false,
                                },
                            ))
                            .expect("Failed to find suitable adapter");

                            let adapter_info = adapter.get_info();
                            log::info!("Found adapter: {:?}", adapter_info.backend);

                            // Request device and queue
                            let (device, queue) = pollster::block_on(adapter.request_device(
                                &wgpu::DeviceDescriptor {
                                    label: Some("Android Device"),
                                    required_features: wgpu::Features::empty(),
                                    required_limits: wgpu::Limits::default(),
                                },
                                None,
                            ))
                            .expect("Failed to create device");

                            let device = Arc::new(device);
                            let queue = Arc::new(queue);

                            // Get surface capabilities and format
                            let surface_caps = surface.get_capabilities(&adapter);
                            let surface_format = surface_caps
                                .formats
                                .iter()
                                .copied()
                                .find(|f| f.is_srgb())
                                .unwrap_or(surface_caps.formats[0]);

                            // Configure surface
                            let surface_config = wgpu::SurfaceConfiguration {
                                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                                format: surface_format,
                                width,
                                height,
                                present_mode: wgpu::PresentMode::Fifo,
                                alpha_mode: surface_caps.alpha_modes[0],
                                view_formats: vec![],
                                desired_maximum_frame_latency: 2,
                            };

                            surface.configure(&device, &surface_config);

                            // Get display density and update platform
                            let density = get_display_density();
                            android_platform.set_scale_factor(density as f64);
                            log::info!("Display density: {:.2}x", density);

                            // Create renderer with Android configuration
                            let mut renderer = WgpuRenderer::with_config(renderer_config.clone());
                            renderer.init_gpu(device.clone(), queue.clone(), surface_format);
                            renderer.set_root_scale(1.0);

                            // Create app shell with content (take from Option)
                            let mut app_shell = AppShell::new(
                                renderer,
                                default_root_key(),
                                content.take().expect("content already used"),
                            );

                            // Wire frame waker for event-driven rendering
                            {
                                let need_frame = need_frame.clone();
                                app_shell.set_frame_waker(move || {
                                    need_frame.store(true, Ordering::Relaxed);
                                });
                            }

                            app_shell.set_buffer_size(width, height);

                            // Set viewport to physical pixels (matches desktop behavior)
                            app_shell.set_viewport(width as f32, height as f32);
                            log::info!(
                                "Set viewport to {}x{} physical pixels at {:.2}x density",
                                width,
                                height,
                                density
                            );

                            surface_state = Some((surface, device, queue, surface_config, app_shell));

                            log::info!("Rendering initialized successfully");
                        }
                    }
                    MainEvent::TerminateWindow { .. } => {
                        log::info!("Window terminated");
                        surface_state = None;
                    }
                    MainEvent::WindowResized { .. } => {
                        if let Some(native_window) = app.native_window() {
                            let width = native_window.width() as u32;
                            let height = native_window.height() as u32;
                            window_size = (width, height);

                            let density = get_display_density();
                            android_platform.set_scale_factor(density as f64);
                            log::info!(
                                "Window resized to {}x{} at {:.2}x density",
                                width,
                                height,
                                density
                            );

                            if let Some((surface, device, _, surface_config, app_shell)) =
                                &mut surface_state
                            {
                                if width > 0 && height > 0 {
                                    surface_config.width = width;
                                    surface_config.height = height;
                                    surface.configure(device, surface_config);
                                    app_shell.set_buffer_size(width, height);

                                    // Set viewport to physical pixels (matches desktop behavior)
                                    app_shell.set_viewport(width as f32, height as f32);

                                    // Force immediate recomposition after viewport change
                                    app_shell.update();
                                    skip_next_update = true;
                                    log::info!("Forced recomposition after viewport change");
                                }
                            }
                        }
                    }
                    MainEvent::RedrawNeeded { .. } => {
                        needs_redraw = true;
                    }
                    _ => {}
                },
                // Handle input events to prevent ANR
                _ => {
                    if let Ok(mut iter) = app.input_events_iter() {
                        loop {
                            if !iter.next(|event| {
                                let handled = match event {
                                    android_activity::input::InputEvent::MotionEvent(
                                        motion_event,
                                    ) => {
                                        // Get pointer position in physical pixels (matches viewport coordinates)
                                        let pointer = motion_event.pointer_at_index(0);
                                        let x = pointer.x();
                                        let y = pointer.y();

                                        log::info!("MotionEvent: action={:?} pos=({:.1}, {:.1})", motion_event.action(), x, y);

                                        match motion_event.action() {
                                            MotionAction::Down | MotionAction::PointerDown => {
                                                if let Some((_, _, _, _, app_shell)) =
                                                    &mut surface_state
                                                {
                                                    app_shell.set_cursor(x, y);
                                                    app_shell.pointer_pressed();
                                                    needs_redraw = true;
                                                }
                                            }
                                            MotionAction::Up | MotionAction::PointerUp => {
                                                if let Some((_, _, _, _, app_shell)) =
                                                    &mut surface_state
                                                {
                                                    app_shell.set_cursor(x, y);
                                                    app_shell.pointer_released();
                                                    needs_redraw = true;
                                                }
                                            }
                                            MotionAction::Move => {
                                                if let Some((_, _, _, _, app_shell)) =
                                                    &mut surface_state
                                                {
                                                    app_shell.set_cursor(x, y);
                                                    if app_shell.should_render() {
                                                        needs_redraw = true;
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                        true
                                    }
                                    _ => false,
                                };

                                if handled {
                                    InputStatus::Handled
                                } else {
                                    InputStatus::Unhandled
                                }
                            }) {
                                break;
                            }
                        }
                    }
                }
            }
        });

        // Check if app side requested a frame (animations, state changes)
        if need_frame.swap(false, Ordering::Relaxed) {
            needs_redraw = true;
        }

        // Render outside event callback
        if needs_redraw && surface_state.is_some() {
            if let Some((surface, _, _, _, app_shell)) = &mut surface_state {
                // Skip update if we just did it in WindowResized
                if skip_next_update {
                    skip_next_update = false;
                } else {
                    app_shell.update();
                }

                match surface.get_current_texture() {
                    Ok(frame) => {
                        let view =
                            frame
                                .texture
                                .create_view(&wgpu::TextureViewDescriptor::default());
                        let (width, height) = app_shell.buffer_size();

                        if let Err(e) = app_shell.renderer().render(&view, width, height) {
                            log::error!("Render error: {:?}", e);
                        }

                        frame.present();
                    }
                    Err(wgpu::SurfaceError::Lost) => {
                        log::warn!("Surface lost, will be reconfigured");
                    }
                    Err(wgpu::SurfaceError::OutOfMemory) => {
                        log::error!("Out of memory!");
                        break;
                    }
                    Err(e) => {
                        log::debug!("Surface error: {:?}", e);
                    }
                }
            }
            needs_redraw = false;
        }
    }
}
