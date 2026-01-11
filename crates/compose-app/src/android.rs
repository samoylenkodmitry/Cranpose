//! Android runtime for Compose applications.
//!
//! This module provides the Android event loop implementation with proper
//! lifecycle management, input handling, and rendering coordination.

use crate::launcher::AppSettings;
use compose_app_shell::{default_root_key, AppShell};
use compose_platform_android::AndroidPlatform;
use compose_render_wgpu::WgpuRenderer;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// GPU resources for the surface (recreated when window is destroyed/created).
struct GpuResources {
    surface: wgpu::Surface<'static>,
    device: Arc<wgpu::Device>,
    config: wgpu::SurfaceConfiguration,
}

/// Get display density from Android NDK Configuration.
///
/// Uses the NDK's AConfiguration_getDensity which returns density constants
/// mapped to the standard Android density classes:
/// - mdpi: 1.0 (160 dpi baseline)
/// - hdpi: 1.5 (240 dpi)
/// - xhdpi: 2.0 (320 dpi) - most common modern phones
/// - xxhdpi: 3.0 (480 dpi)
/// - xxxhdpi: 4.0 (640 dpi)
///
/// The factor is calculated as DPI / 160 per Android NDK documentation.
fn get_display_density(app: &android_activity::AndroidApp) -> f32 {
    let config = app.config();
    let density_dpi = config.density(); // Returns Option<u32> with raw DPI value

    // Convert DPI to scale factor (baseline is 160 dpi = 1.0x)
    // e.g., 320 dpi / 160 = 2.0x (xhdpi)
    density_dpi.map(|dpi| dpi as f32 / 160.0).unwrap_or(2.0) // Fallback to xhdpi (2.0) if density unavailable
}

/// Renders a single frame. Returns true if out of memory (should exit).
fn render_once(resources: &mut GpuResources, shell: &mut AppShell<WgpuRenderer>) -> bool {
    shell.update();

    match resources.surface.get_current_texture() {
        Ok(frame) => {
            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let (width, height) = shell.buffer_size();

            if let Err(e) = shell.renderer().render(&view, width, height) {
                log::error!("Render error: {:?}", e);
            }

            frame.present();
            false
        }
        Err(wgpu::SurfaceError::Lost) | Err(wgpu::SurfaceError::Outdated) => {
            // Reconfigure surface using current size and config
            let (width, height) = shell.buffer_size();
            resources.config.width = width;
            resources.config.height = height;
            resources
                .surface
                .configure(&resources.device, &resources.config);
            false
        }
        Err(wgpu::SurfaceError::OutOfMemory) => {
            log::error!("Out of memory; exiting");
            true
        }
        Err(e) => {
            log::debug!("Surface error: {:?}", e);
            false
        }
    }
}

/// Runs an Android Compose application with wgpu rendering.
///
/// Called by `AppLauncher::run_android()`. This is the framework-level
/// entrypoint that manages the Android lifecycle and event loop.
///
/// **Note:** Applications should use `AppLauncher` instead of calling this directly.
pub fn run(
    app: android_activity::AndroidApp,
    settings: AppSettings,
    content: impl FnMut() + 'static,
) {
    use android_activity::{input::MotionAction, InputStatus, MainEvent, PollEvent};

    // Install panic hook for better crash logging in Logcat
    std::panic::set_hook(Box::new(|panic_info| {
        let location = panic_info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".to_string());
        let message = panic_info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| *s)
            .or_else(|| {
                panic_info
                    .payload()
                    .downcast_ref::<String>()
                    .map(|s| s.as_str())
            })
            .unwrap_or("Box<dyn Any>");
        log::error!("PANIC at {}: {}", location, message);
    }));

    // Wrap content in Rc<RefCell> for reuse across window recreations
    let content = std::rc::Rc::new(std::cell::RefCell::new(content));

    // App shell (created once, persists across window recreations)
    let mut app_shell: Option<AppShell<WgpuRenderer>> = None;

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

    // Exit flag for Destroy event (can't break from inside poll_events closure)
    let should_exit = Arc::new(AtomicBool::new(false));

    // Initialize wgpu instance with GL and Vulkan backends
    // Use DISCARD_HAL_LABELS to prevent crash in emulator's Vulkan debug utils
    // (vk_common_SetDebugUtilsObjectNameEXT crashes on null labels)
    let backends = wgpu::Backends::GL | wgpu::Backends::VULKAN;

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends,
        flags: wgpu::InstanceFlags::empty(), // No debug/validation - prevents label crash
        ..Default::default()
    });

    // Platform abstraction for density/pointer conversion
    let mut android_platform = AndroidPlatform::new();

    // GPU resources (recreated when window is destroyed/created)
    let mut gpu_resources: Option<GpuResources> = None;

    // Main event loop
    loop {
        // Dynamic poll duration:
        // - None when no window (paused, no surface)
        // - ZERO when dirty or animating (immediate rendering)
        // - None when idle (event-driven sleep)
        let poll_duration = if gpu_resources.is_none() {
            None // No window, sleep until next event
        } else if let Some(shell) = &app_shell {
            if shell.needs_redraw() {
                Some(std::time::Duration::ZERO) // Dirty or animating, tight loop
            } else {
                None // Idle, sleep until next event
            }
        } else {
            None
        };

        app.poll_events(poll_duration, |event| {
            match event {
                PollEvent::Main(main_event) => match main_event {
                    MainEvent::InitWindow { .. } => {
                        log::info!("Window initialized, setting up rendering");

                        if let Some(native_window) = app.native_window() {
                            // Get actual window dimensions
                            let width = native_window.width() as u32;
                            let height = native_window.height() as u32;

                            // Create surface using raw window handle from NativeWindow
                            let surface = unsafe {
                                use raw_window_handle::{
                                    AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle,
                                    RawWindowHandle,
                                };

                                let window_handle = AndroidNdkWindowHandle::new(
                                    std::ptr::NonNull::new(native_window.ptr().as_ptr() as *mut _)
                                        .expect("NativeWindow pointer is null"),
                                );
                                let raw_window_handle = RawWindowHandle::AndroidNdk(window_handle);

                                let display_handle = AndroidDisplayHandle::new();
                                let raw_display_handle = RawDisplayHandle::Android(display_handle);

                                let target = wgpu::SurfaceTargetUnsafe::RawHandle {
                                    raw_display_handle,
                                    raw_window_handle,
                                };

                                instance
                                    .create_surface_unsafe(target)
                                    .expect("Failed to create WGPU surface")
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
                            // Use downlevel limits for broad compatibility, with adapter's actual limits
                            let (device, queue) = pollster::block_on(
                                adapter.request_device(&wgpu::DeviceDescriptor {
                                    label: Some("Android Device"),
                                    required_features: wgpu::Features::empty(),
                                    required_limits: wgpu::Limits::downlevel_defaults()
                                        .using_resolution(adapter.limits()),
                                    memory_hints: wgpu::MemoryHints::default(),
                                    trace: wgpu::Trace::Off,
                                }),
                            )
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
                            let density = get_display_density(&app);
                            android_platform.set_scale_factor(density as f64);
                            compose_ui::set_density(density);
                            log::info!("Display density: {:.2}x", density);

                            // Create or reuse app shell
                            if app_shell.is_none() {
                                // First initialization - create renderer and app shell
                                let mut renderer = if let Some(fonts) = settings.fonts {
                                    WgpuRenderer::new_with_fonts(fonts)
                                } else {
                                    WgpuRenderer::new()
                                };
                                renderer.init_gpu(device.clone(), queue.clone(), surface_format);
                                renderer.set_root_scale(density);

                                // Create app shell with content closure
                                let content_clone = content.clone();
                                let shell =
                                    AppShell::new(renderer, default_root_key(), move || {
                                        content_clone.borrow_mut()()
                                    });

                                app_shell = Some(shell);

                                // Wire frame waker for event-driven rendering
                                if let Some(shell) = &mut app_shell {
                                    let need_frame = need_frame.clone();
                                    shell.set_frame_waker(move || {
                                        need_frame.store(true, Ordering::Relaxed);
                                    });
                                }

                                log::info!("App shell created");
                            } else {
                                // Window recreated - reinitialize GPU resources
                                if let Some(shell) = &mut app_shell {
                                    shell.renderer().init_gpu(
                                        device.clone(),
                                        queue.clone(),
                                        surface_format,
                                    );
                                    shell.renderer().set_root_scale(density);
                                    compose_ui::set_density(density);
                                    log::info!("Renderer reinitialized with new GPU resources");
                                }
                            }

                            // Set buffer_size and viewport
                            if let Some(shell) = &mut app_shell {
                                shell.set_buffer_size(width, height);

                                let width_dp = width as f32 / density;
                                let height_dp = height as f32 / density;
                                shell.set_viewport(width_dp, height_dp);
                                log::info!(
                                    "Set viewport to {:.1}x{:.1} dp ({}x{} px at {:.2}x density)",
                                    width_dp,
                                    height_dp,
                                    width,
                                    height,
                                    density
                                );
                            }

                            // Store GPU resources
                            gpu_resources = Some(GpuResources {
                                surface,
                                device,
                                config: surface_config,
                            });

                            log::info!("Rendering initialized successfully");
                        }
                    }
                    MainEvent::TerminateWindow { .. } => {
                        log::info!("Window terminated");
                        gpu_resources = None;
                    }
                    MainEvent::WindowResized { .. } => {
                        if let Some(native_window) = app.native_window() {
                            let width = native_window.width() as u32;
                            let height = native_window.height() as u32;

                            let density = get_display_density(&app);
                            android_platform.set_scale_factor(density as f64);
                            compose_ui::set_density(density);
                            log::info!(
                                "Window resized to {}x{} at {:.2}x density",
                                width,
                                height,
                                density
                            );

                            if let (Some(resources), Some(shell)) =
                                (&mut gpu_resources, &mut app_shell)
                            {
                                if width > 0 && height > 0 {
                                    resources.config.width = width;
                                    resources.config.height = height;
                                    resources
                                        .surface
                                        .configure(&resources.device, &resources.config);

                                    // Set buffer_size to physical pixels
                                    shell.set_buffer_size(width, height);

                                    // Set viewport to logical dp (marks dirty internally)
                                    let width_dp = width as f32 / density;
                                    let height_dp = height as f32 / density;
                                    shell.set_viewport(width_dp, height_dp);

                                    // Update renderer scale
                                    shell.renderer().set_root_scale(density);
                                    compose_ui::set_density(density);
                                }
                            }
                        }
                    }
                    MainEvent::RedrawNeeded { .. } => {
                        if let Some(shell) = &mut app_shell {
                            shell.mark_dirty();
                        }
                    }
                    MainEvent::Pause => {
                        log::info!("App paused");
                    }
                    MainEvent::Resume { .. } => {
                        log::info!("App resumed");
                    }
                    MainEvent::Start => {
                        log::info!("App started");
                    }
                    MainEvent::Stop => {
                        log::info!("App stopped");
                    }
                    MainEvent::SaveState { .. } => {
                        log::info!("Save state requested (hook for future serialization)");
                    }
                    MainEvent::Destroy => {
                        log::info!("App destroy requested, will exit after this event");
                        should_exit.store(true, Ordering::Relaxed);
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
                                        // Get pointer position in physical pixels and convert to logical dp
                                        let pointer = motion_event.pointer_at_index(0);
                                        let x_px = pointer.x() as f64;
                                        let y_px = pointer.y() as f64;
                                        let logical = android_platform.pointer_position(x_px, y_px);

                                        match motion_event.action() {
                                            MotionAction::Down | MotionAction::PointerDown => {
                                                println!(
                                                    "[TOUCH] Down at ({:.1}, {:.1})",
                                                    logical.x, logical.y
                                                );
                                                if let Some(shell) = &mut app_shell {
                                                    shell.set_cursor(logical.x, logical.y);
                                                    shell.pointer_pressed();
                                                }
                                            }
                                            MotionAction::Up | MotionAction::PointerUp => {
                                                println!(
                                                    "[TOUCH] Up at ({:.1}, {:.1})",
                                                    logical.x, logical.y
                                                );
                                                if let Some(shell) = &mut app_shell {
                                                    shell.set_cursor(logical.x, logical.y);
                                                    shell.pointer_released();
                                                }
                                            }
                                            MotionAction::Move => {
                                                println!(
                                                    "[TOUCH] Move at ({:.1}, {:.1})",
                                                    logical.x, logical.y
                                                );
                                                if let Some(shell) = &mut app_shell {
                                                    shell.set_cursor(logical.x, logical.y);
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
            if let Some(shell) = &mut app_shell {
                shell.mark_dirty();
            }
        }

        // Check if Destroy event requested exit
        if should_exit.load(Ordering::Relaxed) {
            log::info!("Exiting cleanly after Destroy event");
            break;
        }

        // Render outside event callback if needed
        if let (Some(resources), Some(shell)) = (&mut gpu_resources, &mut app_shell) {
            if shell.needs_redraw() {
                if render_once(resources, shell) {
                    break; // Out of memory, exit
                }
            }
        }
    }
}
