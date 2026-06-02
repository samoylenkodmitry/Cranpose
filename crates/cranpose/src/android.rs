//! Android runtime for Compose applications.
//!
//! This module provides the Android event loop implementation with proper
//! lifecycle management, input handling, and rendering coordination.

use crate::{
    android_host_window,
    android_jni::{clear_pending_android_jni_exception, with_android_activity_env},
    android_overlay_window,
    android_surface::{create_android_wgpu_surface, AndroidSurfaceError},
    launcher::{AndroidOverlayWindowOptions, AppSettings},
    wgpu_surface::{current_surface_texture, SurfaceFrame},
};
use cranpose_app_shell::{default_root_key, AppShell, PlatformFrameDriver};
use cranpose_platform_android::AndroidPlatform;
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::{Point, Size};
use ndk::native_window::NativeWindow;
use std::time::{Duration, Instant};
use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr::NonNull,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

/// GPU resources for the current Android surface and its reusable WGPU device.
struct GpuResources {
    surface: wgpu::Surface<'static>,
    native_window_ptr: NonNull<c_void>,
    adapter: Arc<wgpu::Adapter>,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface_format: wgpu::TextureFormat,
    backend: wgpu::Backend,
    config: wgpu::SurfaceConfiguration,
    _native_window: Option<NativeWindow>,
}

/// Pending input event to be processed outside poll_events callback.
/// This prevents blocking the main thread during input event acknowledgment.
enum PendingInput {
    PointerDown(f32, f32),
    PointerUp(f32, f32),
    PointerMove(f32, f32),
}

fn pending_input_from_android_event(
    event: &android_activity::input::InputEvent<'_>,
    android_platform: &AndroidPlatform,
) -> Option<PendingInput> {
    let android_activity::input::InputEvent::MotionEvent(motion_event) = event else {
        return None;
    };

    let pointer = motion_event.pointer_at_index(0);
    let logical = android_platform.pointer_position(pointer.x() as f64, pointer.y() as f64);
    match motion_event.action() {
        android_activity::input::MotionAction::Down
        | android_activity::input::MotionAction::PointerDown => Some(PendingInput::PointerDown(
            logical.x as f32,
            logical.y as f32,
        )),
        android_activity::input::MotionAction::Up
        | android_activity::input::MotionAction::PointerUp => {
            Some(PendingInput::PointerUp(logical.x as f32, logical.y as f32))
        }
        android_activity::input::MotionAction::Move => Some(PendingInput::PointerMove(
            logical.x as f32,
            logical.y as f32,
        )),
        _ => None,
    }
}

fn drain_android_input_events(
    app: &android_activity::AndroidApp,
    android_platform: &AndroidPlatform,
    pending_inputs: &mut Vec<PendingInput>,
) {
    let Ok(mut iter) = app.input_events_iter() else {
        return;
    };

    for _ in 0..MAX_ANDROID_INPUT_EVENTS_PER_POLL {
        let event_available = iter.next(|event| {
            if let Some(input) = pending_input_from_android_event(event, android_platform) {
                pending_inputs.push(input);
                android_activity::InputStatus::Handled
            } else {
                android_activity::InputStatus::Unhandled
            }
        });
        if !event_available {
            break;
        }
    }
}

const MAX_ANDROID_INPUT_EVENTS_PER_POLL: usize = 10;

#[derive(Clone, Copy)]
struct PendingHostWindowSizeRequest {
    state: Option<android_host_window::AndroidHostWindowState>,
    requested: Size,
    requested_at: Instant,
}

struct AndroidFrameDriver {
    need_frame: Arc<AtomicBool>,
    app_waker: android_activity::AndroidAppWaker,
    next_deadline: Cell<Option<web_time::Instant>>,
}

impl AndroidFrameDriver {
    fn new(app_waker: android_activity::AndroidAppWaker) -> Self {
        Self {
            need_frame: Arc::new(AtomicBool::new(false)),
            app_waker,
            next_deadline: Cell::new(None),
        }
    }

    fn frame_waker(&self) -> impl Fn() + Send + Sync + 'static {
        let need_frame = self.need_frame.clone();
        let app_waker = self.app_waker.clone();
        move || {
            need_frame.store(true, Ordering::Relaxed);
            app_waker.wake();
        }
    }

    fn frame_requested(&self) -> bool {
        self.need_frame.load(Ordering::Relaxed)
    }

    fn take_frame_request(&self) -> bool {
        self.need_frame.swap(false, Ordering::Relaxed)
    }

    fn deadline_timeout(&self) -> Option<Duration> {
        self.next_deadline.get().map(duration_until_frame_deadline)
    }
}

impl PlatformFrameDriver for AndroidFrameDriver {
    fn request_frame(&self) {
        self.need_frame.store(true, Ordering::Relaxed);
        self.app_waker.wake();
    }

    fn request_wake_at(&self, deadline: web_time::Instant) {
        self.next_deadline.set(Some(deadline));
    }

    fn clear_wake(&self) {
        self.next_deadline.set(None);
    }
}

fn duration_until_frame_deadline(deadline: web_time::Instant) -> Duration {
    deadline
        .checked_duration_since(web_time::Instant::now())
        .unwrap_or(Duration::ZERO)
}

fn earliest_android_poll_timeout(
    first: Option<Duration>,
    second: Option<Duration>,
) -> Option<Duration> {
    match (first, second) {
        (Some(first), Some(second)) => Some(first.min(second)),
        (Some(duration), None) | (None, Some(duration)) => Some(duration),
        (None, None) => None,
    }
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

fn update_android_platform_geometry(
    app: &android_activity::AndroidApp,
    android_platform: &mut AndroidPlatform,
) -> f32 {
    let density = get_display_density(app);
    android_platform.set_scale_factor(density as f64);

    let content_rect = app.content_rect();
    if content_rect.right > content_rect.left && content_rect.bottom > content_rect.top {
        android_platform
            .set_input_surface_offset_px(content_rect.left as f64, content_rect.top as f64);
    } else {
        android_platform.set_input_surface_offset_px(0.0, 0.0);
    }

    density
}

fn update_android_shell_geometry(
    shell: &mut AppShell<WgpuRenderer>,
    density: f32,
    host_window_registry: &android_host_window::AndroidHostWindowRegistry,
) -> Option<Size> {
    shell.renderer().set_root_scale(density);
    shell.set_density(density);

    let (width, height) = shell.buffer_size();
    if width > 0 && height > 0 {
        let width_dp = width as f32 / density;
        let height_dp = height as f32 / density;
        shell.set_viewport(width_dp, height_dp);
        let actual = Size::new(width_dp, height_dp);
        android_host_window::sync_android_host_window_actual_size(host_window_registry, actual);
        Some(actual)
    } else {
        None
    }
}

/// Renders a single frame. Returns true if out of memory (should exit).
fn render_once(resources: &mut GpuResources, shell: &mut AppShell<WgpuRenderer>) -> bool {
    match current_surface_texture(&resources.surface, "android") {
        SurfaceFrame::Ready(frame) => {
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
        SurfaceFrame::Reconfigure => {
            let (width, height) = shell.buffer_size();
            resources.config.width = width;
            resources.config.height = height;
            resources
                .surface
                .configure(&resources.device, &resources.config);
            false
        }
        SurfaceFrame::Skip => false,
    }
}

struct AndroidGpuSetup {
    resources: GpuResources,
    renderer_needs_init: bool,
}

fn initialize_android_rendering<F>(
    instance: &wgpu::Instance,
    existing_resources: Option<GpuResources>,
    app_shell: &mut Option<AppShell<WgpuRenderer>>,
    content: &Rc<RefCell<F>>,
    settings: &AppSettings,
    frame_driver: &AndroidFrameDriver,
    host_window_registry: &android_host_window::AndroidHostWindowRegistry,
    native_window_ptr: NonNull<c_void>,
    native_window_owner: Option<NativeWindow>,
    width: u32,
    height: u32,
    density: f32,
) -> Result<(GpuResources, Option<Size>), AndroidSurfaceError>
where
    F: FnMut() + 'static,
{
    let setup = create_android_gpu_resources(
        instance,
        existing_resources,
        native_window_ptr,
        native_window_owner,
        width,
        height,
    )?;

    if app_shell.is_none() {
        let fonts: &[&[u8]] = settings.fonts.unwrap_or(&[]);
        let mut renderer = WgpuRenderer::new(fonts);
        renderer.init_gpu(
            setup.resources.device.clone(),
            setup.resources.queue.clone(),
            setup.resources.surface_format,
            setup.resources.backend,
        );

        let content_clone = content.clone();
        let density = density.max(f32::EPSILON);
        let shell = AppShell::new_with_size_and_density(
            renderer,
            default_root_key(),
            move || content_clone.borrow_mut()(),
            (width, height),
            (width as f32 / density, height as f32 / density),
            density,
        );

        *app_shell = Some(shell);

        if let Some(shell) = app_shell {
            shell.set_frame_waker(frame_driver.frame_waker());
        }

        log::info!("App shell created");
    } else if setup.renderer_needs_init {
        if let Some(shell) = app_shell {
            shell.renderer().init_gpu(
                setup.resources.device.clone(),
                setup.resources.queue.clone(),
                setup.resources.surface_format,
                setup.resources.backend,
            );
            log::info!("Renderer reinitialized with new Android GPU pipeline resources");
        }
    } else {
        log::debug!("Reused Android WGPU device and renderer resources for surface update");
    }

    if let Some(shell) = app_shell {
        shell.renderer().set_root_scale(density);
        shell.set_density(density);
    }

    let actual_size = app_shell.as_mut().and_then(|shell| {
        shell.set_buffer_size(width, height);
        update_android_shell_geometry(shell, density, host_window_registry)
    });

    Ok((setup.resources, actual_size))
}

fn create_android_gpu_resources(
    instance: &wgpu::Instance,
    existing_resources: Option<GpuResources>,
    native_window_ptr: NonNull<c_void>,
    native_window_owner: Option<NativeWindow>,
    width: u32,
    height: u32,
) -> Result<AndroidGpuSetup, AndroidSurfaceError> {
    if let Some(mut resources) = existing_resources {
        if resources.native_window_ptr == native_window_ptr {
            resources.config.width = width;
            resources.config.height = height;
            resources
                .surface
                .configure(&resources.device, &resources.config);
            if let Some(native_window_owner) = native_window_owner {
                resources._native_window = Some(native_window_owner);
            }
            return Ok(AndroidGpuSetup {
                resources,
                renderer_needs_init: false,
            });
        }

        return create_android_gpu_resources_for_existing_device(
            instance,
            &resources,
            native_window_ptr,
            native_window_owner,
            width,
            height,
        );
    }

    let surface = create_android_wgpu_surface(instance, native_window_ptr)?;

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
    }))?;

    let adapter_info = adapter.get_info();
    log::info!("Found adapter: {:?}", adapter_info.backend);
    let adapter = Arc::new(adapter);

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Android Device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))?;

    let device = Arc::new(device);
    let queue = Arc::new(queue);
    let config = create_android_surface_config(&surface, &adapter, width, height)?;
    surface.configure(&device, &config);

    Ok(AndroidGpuSetup {
        resources: GpuResources {
            surface,
            native_window_ptr,
            adapter,
            device,
            queue,
            surface_format: config.format,
            backend: adapter_info.backend,
            config,
            _native_window: native_window_owner,
        },
        renderer_needs_init: true,
    })
}

fn create_android_gpu_resources_for_existing_device(
    instance: &wgpu::Instance,
    existing: &GpuResources,
    native_window_ptr: NonNull<c_void>,
    native_window_owner: Option<NativeWindow>,
    width: u32,
    height: u32,
) -> Result<AndroidGpuSetup, AndroidSurfaceError> {
    let surface = create_android_wgpu_surface(instance, native_window_ptr)?;
    let config = create_android_surface_config(&surface, &existing.adapter, width, height)?;
    surface.configure(&existing.device, &config);
    let renderer_needs_init = config.format != existing.surface_format;

    Ok(AndroidGpuSetup {
        resources: GpuResources {
            surface,
            native_window_ptr,
            adapter: existing.adapter.clone(),
            device: existing.device.clone(),
            queue: existing.queue.clone(),
            surface_format: config.format,
            backend: existing.backend,
            config,
            _native_window: native_window_owner,
        },
        renderer_needs_init,
    })
}

fn create_android_surface_config(
    surface: &wgpu::Surface<'static>,
    adapter: &wgpu::Adapter,
    width: u32,
    height: u32,
) -> Result<wgpu::SurfaceConfiguration, AndroidSurfaceError> {
    let surface_caps = surface.get_capabilities(&adapter);
    let surface_format = surface_caps
        .formats
        .iter()
        .copied()
        .find(|f| f.is_srgb())
        .or_else(|| surface_caps.formats.first().copied())
        .ok_or(AndroidSurfaceError::NoSurfaceFormat)?;
    let alpha_mode = surface_caps
        .alpha_modes
        .first()
        .copied()
        .ok_or(AndroidSurfaceError::NoAlphaMode)?;
    let present_mode = crate::present_mode::select_present_mode(&surface_caps);
    Ok(wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width,
        height,
        present_mode,
        alpha_mode,
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    })
}

fn dispatch_android_surface_size_request(
    app: &android_activity::AndroidApp,
    requested: Size,
    position: Point,
    density: f32,
    overlay_options: Option<AndroidOverlayWindowOptions>,
) -> Result<(), String> {
    let requested =
        android_host_window::validate_logical_size(requested).map_err(|error| error.to_string())?;
    if overlay_options.is_some() {
        return android_overlay_window::update_android_overlay_window_bounds(
            app, position, requested, density,
        );
    }

    let (width_px, height_px) =
        android_host_window::logical_to_physical_window_size(requested, density);
    set_android_window_layout_px(app, width_px, height_px)
}

fn dispatch_registered_android_surface_size_request(
    app: &android_activity::AndroidApp,
    host_window_registry: &android_host_window::AndroidHostWindowRegistry,
    density: f32,
    overlay_options: Option<AndroidOverlayWindowOptions>,
    last_dispatched: &mut Option<(android_host_window::AndroidHostWindowState, u64, u64)>,
    pending_confirmation: &mut Option<PendingHostWindowSizeRequest>,
) {
    let Some(request) =
        android_host_window::latest_android_host_window_request(host_window_registry)
    else {
        return;
    };
    let dispatch_key = (
        request.state,
        request.size_revision,
        if overlay_options.is_some() {
            request.position_revision
        } else {
            0
        },
    );
    if *last_dispatched == Some(dispatch_key) {
        return;
    }

    let position = overlay_options
        .filter(|_| request.position_revision == 0)
        .map(|options| Point::new(options.x as f32, options.y as f32))
        .unwrap_or(request.position);
    request.state.mark_pending(request.size);
    match dispatch_android_surface_size_request(
        app,
        request.size,
        position,
        density,
        overlay_options,
    ) {
        Ok(()) => {
            *last_dispatched = Some(dispatch_key);
            *pending_confirmation = Some(PendingHostWindowSizeRequest {
                state: Some(request.state),
                requested: request.size,
                requested_at: Instant::now(),
            });
            let target = if overlay_options.is_some() {
                "Android overlay surface"
            } else {
                "Android host-window"
            };
            if overlay_options.is_some() {
                log::info!(
                    "Requested {target} bounds {:.1}x{:.1} dp at {:.1},{:.1} dp",
                    request.size.width,
                    request.size.height,
                    position.x,
                    position.y
                );
            } else {
                log::info!(
                    "Requested {target} size {:.1}x{:.1} dp",
                    request.size.width,
                    request.size.height
                );
            }
        }
        Err(message) => {
            *last_dispatched = Some(dispatch_key);
            request.state.mark_dispatch_failed(request.size, message);
        }
    }
}

fn confirm_android_host_window_request(
    pending_confirmation: &mut Option<PendingHostWindowSizeRequest>,
    actual_size: Size,
) {
    let Some(pending) = *pending_confirmation else {
        return;
    };

    if android_host_window::sizes_match(pending.requested, actual_size) {
        if let Some(state) = pending.state {
            state.mark_applied(pending.requested, actual_size);
        }
        *pending_confirmation = None;
        return;
    }

    if pending.requested_at.elapsed() >= android_host_window::HOST_WINDOW_CONFIRMATION_TIMEOUT {
        if let Some(state) = pending.state {
            state.mark_unsupported(pending.requested, actual_size);
        }
        log::info!(
            "Android surface size request {:.1}x{:.1} dp was not honored; actual is {:.1}x{:.1} dp",
            pending.requested.width,
            pending.requested.height,
            actual_size.width,
            actual_size.height
        );
        *pending_confirmation = None;
    }
}

fn set_android_window_layout_px(
    app: &android_activity::AndroidApp,
    width_px: i32,
    height_px: i32,
) -> Result<(), String> {
    use jni::{jni_sig, jni_str, objects::JValue};

    with_android_activity_env(app, |env, activity| {
        let class = android_overlay_window::find_android_overlay_class(env, &activity)?;
        let result = env
            .call_static_method(
                class,
                jni_str!("setActivityWindowLayout"),
                jni_sig!("(Landroid/app/Activity;II)I"),
                &[
                    JValue::Object(&activity),
                    JValue::Int(width_px),
                    JValue::Int(height_px),
                ],
            )
            .and_then(|value| value.i())
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                format!("failed to request Android window layout: {error}")
            })?;

        match result {
            0 => Ok(()),
            code => Err(format!(
                "Android window layout request failed with code {code}"
            )),
        }
    })
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
    use android_activity::{MainEvent, PollEvent};

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

    let android_frame_driver = AndroidFrameDriver::new(app.create_waker());
    let host_window_registry = Rc::new(android_host_window::AndroidHostWindowRegistry::default());
    let overlay_event_queue = Arc::new(android_overlay_window::AndroidOverlayEventQueue::default());

    // Exit flag for Destroy event (can't break from inside poll_events closure)
    let should_exit = Arc::new(AtomicBool::new(false));

    // Initialize wgpu instance with GL and Vulkan backends
    // Use DISCARD_HAL_LABELS to prevent crash in emulator's Vulkan debug utils
    // (vk_common_SetDebugUtilsObjectNameEXT crashes on null labels)
    let backends = wgpu::Backends::GL | wgpu::Backends::VULKAN;

    let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    instance_descriptor.backends = backends;
    // No debug/validation: emulator Vulkan debug utils can crash on null labels.
    instance_descriptor.flags = wgpu::InstanceFlags::empty();
    let instance = wgpu::Instance::new(instance_descriptor);

    // Platform abstraction for density/pointer conversion
    let mut android_platform = AndroidPlatform::new();
    let mut current_host_window_size = Size::ZERO;
    let mut initial_host_window_size = settings.initial_size_explicit.then(|| {
        Size::new(
            settings.initial_width as f32,
            settings.initial_height as f32,
        )
    });
    let mut last_dispatched_host_window_request =
        None::<(android_host_window::AndroidHostWindowState, u64, u64)>;
    let mut pending_host_window_confirmation = None::<PendingHostWindowSizeRequest>;
    let mut overlay_window_options = settings.android_overlay_window;
    let mut overlay_window_requested = false;

    // GPU resources (recreated when window is destroyed/created)
    let mut gpu_resources: Option<GpuResources> = None;

    // Queue for input events (processed outside poll_events to prevent ANR)
    let mut pending_inputs: Vec<PendingInput> = Vec::new();

    // Main event loop
    loop {
        let pending_confirmation_timeout = pending_host_window_confirmation.map(|pending| {
            android_host_window::HOST_WINDOW_CONFIRMATION_TIMEOUT
                .checked_sub(pending.requested_at.elapsed())
                .unwrap_or(Duration::ZERO)
        });

        if let Some(shell) = app_shell.as_ref() {
            shell.schedule_platform_frame(&android_frame_driver);
        } else {
            android_frame_driver.clear_wake();
        }
        let frame_deadline_timeout = android_frame_driver.deadline_timeout();
        let idle_timeout =
            earliest_android_poll_timeout(pending_confirmation_timeout, frame_deadline_timeout);

        let poll_duration = if !pending_inputs.is_empty() {
            Some(Duration::ZERO)
        } else if android_frame_driver.frame_requested() {
            Some(Duration::ZERO)
        } else {
            idle_timeout
        };

        app.poll_events(poll_duration, |event| {
            match event {
                PollEvent::Main(main_event) => match main_event {
                    MainEvent::InitWindow { .. } => {
                        log::info!("Window initialized, setting up rendering");

                        if let Some(options) = overlay_window_options {
                            let density =
                                update_android_platform_geometry(&app, &mut android_platform);
                            android_platform.set_input_surface_offset_px(0.0, 0.0);
                            if !overlay_window_requested {
                                match android_overlay_window::show_android_overlay_window(
                                    &app,
                                    options,
                                    density,
                                    &overlay_event_queue,
                                ) {
                                    Ok(()) => {
                                        overlay_window_requested = true;
                                        log::info!(
                                            "Requested Android overlay surface {}x{} dp at ({}, {})",
                                            options.width,
                                            options.height,
                                            options.x,
                                            options.y
                                        );
                                    }
                                    Err(error) => {
                                        overlay_window_options = None;
                                        log::warn!(
                                            "Android overlay surface unavailable; waiting for activity surface fallback: {error}"
                                        );
                                    }
                                }
                            }
                        }

                        if overlay_window_options.is_none() {
                            if let Some(native_window) = app.native_window() {
                                let width = native_window.width() as u32;
                                let height = native_window.height() as u32;
                                let density =
                                    update_android_platform_geometry(&app, &mut android_platform);
                                let (input_offset_x, input_offset_y) =
                                    android_platform.input_surface_offset_px();
                                log::info!(
                                    "Display density: {:.2}x, input surface offset: ({:.1}, {:.1}) px",
                                    density,
                                    input_offset_x,
                                    input_offset_y
                                );

                                match initialize_android_rendering(
                                    &instance,
                                    gpu_resources.take(),
                                    &mut app_shell,
                                    &content,
                                    &settings,
                                    &android_frame_driver,
                                    &host_window_registry,
                                    native_window.ptr().cast(),
                                    None,
                                    width,
                                    height,
                                    density,
                                ) {
                                    Ok((resources, actual_size)) => {
                                        if let Some(actual_size) = actual_size {
                                            current_host_window_size = actual_size;
                                        }
                                        let width_dp = current_host_window_size.width;
                                        let height_dp = current_host_window_size.height;
                                        log::info!(
                                            "Set viewport to {:.1}x{:.1} dp ({}x{} px at {:.2}x density)",
                                            width_dp,
                                            height_dp,
                                            width,
                                            height,
                                            density
                                        );

                                        if let Some(requested) = initial_host_window_size.take() {
                                            match dispatch_android_surface_size_request(
                                                &app,
                                                requested,
                                                Point::ZERO,
                                                density,
                                                None,
                                            ) {
                                                Ok(()) => {
                                                    pending_host_window_confirmation =
                                                        Some(PendingHostWindowSizeRequest {
                                                            state: None,
                                                            requested,
                                                            requested_at: Instant::now(),
                                                        });
                                                    log::info!(
                                                        "Requested initial Android host-window size {:.1}x{:.1} dp",
                                                        requested.width,
                                                        requested.height
                                                    );
                                                }
                                                Err(error) => {
                                                    log::warn!(
                                                        "Initial Android host-window size request failed: {error}"
                                                    );
                                                }
                                            }
                                        }

                                        gpu_resources = Some(resources);
                                        log::info!("Rendering initialized successfully");
                                    }
                                    Err(error) => {
                                        log::error!("Android rendering initialization failed: {error}");
                                    }
                                }
                            }
                        }
                    }
                    MainEvent::TerminateWindow { .. } => {
                        log::info!("Window terminated");
                        if overlay_window_options.is_none() {
                            gpu_resources = None;
                        }
                    }
                    MainEvent::WindowResized { .. } => {
                        if overlay_window_options.is_none() {
                            if let Some(native_window) = app.native_window() {
                                let width = native_window.width() as u32;
                                let height = native_window.height() as u32;

                                let density =
                                    update_android_platform_geometry(&app, &mut android_platform);
                                let (input_offset_x, input_offset_y) =
                                    android_platform.input_surface_offset_px();
                                log::info!(
                                    "Window resized to {}x{} at {:.2}x density with input surface offset ({:.1}, {:.1}) px",
                                    width,
                                    height,
                                    density,
                                    input_offset_x,
                                    input_offset_y
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

                                        if let Some(actual_size) =
                                            update_android_shell_geometry(
                                                shell,
                                                density,
                                                &host_window_registry,
                                            )
                                        {
                                            current_host_window_size = actual_size;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    MainEvent::ContentRectChanged { .. } => {
                        let density = update_android_platform_geometry(&app, &mut android_platform);
                        if overlay_window_options.is_some() {
                            android_platform.set_input_surface_offset_px(0.0, 0.0);
                        }
                        let (input_offset_x, input_offset_y) =
                            android_platform.input_surface_offset_px();
                        log::info!(
                            "Content rect changed; input surface offset: ({:.1}, {:.1}) px at {:.2}x density",
                            input_offset_x,
                            input_offset_y,
                            density
                        );

                        if let Some(shell) = &mut app_shell {
                            if let Some(actual_size) =
                                update_android_shell_geometry(shell, density, &host_window_registry)
                            {
                                current_host_window_size = actual_size;
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
                        log::info!("Save state requested");
                    }
                    MainEvent::Destroy => {
                        log::info!("App destroy requested, will exit after this event");
                        if overlay_window_options.is_some() {
                            android_overlay_window::hide_android_overlay_window(&app);
                        }
                        should_exit.store(true, Ordering::Relaxed);
                    }
                    MainEvent::InputAvailable => {
                        drain_android_input_events(
                            &app,
                            &android_platform,
                            &mut pending_inputs,
                        );
                    }
                    _ => {}
                },
                _ => {
                    // Non-main poll events do not own Android NativeActivity
                    // input. Native input is delivered as MainEvent::InputAvailable.
                }
            }
        });

        for event in
            android_overlay_window::drain_android_overlay_window_events(&overlay_event_queue)
        {
            match event {
                android_overlay_window::AndroidOverlayWindowEvent::CreateFailed(message) => {
                    log::warn!("Android overlay surface failed: {message}");
                    overlay_window_options = None;

                    if let Some(native_window) = app.native_window() {
                        let width = native_window.width() as u32;
                        let height = native_window.height() as u32;
                        if width > 0 && height > 0 {
                            let density =
                                update_android_platform_geometry(&app, &mut android_platform);
                            match initialize_android_rendering(
                                &instance,
                                gpu_resources.take(),
                                &mut app_shell,
                                &content,
                                &settings,
                                &android_frame_driver,
                                &host_window_registry,
                                native_window.ptr().cast(),
                                None,
                                width,
                                height,
                                density,
                            ) {
                                Ok((resources, actual_size)) => {
                                    if let Some(actual_size) = actual_size {
                                        current_host_window_size = actual_size;
                                    }
                                    gpu_resources = Some(resources);
                                }
                                Err(error) => {
                                    log::error!(
                                        "Android activity surface fallback initialization failed: {error}"
                                    );
                                }
                            }
                        }
                    }
                }
                android_overlay_window::AndroidOverlayWindowEvent::SurfaceChanged {
                    native_window,
                    width,
                    height,
                } => {
                    if width > 0 && height > 0 {
                        let density = get_display_density(&app);
                        android_platform.set_scale_factor(density as f64);
                        android_platform.set_input_surface_offset_px(0.0, 0.0);
                        if let Some(shell) = app_shell.as_mut() {
                            shell.set_density(density);
                        }

                        let native_window_ptr = native_window.ptr().cast();
                        match initialize_android_rendering(
                            &instance,
                            gpu_resources.take(),
                            &mut app_shell,
                            &content,
                            &settings,
                            &android_frame_driver,
                            &host_window_registry,
                            native_window_ptr,
                            Some(native_window),
                            width,
                            height,
                            density,
                        ) {
                            Ok((resources, actual_size)) => {
                                if let Some(actual_size) = actual_size {
                                    current_host_window_size = actual_size;
                                }
                                gpu_resources = Some(resources);
                                log::info!(
                                    "Android overlay surface ready at {}x{} px ({:.2}x density)",
                                    width,
                                    height,
                                    density
                                );
                            }
                            Err(error) => {
                                log::error!(
                                    "Android overlay surface initialization failed: {error}"
                                );
                            }
                        }
                    }
                }
                android_overlay_window::AndroidOverlayWindowEvent::SurfaceDestroyed => {
                    if overlay_window_options.is_some() {
                        gpu_resources = None;
                    }
                }
                android_overlay_window::AndroidOverlayWindowEvent::Pointer { action, x, y } => {
                    let logical = android_platform.pointer_position(x as f64, y as f64);
                    match action {
                        android_overlay_window::AndroidOverlayPointerAction::Down => {
                            pending_inputs.push(PendingInput::PointerDown(
                                logical.x as f32,
                                logical.y as f32,
                            ));
                        }
                        android_overlay_window::AndroidOverlayPointerAction::Up
                        | android_overlay_window::AndroidOverlayPointerAction::Cancel => {
                            pending_inputs
                                .push(PendingInput::PointerUp(logical.x as f32, logical.y as f32));
                        }
                        android_overlay_window::AndroidOverlayPointerAction::Move => {
                            pending_inputs.push(PendingInput::PointerMove(
                                logical.x as f32,
                                logical.y as f32,
                            ));
                        }
                    }
                }
            }
        }

        // Process pending input events outside poll_events to prevent ANR
        if !pending_inputs.is_empty() {
            if let Some(shell) = &mut app_shell {
                for input in pending_inputs.drain(..) {
                    match input {
                        PendingInput::PointerDown(x, y) => {
                            shell.set_cursor(x, y);
                            shell.pointer_pressed();
                        }
                        PendingInput::PointerUp(x, y) => {
                            shell.set_cursor(x, y);
                            shell.pointer_released();
                        }
                        PendingInput::PointerMove(x, y) => {
                            shell.set_cursor(x, y);
                        }
                    }
                }
            }
        }

        // Check if app side requested a frame (animations, state changes)
        if android_frame_driver.take_frame_request() {
            if let Some(shell) = &mut app_shell {
                shell.mark_dirty();
            }
        }

        confirm_android_host_window_request(
            &mut pending_host_window_confirmation,
            current_host_window_size,
        );

        // Check if Destroy event requested exit
        if should_exit.load(Ordering::Relaxed) {
            log::info!("Exiting cleanly after Destroy event");
            break;
        }

        // Render outside event callback if needed
        if let (Some(resources), Some(shell)) = (&mut gpu_resources, &mut app_shell) {
            if shell.needs_redraw() {
                android_host_window::with_android_host_window_registry(
                    &host_window_registry,
                    || {
                        shell.update();
                    },
                );
                dispatch_registered_android_surface_size_request(
                    &app,
                    &host_window_registry,
                    android_platform.scale_factor(),
                    overlay_window_options,
                    &mut last_dispatched_host_window_request,
                    &mut pending_host_window_confirmation,
                );
                if render_once(resources, shell) {
                    break; // Out of memory, exit
                }
            }
        }
    }
}
