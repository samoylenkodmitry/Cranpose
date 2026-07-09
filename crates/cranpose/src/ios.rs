//! iOS runtime for Cranpose applications.
//!
//! This backend is built on winit's UIKit integration (`winit-uikit`), which
//! provides a real `UIWindow`/`UIView` backed by a `CAMetalLayer`, CADisplayLink
//! driven redraw scheduling, touch input, density (scale factor) and the
//! application lifecycle. It is a native iOS surface, not a reuse of the desktop
//! window backend: the event loop here only manages a single fullscreen surface
//! and maps UIKit touches to Cranpose pointer input.
//!
//! winit starts `UIApplicationMain` from [`EventLoop::run_app`], so the iOS app
//! binary needs no Objective-C entry point.

use crate::launcher::{AppSettings, LaunchError};
use crate::wgpu_surface::{current_surface_texture, surface_present_required, SurfaceFrame};
use cranpose_app_shell::{default_root_key, AppShell, PointerSource};
use cranpose_core::CompositionLocalProvider;
use cranpose_platform_desktop_winit::DesktopWinitPlatform;
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::{local_safe_area_insets, EdgeInsets};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ButtonSource, ElementState, PointerSource as WinitPointerSource, WindowEvent};

/// Maps a winit pointer-move source onto the framework [`PointerSource`].
fn pointer_source_from_winit(source: &WinitPointerSource) -> PointerSource {
    match source {
        WinitPointerSource::Mouse => PointerSource::Mouse,
        WinitPointerSource::Touch { .. } => PointerSource::Touch,
        WinitPointerSource::TabletTool { .. } => PointerSource::Stylus,
        _ => PointerSource::Unknown,
    }
}

/// Maps a winit button source onto the framework [`PointerSource`].
fn pointer_source_from_button(button: &ButtonSource) -> PointerSource {
    match button {
        ButtonSource::Mouse(_) => PointerSource::Mouse,
        ButtonSource::Touch { .. } => PointerSource::Touch,
        ButtonSource::TabletTool { .. } => PointerSource::Stylus,
        _ => PointerSource::Unknown,
    }
}
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowAttributes, WindowId};

/// GPU resources tied to the live surface.
struct GpuResources {
    surface: wgpu::Surface<'static>,
    device: Arc<wgpu::Device>,
    config: wgpu::SurfaceConfiguration,
    /// Forces the first present after (re)configuring the surface even when the
    /// composition reports no visual work yet.
    surface_dirty: bool,
}

/// winit application handler for the single fullscreen iOS surface.
struct IosApp<F: FnMut() + 'static> {
    settings: AppSettings,
    content: Option<Rc<RefCell<F>>>,
    window: Option<Arc<dyn Window>>,
    gpu: Option<GpuResources>,
    shell: Option<AppShell<WgpuRenderer>>,
    platform: DesktopWinitPlatform,
    /// System safe-area insets (logical pixels) published to composition through
    /// [`local_safe_area_insets`]. Shared with the content closure so rotation
    /// and notch changes flow to the UI without rebuilding the shell.
    safe_area: Rc<Cell<EdgeInsets>>,
    /// Wakes the event loop for runtime-driven frames (animations, async work).
    event_proxy: EventLoopProxy,
    launch_error: Rc<RefCell<Option<LaunchError>>>,
}

impl<F: FnMut() + 'static> IosApp<F> {
    fn new(
        settings: AppSettings,
        content: F,
        event_proxy: EventLoopProxy,
        launch_error: Rc<RefCell<Option<LaunchError>>>,
    ) -> Self {
        Self {
            settings,
            content: Some(Rc::new(RefCell::new(content))),
            window: None,
            gpu: None,
            shell: None,
            platform: DesktopWinitPlatform::default(),
            safe_area: Rc::new(Cell::new(EdgeInsets::default())),
            event_proxy,
            launch_error,
        }
    }

    /// Records a fatal launch error and stops the event loop.
    fn abort(&self, event_loop: &dyn ActiveEventLoop, error: LaunchError) {
        *self.launch_error.borrow_mut() = Some(error);
        event_loop.exit();
    }

    /// Refreshes the published safe-area insets from the live window.
    fn refresh_safe_area(&self, window: &Arc<dyn Window>) {
        self.safe_area.set(safe_area_insets(window));
    }

    /// Reconfigures the swapchain to the current surface size and density and
    /// keeps the composition viewport in sync.
    fn reconfigure(&mut self, width: u32, height: u32, scale_factor: f64) {
        let width = width.max(1);
        let height = height.max(1);
        if let (Some(gpu), Some(shell)) = (self.gpu.as_mut(), self.shell.as_mut()) {
            gpu.config.width = width;
            gpu.config.height = height;
            gpu.surface.configure(&gpu.device, &gpu.config);
            gpu.surface_dirty = true;

            let density = (scale_factor as f32).max(f32::EPSILON);
            self.platform.set_scale_factor(scale_factor);
            shell.renderer().set_root_scale(density);
            shell.set_density(density);
            shell.set_buffer_size(width, height);
            shell.set_viewport(width as f32 / density, height as f32 / density);
            shell.mark_dirty();
        }
    }

    /// Renders a single frame, presenting only when work is pending.
    fn render(&mut self) {
        let (Some(gpu), Some(shell)) = (self.gpu.as_mut(), self.shell.as_mut()) else {
            return;
        };

        // Drain soft-keyboard edits into the focused text field before updating.
        for input in crate::ios_keyboard::take_key_inputs() {
            match input {
                crate::ios_keyboard::KeyInput::Insert(text) => {
                    shell.on_paste(&text);
                }
                crate::ios_keyboard::KeyInput::Backspace => {
                    shell.on_key_event(&cranpose_ui::KeyEvent::key_down(
                        cranpose_ui::KeyCode::Backspace,
                        "",
                    ));
                }
            }
        }

        let dirty_before = gpu.surface_dirty;
        let update_result = shell.update();
        if !surface_present_required(
            dirty_before,
            update_result.visual_changed,
            shell.needs_redraw(),
        ) {
            return;
        }

        match current_surface_texture(&gpu.surface, "ios") {
            SurfaceFrame::Ready(frame) => {
                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let (width, height) = shell.buffer_size();
                if let Err(error) = shell.renderer().render(&view, width, height) {
                    log::error!("iOS render error: {error:?}");
                }
                frame.present();
                gpu.surface_dirty = false;
            }
            SurfaceFrame::Reconfigure => {
                gpu.surface.configure(&gpu.device, &gpu.config);
                gpu.surface_dirty = true;
                shell.mark_dirty();
            }
            SurfaceFrame::Skip => {
                gpu.surface_dirty = true;
            }
        }
    }
}

impl<F: FnMut() + 'static> ApplicationHandler for IosApp<F> {
    fn proxy_wake_up(&mut self, _event_loop: &dyn ActiveEventLoop) {
        // A runtime frame was requested (see the frame waker). Schedule a redraw
        // outside the redraw handler so it is actually serviced.
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let window: Arc<dyn Window> = match event_loop.create_window(WindowAttributes::default()) {
            Ok(window) => window.into(),
            Err(error) => {
                self.abort(event_loop, LaunchError::WindowCreate(error));
                return;
            }
        };

        let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_descriptor.backends = wgpu::Backends::all();
        let instance = wgpu::Instance::new(instance_descriptor);

        let surface = match instance.create_surface(window.clone()) {
            Ok(surface) => surface,
            Err(error) => {
                self.abort(event_loop, LaunchError::SurfaceCreate(error));
                return;
            }
        };

        let adapter =
            match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })) {
                Ok(adapter) => adapter,
                Err(error) => {
                    self.abort(event_loop, LaunchError::NoAdapter(error));
                    return;
                }
            };
        let backend = adapter.get_info().backend;

        let (device, queue) =
            match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("Cranpose iOS Device"),
                required_features: wgpu::Features::empty(),
                required_limits: crate::gpu_limits::mobile_device_limits(adapter.limits()),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })) {
                Ok(pair) => pair,
                Err(error) => {
                    self.abort(event_loop, LaunchError::DeviceCreate(error));
                    return;
                }
            };

        let size = window.surface_size();
        let width = size.width.max(1);
        let height = size.height.max(1);
        let config = match ios_surface_config(&surface, &adapter, width, height) {
            Ok(config) => config,
            Err(error) => {
                self.abort(event_loop, error);
                return;
            }
        };

        let device = Arc::new(device);
        let queue = Arc::new(queue);
        surface.configure(&device, &config);

        let scale_factor = window.scale_factor();
        let density = (scale_factor as f32).max(f32::EPSILON);
        self.platform.set_scale_factor(scale_factor);
        self.refresh_safe_area(&window);

        let fonts: &[&[u8]] = self.settings.fonts.unwrap_or(&[]);
        let mut renderer = WgpuRenderer::new(fonts);
        renderer.init_gpu(
            Arc::clone(&device),
            Arc::clone(&queue),
            config.format,
            backend,
        );
        renderer.set_root_scale(density);

        let Some(content) = self.content.take() else {
            self.abort(event_loop, LaunchError::ContentUnavailable);
            return;
        };
        let content_for_shell = Rc::clone(&content);
        let safe_area_for_shell = Rc::clone(&self.safe_area);
        let mut shell = AppShell::new_with_size_and_density(
            renderer,
            default_root_key(),
            move || {
                let insets = safe_area_for_shell.get();
                CompositionLocalProvider(vec![local_safe_area_insets().provides(insets)], || {
                    content_for_shell.borrow_mut()()
                });
            },
            (width, height),
            (width as f32 / density, height as f32 / density),
            density,
        );

        // Route the selection menu's Copy/Cut/Paste through the iOS pasteboard.
        // The clipboard is per-AppContext, so register it inside the shell's
        // context (the global picker/URI handlers are registered before the
        // event loop instead).
        shell.app_context().enter(crate::ios_clipboard::register);
        shell.app_context().enter(crate::ios_keyboard::register);

        // Drive runtime-requested frames (animations, async results) through the
        // event-loop proxy. Calling `request_redraw` directly from the waker is
        // re-entrant with the in-progress redraw and winit-uikit coalesces it
        // away, so animations would only advance when another event (a touch or
        // scroll) happened to wake the loop. `wake_up` defers to `proxy_wake_up`,
        // which schedules the next redraw cleanly outside the redraw handler.
        let waker_proxy = self.event_proxy.clone();
        shell.set_frame_waker(move || waker_proxy.wake_up());

        // Wake the loop when a soft-keyboard keystroke is queued so it drains
        // (into the focused field) on the next frame, not the cursor-blink tick.
        let keyboard_proxy = self.event_proxy.clone();
        crate::ios_keyboard::set_wake(Box::new(move || keyboard_proxy.wake_up()));

        self.gpu = Some(GpuResources {
            surface,
            device,
            config,
            surface_dirty: true,
        });
        self.shell = Some(shell);
        self.window = Some(window.clone());
        window.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.clone() else {
            return;
        };
        if window_id != window.id() {
            return;
        }

        match event {
            WindowEvent::SurfaceResized(size) if size.width > 0 && size.height > 0 => {
                self.reconfigure(size.width, size.height, window.scale_factor());
                self.refresh_safe_area(&window);
                window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = window.surface_size();
                self.reconfigure(size.width, size.height, window.scale_factor());
                self.refresh_safe_area(&window);
                window.request_redraw();
            }
            WindowEvent::PointerMoved {
                position, source, ..
            } => {
                let logical = self.platform.pointer_position(position);
                if let Some(shell) = self.shell.as_mut() {
                    shell.set_pointer_source(pointer_source_from_winit(&source));
                    if shell.set_cursor(logical.x, logical.y) {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::PointerButton {
                state,
                position,
                button: button @ (ButtonSource::Mouse(_) | ButtonSource::Touch { .. }),
                ..
            } => {
                let logical = self.platform.pointer_position(position);
                if let Some(shell) = self.shell.as_mut() {
                    shell.set_pointer_source(pointer_source_from_button(&button));
                    let changed = match state {
                        ElementState::Pressed => {
                            shell.set_cursor(logical.x, logical.y);
                            shell.pointer_pressed()
                        }
                        // Touch lift-off positions must not become velocity
                        // samples (see AppShell::pointer_released_at_position).
                        ElementState::Released => {
                            shell.pointer_released_at_position(logical.x, logical.y)
                        }
                    };
                    if changed {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::PointerLeft { .. } => {
                if let Some(shell) = self.shell.as_mut() {
                    shell.cancel_gesture();
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                self.render();
            }
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            _ => {}
        }
    }
}

/// Reads the window safe-area insets and converts them to logical pixels.
fn safe_area_insets(window: &Arc<dyn Window>) -> EdgeInsets {
    let insets = window.safe_area();
    let scale = window.scale_factor().max(f64::EPSILON);
    EdgeInsets::from_components(
        (insets.left as f64 / scale) as f32,
        (insets.top as f64 / scale) as f32,
        (insets.right as f64 / scale) as f32,
        (insets.bottom as f64 / scale) as f32,
    )
}

/// Builds the swapchain configuration for the iOS Metal surface, preferring an
/// sRGB format and the platform's vsync-friendly present mode.
fn ios_surface_config(
    surface: &wgpu::Surface<'static>,
    adapter: &wgpu::Adapter,
    width: u32,
    height: u32,
) -> Result<wgpu::SurfaceConfiguration, LaunchError> {
    let caps = surface.get_capabilities(adapter);
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|format| format.is_srgb())
        .or_else(|| caps.formats.first().copied())
        .ok_or(LaunchError::NoSurfaceFormat)?;
    let alpha_mode = caps
        .alpha_modes
        .first()
        .copied()
        .ok_or(LaunchError::NoSurfaceAlphaMode)?;
    let present_mode = crate::present_mode::select_present_mode(&caps);
    Ok(wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width,
        height,
        present_mode,
        alpha_mode,
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    })
}

/// Runs a Cranpose application on iOS, blocking for the lifetime of the app.
///
/// Called by [`crate::AppLauncher::try_run`] on iOS.
pub fn try_run(settings: AppSettings, content: impl FnMut() + 'static) -> Result<(), LaunchError> {
    // Register the iOS document picker as the platform file picker, and the
    // UIApplication-based URI opener as the platform URI handler. Both run on
    // the UIKit main thread, before any request can be issued.
    crate::ios_file_picker::register();
    crate::ios_uri_handler::register();
    crate::ios_share_sheet::register();
    crate::ios_image_picker::register();
    crate::ios_notifier::register();
    crate::ios_haptics::register();
    crate::ios_device_info::register();
    crate::ios_background::register();
    crate::ios_writable_folder::register();
    crate::ios_camera::register();

    let event_loop = EventLoop::builder()
        .build()
        .map_err(LaunchError::EventLoopCreate)?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let event_proxy = event_loop.create_proxy();
    let launch_error = Rc::new(RefCell::new(None));
    let app = IosApp::new(settings, content, event_proxy, Rc::clone(&launch_error));
    let run_result = event_loop.run_app(app);

    if let Some(error) = launch_error.borrow_mut().take() {
        return Err(error);
    }
    run_result.map_err(LaunchError::EventLoopRun)
}
