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
    dpi::PhysicalPosition,
    event::{ButtonSource, ElementState, WindowEvent},
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
        is_primary_pointer_button, other_mouse_button, pointer_source_from_button,
        pointer_source_from_winit,
    },
    winit_touch::{button_finger, kind_finger, source_finger},
};

struct GpuResources {
    surface: wgpu::Surface<'static>,
    /// Where a lost device's successor opens.
    adapter: wgpu::Adapter,
    device: Arc<wgpu::Device>,
    config: wgpu::SurfaceConfiguration,
    surface_dirty: bool,
}

struct IosApp<F: FnMut() + 'static> {
    #[cfg(feature = "webview")]
    webviews: crate::webview_host::WebViews<wry::WebView>,
    #[cfg(feature = "webview")]
    webview_wake: Rc<dyn Fn()>,
    settings: AppSettings,
    content: Option<Rc<RefCell<F>>>,
    window: Option<Arc<dyn Window>>,
    gpu: Option<GpuResources>,
    shell: Option<AppShell<WgpuRenderer>>,
    accessibility: Option<super::accessibility::IosAccessibilityBridge>,
    platform: DesktopWinitPlatform,
    platform_env: Rc<crate::platform_env::PlatformEnvironment>,
    last_keyboard_bottom: f32,
    event_proxy: EventLoopProxy,
    launch_error: Rc<RefCell<Option<LaunchError>>>,
    touch_router: cranpose_app_shell::TouchRouter<winit::event::FingerId>,
    next_off_screen_render: Option<Instant>,
    device_loss: crate::gpu_loss::LossSignal,
    device_loss_drill: crate::gpu_loss::LossDrill,
}

const OFF_SCREEN_RENDER_PERIOD: Duration = Duration::from_millis(16);

impl<F: FnMut() + 'static> IosApp<F> {
    /// Presses or releases a pointer button: the primary one of a finger, a
    /// pen or a mouse through the touch router, and a mouse's other buttons
    /// as the shell's other mouse buttons. Returns whether a node took it.
    fn pointer_button(
        &mut self,
        state: ElementState,
        position: PhysicalPosition<f64>,
        button: &ButtonSource,
    ) -> bool {
        let logical = self.platform.pointer_position(position);
        let Some(shell) = self.shell.as_mut() else {
            return false;
        };
        shell.set_pointer_source(pointer_source_from_button(button));
        if !is_primary_pointer_button(button) {
            let Some(button) = other_mouse_button(button) else {
                return false;
            };
            shell.set_cursor(logical.x, logical.y);
            let mut surface = shell.primary();
            return match state {
                ElementState::Pressed => surface.mouse_button_pressed(button),
                ElementState::Released => surface.mouse_button_released(button),
            };
        }
        let finger = button_finger(button);
        match state {
            ElementState::Pressed => self.touch_router.press(shell, finger, logical.x, logical.y),
            ElementState::Released => self
                .touch_router
                .release(shell, finger, logical.x, logical.y),
        }
    }

    fn new(
        settings: AppSettings,
        content: F,
        event_proxy: EventLoopProxy,
        launch_error: Rc<RefCell<Option<LaunchError>>>,
    ) -> Self {
        let device_loss = crate::gpu_loss::LossSignal::default();
        let drill_proxy = event_proxy.clone();
        let device_loss_drill =
            crate::gpu_loss::LossDrill::from_launch_args(&device_loss, move || {
                drill_proxy.wake_up();
            });
        Self {
            #[cfg(feature = "webview")]
            webviews: Default::default(),
            #[cfg(feature = "webview")]
            webview_wake: crate::wry_webview::wake_callback(&event_proxy),
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
            touch_router: cranpose_app_shell::TouchRouter::default(),
            next_off_screen_render: None,
            device_loss,
            device_loss_drill,
        }
    }

    fn pump_off_screen(&mut self, event_loop: &dyn ActiveEventLoop) {
        if !cranpose_services::background_active() || !super::background::app_is_off_screen() {
            self.next_off_screen_render = None;
        } else {
            let pending_ui = self.shell.as_ref().is_some_and(AppShell::has_pending_ui);
            let due = self
                .next_off_screen_render
                .is_some_and(|at| at <= Instant::now());
            if pending_ui || due {
                self.render();
                self.next_off_screen_render =
                    pending_ui.then(|| Instant::now() + OFF_SCREEN_RENDER_PERIOD);
            }
        }
        self.schedule_wake(event_loop);
    }

    fn schedule_wake(&self, event_loop: &dyn ActiveEventLoop) {
        let accessibility = self
            .accessibility
            .as_ref()
            .and_then(super::accessibility::IosAccessibilityBridge::wake_deadline);
        let wake = self
            .next_off_screen_render
            .into_iter()
            .chain(accessibility)
            .min();
        event_loop.set_control_flow(wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }

    fn publish_held_accessibility(&mut self) {
        let (Some(accessibility), Some(shell)) = (self.accessibility.as_mut(), self.shell.as_mut())
        else {
            return;
        };
        if accessibility
            .wake_deadline()
            .is_some_and(|deadline| deadline <= Instant::now())
        {
            accessibility.sync(
                shell,
                #[cfg(feature = "webview")]
                &self.webviews,
            );
        }
    }

    /// Opens a new device when the one in use was lost and hands it to the
    /// renderer, keeping the composition, its state and its input; ends the
    /// app when devices keep getting lost without a frame.
    fn recover_lost_device(&mut self, event_loop: &dyn ActiveEventLoop) {
        if !self.device_loss.take_loss() {
            return;
        }
        if !self.device_loss.begin_recovery() {
            self.abort(event_loop, LaunchError::GpuDeviceLost);
            return;
        }
        let Some(adapter) = self.gpu.as_ref().map(|gpu| &gpu.adapter) else {
            return;
        };
        let (device, queue) =
            match pollster::block_on(adapter.request_device(&ios_device_descriptor(adapter))) {
                Ok(pair) => pair,
                Err(error) => {
                    self.abort(event_loop, LaunchError::DeviceCreate(error));
                    return;
                }
            };
        let (Some(gpu), Some(shell)) = (self.gpu.as_mut(), self.shell.as_mut()) else {
            return;
        };
        let device = Arc::new(device);
        gpu.surface.configure(&device, &gpu.config);
        let loss_proxy = self.event_proxy.clone();
        let renderer = shell.renderer();
        renderer.set_device_lost_handler(self.device_loss.handler(move || loss_proxy.wake_up()));
        renderer.init_gpu(cranpose_render_wgpu::GpuTarget::on_adapter(
            &gpu.adapter,
            Arc::clone(&device),
            Arc::new(queue),
            crate::surface_format::display_surface_view_format(gpu.config.format),
        ));
        gpu.device = device;
        gpu.surface_dirty = true;
        shell.mark_dirty();
        log::warn!("[gpu-device] opened a new device after the last one was lost");
    }

    fn abort(&self, event_loop: &dyn ActiveEventLoop, error: LaunchError) {
        *self.launch_error.borrow_mut() = Some(error);
        event_loop.exit();
    }

    fn refresh_environment(&mut self, window: &Arc<dyn Window>) {
        let insets = safe_area_insets(window);
        let mut changed = self.platform_env.set_safe_area(insets);
        if let Some(shell) = self.shell.as_mut() {
            shell.set_safe_area(insets);
        }
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

    fn render(&mut self) -> bool {
        #[cfg(feature = "webview")]
        self.webviews.dispatch(&self.platform_env.native_views);
        let mut ime_changed = false;
        if super::keyboard::keyboard_poll_active() {
            if let Some(bottom) = super::keyboard::poll_keyboard_bottom_inset()
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
            accessibility.drain_value_steps(shell);
            accessibility.drain_scrolls(shell);
            accessibility.drain_dismissals(shell);
            accessibility.drain_jumps(shell);
            accessibility.drain_escapes(shell);
            accessibility.drain_custom_actions(shell);
            accessibility.drain_magic_taps(shell);
            accessibility.drain_focus(shell);
        }

        let (Some(gpu), Some(shell)) = (self.gpu.as_mut(), self.shell.as_mut()) else {
            return false;
        };
        if ime_changed {
            shell.request_root_render();
        }

        for op in super::keyboard::take_ime_ops() {
            match op {
                super::keyboard::ImeOp::Replace(start, end, text) => {
                    shell.on_ime_set_selection(start, end);
                    shell.on_paste(&text);
                }
                super::keyboard::ImeOp::SetSelection(start, end) => {
                    shell.on_ime_set_selection(start, end);
                }
                super::keyboard::ImeOp::Action => {
                    shell.on_ime_action(cranpose_ui::ImeAction::Default);
                }
            }
        }
        if let Some(state) = shell.ime_editor_state() {
            super::keyboard::set_mirror(state);
        }
        if let Some(geom) = shell.ime_caret_geometry() {
            super::keyboard::set_caret_geometry(geom);
        }

        let dirty_before = gpu.surface_dirty;
        let update_result = shell.update();
        #[cfg(feature = "webview")]
        crate::wry_webview::sync(
            &mut self.webviews,
            &self.platform_env.native_views,
            Some(shell),
            self.window.as_ref(),
            &self.webview_wake,
        );
        if self.accessibility.is_none() {
            self.accessibility =
                super::accessibility::IosAccessibilityBridge::new(self.event_proxy.clone());
        }
        if let Some(accessibility) = self.accessibility.as_mut() {
            accessibility.sync(
                shell,
                #[cfg(feature = "webview")]
                &self.webviews,
            );
        }
        if super::background::app_is_off_screen() {
            gpu.surface_dirty = true;
            return false;
        }
        if !surface_present_required(
            dirty_before,
            update_result.visual_changed,
            &mut shell.primary(),
        ) {
            return false;
        }

        match current_surface_texture(&gpu.surface, "ios") {
            SurfaceFrame::Ready(frame) => {
                let size = shell.buffer_size();
                if let Err(error) = crate::wgpu_surface::render_surface_frame(
                    shell.renderer(),
                    &frame,
                    gpu.config.format,
                    size,
                ) {
                    log::error!("iOS render error: {error:?}");
                }
                shell.renderer().present(frame);
                gpu.surface_dirty = false;
                self.device_loss.presented();
                self.device_loss_drill.presented();
                true
            }
            SurfaceFrame::Reconfigure => {
                gpu.surface.configure(&gpu.device, &gpu.config);
                gpu.surface_dirty = true;
                shell.mark_dirty();
                false
            }
            SurfaceFrame::Skip => {
                gpu.surface_dirty = true;
                false
            }
        }
    }

    fn request_owed_frame(&self) {
        if self
            .shell
            .as_ref()
            .is_some_and(|shell| shell.frame_schedule().needs_frame)
        {
            self.event_proxy.wake_up();
        }
    }
}

impl<F: FnMut() + 'static> ApplicationHandler for IosApp<F> {
    fn resumed(&mut self, _event_loop: &dyn ActiveEventLoop) {
        cranpose_services::advance_lifecycle(cranpose_services::LifecycleState::Resumed);
    }

    fn suspended(&mut self, _event_loop: &dyn ActiveEventLoop) {
        cranpose_services::advance_lifecycle(cranpose_services::LifecycleState::Stopped);
    }

    fn proxy_wake_up(&mut self, _event_loop: &dyn ActiveEventLoop) {
        self.recover_lost_device(_event_loop);
        if let Some(shell) = self.shell.as_mut() {
            shell.run_pending_tasks();
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        self.pump_off_screen(_event_loop);
    }

    fn new_events(&mut self, event_loop: &dyn ActiveEventLoop, cause: winit::event::StartCause) {
        if matches!(cause, winit::event::StartCause::ResumeTimeReached { .. }) {
            self.publish_held_accessibility();
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
        let (device, queue) =
            match pollster::block_on(adapter.request_device(&ios_device_descriptor(&adapter))) {
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
        let loss_proxy = self.event_proxy.clone();
        renderer.set_device_lost_handler(self.device_loss.handler(move || loss_proxy.wake_up()));
        renderer.init_gpu(cranpose_render_wgpu::GpuTarget::on_adapter(
            &adapter,
            Arc::clone(&device),
            Arc::clone(&queue),
            crate::surface_format::display_surface_view_format(config.format),
        ));

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

        shell.app_context().enter(super::clipboard::register);
        shell.app_context().enter(super::keyboard::register);
        shell.app_context().enter(super::back_gesture::register);
        shell.set_semantics_enabled(true);
        crate::accessibility::install_inspector(&mut shell, self.settings.developer_inspector);
        shell.set_safe_area(self.platform_env.safe_area());

        let mut accessibility =
            super::accessibility::IosAccessibilityBridge::new(self.event_proxy.clone());
        if let Some(accessibility) = accessibility.as_mut() {
            accessibility.sync(
                &mut shell,
                #[cfg(feature = "webview")]
                &self.webviews,
            );
        }

        let waker_proxy = self.event_proxy.clone();
        shell.set_frame_waker(move || waker_proxy.wake_up());

        let keyboard_proxy = self.event_proxy.clone();
        super::keyboard::set_wake(Box::new(move || keyboard_proxy.wake_up()));

        self.gpu = Some(GpuResources {
            surface,
            adapter,
            device,
            config,
            surface_dirty: true,
        });
        self.shell = Some(shell);
        self.accessibility = accessibility;
        self.window = Some(Arc::clone(&window));
        super::scene::hold_window(window.as_ref());
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
                if let Some(shell) = self.shell.as_mut() {
                    shell.set_pointer_source(pointer_source_from_winit(&source));
                    if self
                        .touch_router
                        .moved(shell, source_finger(&source), logical.x, logical.y)
                    {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::PointerButton {
                state,
                position,
                button,
                ..
            } => {
                if self.pointer_button(state, position, &button) {
                    window.request_redraw();
                }
            }
            WindowEvent::PointerLeft { position, kind, .. } => {
                let logical = position.map(|position| {
                    let point = self.platform.pointer_position(position);
                    (point.x, point.y)
                });
                if let Some(shell) = self.shell.as_mut()
                    && self.touch_router.cancel(shell, kind_finger(&kind), logical)
                {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if self.render() {
                    self.request_owed_frame();
                }
                self.schedule_wake(event_loop);
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
        cranpose_services::advance_lifecycle(cranpose_services::LifecycleState::Destroyed);
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

/// The device the iOS app opens on `adapter`, at launch and after a loss.
fn ios_device_descriptor(adapter: &wgpu::Adapter) -> wgpu::DeviceDescriptor<'static> {
    wgpu::DeviceDescriptor {
        label: Some("Cranpose iOS Device"),
        required_features: cranpose_render_wgpu::optional_device_features(adapter),
        required_limits: crate::gpu_limits::mobile_device_limits(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: crate::gpu_limits::mobile_memory_hints(),
        trace: wgpu::Trace::Off,
    }
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
    Ok(crate::wgpu_surface::root_surface_config(
        &caps,
        format,
        (width, height),
        present_mode,
        alpha_mode,
        2,
    ))
}

/// Runs a Cranpose application on iOS, blocking for the lifetime of the app.
///
/// Called by [`crate::AppLauncher::try_run`] on iOS.
pub fn try_run(settings: AppSettings, content: impl FnMut() + 'static) -> Result<(), LaunchError> {
    crate::apple::mobile::init_logging();
    super::file_picker::register();
    super::uri_handler::register();
    super::share_sheet::register();
    super::image_picker::register();
    super::notifier::register();
    super::haptics::register();
    crate::apple::microphone::register();
    #[cfg(feature = "wearable")]
    crate::apple::wearable::register();
    super::app_info::register();
    super::device_info::register();
    crate::apple::network::register();
    crate::process_info::install();
    super::background::register();
    super::downloads::register();
    super::storage::register();
    super::writable_folder::register();
    crate::apple::camera::register();
    super::media::register();
    super::host::register();
    super::bundled_assets::register();
    cranpose_services::set_platform_app_updater(Arc::new(
        cranpose_services::GitHubAppUpdater::new(),
    ));
    #[cfg(feature = "storekit")]
    cranpose_storekit::register();
    settings.start_process();

    super::scene::register_class();

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
