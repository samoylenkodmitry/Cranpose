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

use std::{
    cell::RefCell,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use cranpose_app_shell::{AppShell, default_root_key};
use cranpose_platform_desktop_winit::DesktopWinitPlatform;
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::EdgeInsets;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    window::{Window, WindowAttributes, WindowId},
};

use crate::{
    app_launcher::{AppSettings, LaunchError},
    wgpu_surface::{
        SurfaceFrame, current_surface_texture, present_initial_placeholder_frame,
        surface_present_required,
    },
    winit_pointer::{
        is_primary_pointer_button, pointer_source_from_button, pointer_source_from_winit,
    },
    winit_touch::{TouchLeave, TouchPointerRouter, TouchRoute},
};

struct GpuResources {
    surface: wgpu::Surface<'static>,
    device: Arc<wgpu::Device>,
    config: wgpu::SurfaceConfiguration,
    surface_dirty: bool,
}

struct IosApp<F: FnMut() + 'static> {
    settings: AppSettings,
    content: Option<Rc<RefCell<F>>>,
    window: Option<Arc<dyn Window>>,
    gpu: Option<GpuResources>,
    shell: Option<AppShell<WgpuRenderer>>,
    accessibility: Option<crate::ios_accessibility::IosAccessibilityBridge>,
    platform: DesktopWinitPlatform,
    platform_env: Rc<crate::platform_env::PlatformEnvironment>,
    last_keyboard_bottom: f32,
    event_proxy: EventLoopProxy,
    launch_error: Rc<RefCell<Option<LaunchError>>>,
    touch_router: TouchPointerRouter,
    next_off_screen_render: Option<Instant>,
}

const OFF_SCREEN_RENDER_PERIOD: Duration = Duration::from_millis(16);

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
            accessibility: None,
            platform: DesktopWinitPlatform::default(),
            platform_env: crate::platform_env::PlatformEnvironment::new(),
            last_keyboard_bottom: 0.0,
            event_proxy,
            launch_error,
            touch_router: TouchPointerRouter::default(),
            next_off_screen_render: None,
        }
    }

    fn pump_off_screen(&mut self, event_loop: &dyn ActiveEventLoop) {
        if !cranpose_services::background_active() || !crate::ios_background::app_is_off_screen() {
            if self.next_off_screen_render.take().is_some() {
                event_loop.set_control_flow(ControlFlow::Wait);
            }
            return;
        }
        let pending_ui = self
            .shell
            .as_ref()
            .is_some_and(|shell| shell.has_pending_ui());
        let due = self
            .next_off_screen_render
            .is_some_and(|at| at <= Instant::now());
        if !pending_ui && !due {
            return;
        }
        self.render();
        self.next_off_screen_render = match pending_ui {
            true => Some(Instant::now() + OFF_SCREEN_RENDER_PERIOD),
            false => None,
        };
        match self.next_off_screen_render {
            Some(at) => event_loop.set_control_flow(ControlFlow::WaitUntil(at)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }

    fn abort(&self, event_loop: &dyn ActiveEventLoop, error: LaunchError) {
        *self.launch_error.borrow_mut() = Some(error);
        event_loop.exit();
    }

    fn refresh_environment(&mut self, window: &Arc<dyn Window>) {
        let mut changed = self.platform_env.set_safe_area(safe_area_insets(window));
        if let Some(theme) = window.theme() {
            changed |= self.platform_env.set_system_theme(match theme {
                winit::window::Theme::Dark => cranpose_services::SystemTheme::Dark,
                winit::window::Theme::Light => cranpose_services::SystemTheme::Light,
            });
        }
        if changed && let Some(shell) = self.shell.as_mut() {
            shell.request_root_render();
        }
    }

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
            cranpose_services::publish_host_surface_size(
                cranpose_services::host_surface::HostSurfaceSize {
                    width: width as f32 / density,
                    height: height as f32 / density,
                    scale: density,
                },
            );
            shell.mark_dirty();
        }
    }

    fn render(&mut self) {
        let mut ime_changed = false;
        if crate::ios_keyboard::keyboard_poll_active() {
            if let Some(bottom) = crate::ios_keyboard::poll_keyboard_bottom_inset()
                && (self.last_keyboard_bottom - bottom).abs() > 0.5
            {
                self.last_keyboard_bottom = bottom;
                self.platform_env.set_ime_insets(EdgeInsets {
                    bottom,
                    ..EdgeInsets::default()
                });
                ime_changed = true;
            }
            self.event_proxy.wake_up();
        }

        if let (Some(accessibility), Some(shell)) =
            (self.accessibility.as_mut(), self.shell.as_mut())
        {
            accessibility.drain_activations(shell);
        }

        let (Some(gpu), Some(shell)) = (self.gpu.as_mut(), self.shell.as_mut()) else {
            return;
        };
        if ime_changed {
            shell.request_root_render();
        }

        for op in crate::ios_keyboard::take_ime_ops() {
            match op {
                crate::ios_keyboard::ImeOp::Replace(start, end, text) => {
                    shell.on_ime_set_selection(start, end);
                    shell.on_paste(&text);
                }
                crate::ios_keyboard::ImeOp::SetSelection(start, end) => {
                    shell.on_ime_set_selection(start, end);
                }
            }
        }
        if let Some(state) = shell.ime_editor_state() {
            crate::ios_keyboard::set_mirror(state.text, state.selection_start, state.selection_end);
        }
        if let Some(geom) = shell.ime_caret_geometry() {
            crate::ios_keyboard::set_caret_geometry(geom.caret_xs, geom.top, geom.line_height);
        }

        let dirty_before = gpu.surface_dirty;
        let update_result = shell.update();
        if let Some(accessibility) = self.accessibility.as_mut() {
            accessibility.sync(shell);
        }
        if crate::ios_background::app_is_off_screen() {
            gpu.surface_dirty = true;
            return;
        }
        if !surface_present_required(
            dirty_before,
            update_result.visual_changed,
            shell.needs_redraw(),
        ) {
            return;
        }

        match current_surface_texture(&gpu.surface, "ios") {
            SurfaceFrame::Ready(frame) => {
                let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
                    format: Some(crate::surface_format::display_surface_view_format(
                        gpu.config.format,
                    )),
                    ..Default::default()
                });
                let (width, height) = shell.buffer_size();
                if let Err(error) = shell
                    .renderer()
                    .render_surface_texture(&view, width, height)
                {
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
    fn resumed(&mut self, _event_loop: &dyn ActiveEventLoop) {
        if matches!(
            cranpose_services::current_lifecycle_state(),
            cranpose_services::LifecycleState::Created | cranpose_services::LifecycleState::Stopped
        ) {
            cranpose_services::dispatch_lifecycle_state(cranpose_services::LifecycleState::Started);
        }
        cranpose_services::dispatch_lifecycle_state(cranpose_services::LifecycleState::Resumed);
    }

    fn suspended(&mut self, _event_loop: &dyn ActiveEventLoop) {
        cranpose_services::dispatch_lifecycle_state(cranpose_services::LifecycleState::Paused);
        cranpose_services::dispatch_lifecycle_state(cranpose_services::LifecycleState::Stopped);
    }

    fn proxy_wake_up(&mut self, _event_loop: &dyn ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        self.pump_off_screen(_event_loop);
    }

    fn new_events(&mut self, event_loop: &dyn ActiveEventLoop, cause: winit::event::StartCause) {
        if matches!(cause, winit::event::StartCause::ResumeTimeReached { .. }) {
            self.pump_off_screen(event_loop);
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

        let (_instance, surface, adapter) =
            match crate::wgpu_surface::create_wgpu_surface_and_adapter(&window) {
                Ok(triple) => triple,
                Err(error) => {
                    self.abort(event_loop, error);
                    return;
                }
            };
        let backend = adapter.get_info().backend;

        let (device, queue) =
            match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("Cranpose iOS Device"),
                required_features: cranpose_render_wgpu::optional_device_features(&adapter),
                required_limits: crate::gpu_limits::mobile_device_limits(adapter.limits()),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: crate::gpu_limits::mobile_memory_hints(),
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
        present_initial_placeholder_frame(
            &surface,
            &device,
            &queue,
            config.format,
            "ios initial present",
        );

        let scale_factor = window.scale_factor();
        let density = (scale_factor as f32).max(f32::EPSILON);
        self.platform.set_scale_factor(scale_factor);
        self.refresh_environment(&window);

        let fonts = self.settings.resolve_font_set();
        let mut renderer = WgpuRenderer::with_font_set(fonts);
        renderer.init_gpu(
            Arc::clone(&device),
            Arc::clone(&queue),
            crate::surface_format::display_surface_view_format(config.format),
            backend,
            adapter.get_downlevel_capabilities().flags,
        );
        renderer.set_root_scale(density);

        let Some(content) = self.content.take() else {
            self.abort(event_loop, LaunchError::ContentUnavailable);
            return;
        };
        let content_for_shell = Rc::clone(&content);
        let env_for_shell = Rc::clone(&self.platform_env);
        let mut shell = AppShell::new_with_size_and_density(
            renderer,
            default_root_key(),
            move || {
                env_for_shell.compose_root(|| content_for_shell.borrow_mut()());
            },
            (width, height),
            (width as f32 / density, height as f32 / density),
            density,
        );

        shell.app_context().enter(crate::ios_clipboard::register);
        shell.app_context().enter(crate::ios_keyboard::register);
        shell.app_context().enter(crate::ios_back_gesture::register);
        shell.set_semantics_enabled(true);

        let mut accessibility =
            crate::ios_accessibility::IosAccessibilityBridge::new(self.event_proxy.clone());
        if let Some(accessibility) = accessibility.as_mut() {
            accessibility.sync(&mut shell);
        }

        let waker_proxy = self.event_proxy.clone();
        shell.set_frame_waker(move || waker_proxy.wake_up());

        let keyboard_proxy = self.event_proxy.clone();
        crate::ios_keyboard::set_wake(Box::new(move || keyboard_proxy.wake_up()));

        self.gpu = Some(GpuResources {
            surface,
            device,
            config,
            surface_dirty: true,
        });
        self.shell = Some(shell);
        self.accessibility = accessibility;
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
                self.refresh_environment(&window);
                window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = window.surface_size();
                self.reconfigure(size.width, size.height, window.scale_factor());
                self.refresh_environment(&window);
                window.request_redraw();
            }
            WindowEvent::PointerMoved {
                position, source, ..
            } => {
                let logical = self.platform.pointer_position(position);
                let route = self.touch_router.moved(&source);
                if let Some(shell) = self.shell.as_mut() {
                    shell.set_pointer_source(pointer_source_from_winit(&source));
                    let changed = match route {
                        TouchRoute::Primary => shell.set_cursor(logical.x, logical.y),
                        TouchRoute::Secondary(id) => {
                            shell.secondary_pointer_moved(id, logical.x, logical.y, None)
                        }
                        TouchRoute::Untracked => false,
                    };
                    if changed {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::PointerButton {
                state,
                position,
                button,
                ..
            } if is_primary_pointer_button(&button) => {
                let logical = self.platform.pointer_position(position);
                match state {
                    ElementState::Pressed => {
                        let route = self.touch_router.press(&button);
                        if let Some(shell) = self.shell.as_mut() {
                            shell.set_pointer_source(pointer_source_from_button(&button));
                            let changed = match route {
                                TouchRoute::Primary => {
                                    let event_time = shell.realtime_pointer_event_time(None);
                                    shell
                                        .set_cursor_at_event_time(logical.x, logical.y, event_time);
                                    shell.pointer_pressed_at_event_time(event_time)
                                }
                                TouchRoute::Secondary(id) => {
                                    shell.secondary_pointer_pressed(id, logical.x, logical.y, None)
                                }
                                TouchRoute::Untracked => false,
                            };
                            if changed {
                                window.request_redraw();
                            }
                        }
                    }
                    ElementState::Released => {
                        let release = self.touch_router.release(&button);
                        if let Some(shell) = self.shell.as_mut() {
                            shell.set_pointer_source(pointer_source_from_button(&button));
                            let mut changed = false;
                            for id in release.secondaries {
                                changed |= shell
                                    .secondary_pointer_released(id, logical.x, logical.y, None);
                            }
                            if release.releases_primary {
                                let event_time = shell.realtime_pointer_event_time(None);
                                changed |= shell.pointer_released_at_position_event_time(
                                    logical.x, logical.y, event_time,
                                );
                            }
                            if changed {
                                window.request_redraw();
                            }
                        }
                    }
                }
            }
            WindowEvent::PointerLeft { position, kind, .. } => {
                let leave = self.touch_router.left(&kind);
                let logical = position.map(|position| self.platform.pointer_position(position));
                if let Some(shell) = self.shell.as_mut() {
                    match leave {
                        TouchLeave::CancelGesture => {
                            shell.cancel_gesture();
                            window.request_redraw();
                        }
                        TouchLeave::ReleaseSecondary(id) => {
                            shell.set_pointer_source(cranpose_app_shell::PointerSource::Touch);
                            let (x, y) = logical.map_or((0.0, 0.0), |point| (point.x, point.y));
                            if shell.secondary_pointer_released(id, x, y, None) {
                                window.request_redraw();
                            }
                        }
                        TouchLeave::Ignore => {}
                    }
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

impl<F: FnMut() + 'static> Drop for IosApp<F> {
    fn drop(&mut self) {
        cranpose_services::dispatch_lifecycle_state(cranpose_services::LifecycleState::Destroyed);
    }
}

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

fn ios_surface_config(
    surface: &wgpu::Surface<'static>,
    adapter: &wgpu::Adapter,
    width: u32,
    height: u32,
) -> Result<wgpu::SurfaceConfiguration, LaunchError> {
    let caps = surface.get_capabilities(adapter);
    let format = crate::surface_format::select_display_surface_format(&caps.formats)
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
        view_formats: crate::surface_format::display_surface_view_formats(format),
        desired_maximum_frame_latency: 2,
    })
}

/// Runs a Cranpose application on iOS, blocking for the lifetime of the app.
///
/// Called by [`crate::AppLauncher::try_run`] on iOS.
pub fn try_run(settings: AppSettings, content: impl FnMut() + 'static) -> Result<(), LaunchError> {
    crate::ios_file_picker::register();
    crate::ios_uri_handler::register();
    crate::ios_share_sheet::register();
    crate::ios_image_picker::register();
    crate::ios_notifier::register();
    crate::ios_haptics::register();
    crate::ios_app_info::register();
    crate::ios_device_info::register();
    crate::process_info::install();
    crate::ios_background::register();
    crate::ios_writable_folder::register();
    crate::apple_camera::register();
    crate::ios_media::register();
    crate::ios_host::register();
    crate::ios_bundled_assets::register();
    cranpose_services::set_platform_app_updater(Arc::new(
        cranpose_services::GitHubAppUpdater::new(),
    ));
    #[cfg(feature = "storekit")]
    cranpose_storekit::register();

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
