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

#[cfg(feature = "robot")]
use std::sync::mpsc;
#[cfg(feature = "robot")]
use winit::event_loop::EventLoopProxy;

/// Robot commands for programmatic control (only available with "robot" feature)
#[cfg(feature = "robot")]
#[derive(Debug, Clone)]
pub enum RobotCommand {
    /// Click at coordinates (physical pixels)
    Click {
        /// X coordinate in physical pixels
        x: f32,
        /// Y coordinate in physical pixels
        y: f32
    },
    /// Move cursor to coordinates (physical pixels)
    MoveTo {
        /// X coordinate in physical pixels
        x: f32,
        /// Y coordinate in physical pixels
        y: f32
    },
    /// Drag from one position to another
    Drag {
        /// Start X coordinate in physical pixels
        from_x: f32,
        /// Start Y coordinate in physical pixels
        from_y: f32,
        /// End X coordinate in physical pixels
        to_x: f32,
        /// End Y coordinate in physical pixels
        to_y: f32
    },
    /// Take a screenshot and save to file
    Screenshot {
        /// File path to save screenshot
        path: String
    },
    /// Shutdown the application
    Shutdown,
}

/// Handle for controlling a robot-enabled app
#[cfg(feature = "robot")]
pub struct RobotAppHandle {
    /// Channel for sending commands to the app thread
    command_tx: mpsc::Sender<RobotCommand>,
    /// Event loop proxy for waking the app
    proxy: EventLoopProxy<RobotCommand>,
}

#[cfg(feature = "robot")]
impl RobotAppHandle {
    /// Send a click command
    pub fn click(&self, x: f32, y: f32) -> Result<(), String> {
        self.command_tx
            .send(RobotCommand::Click { x, y })
            .map_err(|e| format!("Failed to send click: {}", e))?;
        self.proxy
            .send_event(RobotCommand::Click { x, y })
            .map_err(|e| format!("Failed to wake event loop: {}", e))
    }

    /// Send a move command
    pub fn move_to(&self, x: f32, y: f32) -> Result<(), String> {
        self.command_tx
            .send(RobotCommand::MoveTo { x, y })
            .map_err(|e| format!("Failed to send move: {}", e))?;
        self.proxy
            .send_event(RobotCommand::MoveTo { x, y })
            .map_err(|e| format!("Failed to wake event loop: {}", e))
    }

    /// Send a drag command
    pub fn drag(&self, from_x: f32, from_y: f32, to_x: f32, to_y: f32) -> Result<(), String> {
        self.command_tx
            .send(RobotCommand::Drag { from_x, from_y, to_x, to_y })
            .map_err(|e| format!("Failed to send drag: {}", e))?;
        self.proxy
            .send_event(RobotCommand::Drag { from_x, from_y, to_x, to_y })
            .map_err(|e| format!("Failed to wake event loop: {}", e))
    }

    /// Send a screenshot command
    ///
    /// Note: Currently returns dummy dimensions (0, 0) as actual screenshot capture is not yet implemented
    pub fn screenshot(&self, path: &str) -> Result<(u32, u32), String> {
        self.command_tx
            .send(RobotCommand::Screenshot { path: path.to_string() })
            .map_err(|e| format!("Failed to send screenshot: {}", e))?;
        self.proxy
            .send_event(RobotCommand::Screenshot { path: path.to_string() })
            .map_err(|e| format!("Failed to wake event loop: {}", e))?;

        // TODO: Return actual dimensions when screenshot is implemented
        Ok((0, 0))
    }

    /// Wait for a number of frames to be rendered
    pub fn wait_frames(&self, _count: u32) -> Result<(), String> {
        // Simple sleep-based implementation for now
        std::thread::sleep(std::time::Duration::from_millis(16 * _count as u64));
        Ok(())
    }

    /// Resize the window
    pub fn resize(&self, _width: u32, _height: u32) -> Result<(), String> {
        // TODO: Implement window resize command
        log::warn!("Window resize not yet implemented");
        Ok(())
    }

    /// Send a shutdown command
    pub fn shutdown(&self) -> Result<(), String> {
        self.command_tx
            .send(RobotCommand::Shutdown)
            .map_err(|e| format!("Failed to send shutdown: {}", e))?;
        self.proxy
            .send_event(RobotCommand::Shutdown)
            .map_err(|e| format!("Failed to wake event loop: {}", e))
    }
}

// Macro to generate the desktop app event loop
// This ensures run() and run_with_robot() share the SAME implementation
macro_rules! desktop_event_loop {
    (
        settings: $settings:expr,
        content: $content:expr,
        event_loop_type: $event_type:ty,
        event_loop_builder: $event_loop_expr:expr,
        frame_wake_event: $wake_event:expr,
        user_event_handler: |$app:ident, $window:ident, $elwt:ident, $cmd:ident| $user_handler:block
    ) => {{
        let frame_proxy = $event_loop_expr.create_proxy();

        let window = Arc::new(
            WindowBuilder::new()
                .with_title($settings.window_title)
                .with_inner_size(LogicalSize::new(
                    $settings.initial_width as f64,
                    $settings.initial_height as f64,
                ))
                .build(&$event_loop_expr)
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

        // Create renderer with fonts from settings
        let mut renderer = if let Some(fonts) = $settings.fonts {
            WgpuRenderer::new_with_fonts(fonts)
        } else {
            WgpuRenderer::new()
        };
        renderer.init_gpu(Arc::new(device), Arc::new(queue), surface_format);
        let initial_scale = window.scale_factor();
        renderer.set_root_scale(initial_scale as f32);

        let mut $app = AppShell::new(renderer, default_root_key(), $content);
        let mut platform = DesktopWinitPlatform::default();
        platform.set_scale_factor(initial_scale);

        $app.set_frame_waker({
            let proxy = frame_proxy.clone();
            move || {
                let _ = proxy.send_event($wake_event);
            }
        });

        // Set buffer_size to physical pixels and viewport to logical dp
        $app.set_buffer_size(size.width, size.height);
        let logical_width = size.width as f32 / initial_scale as f32;
        let logical_height = size.height as f32 / initial_scale as f32;
        $app.set_viewport(logical_width, logical_height);

        let _ = $event_loop_expr.run(move |event, elwt| {
            elwt.set_control_flow(ControlFlow::Wait);
            match event {
                Event::UserEvent($cmd) => {
                    let $app = &mut $app;
                    let $window = &window;
                    let $elwt = elwt;
                    $user_handler
                },
                Event::WindowEvent { window_id, event } if window_id == window.id() => match event {
                    WindowEvent::CloseRequested => {
                        elwt.exit();
                    }
                    WindowEvent::Resized(new_size) => {
                        if new_size.width > 0 && new_size.height > 0 {
                            surface_config.width = new_size.width;
                            surface_config.height = new_size.height;
                            let device = $app.renderer().device();
                            surface.configure(device, &surface_config);

                            let scale_factor = window.scale_factor();
                            let logical_width = new_size.width as f32 / scale_factor as f32;
                            let logical_height = new_size.height as f32 / scale_factor as f32;

                            $app.set_buffer_size(new_size.width, new_size.height);
                            $app.set_viewport(logical_width, logical_height);
                        }
                    }
                    WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                        platform.set_scale_factor(scale_factor);
                        $app.renderer().set_root_scale(scale_factor as f32);

                        let new_size = window.inner_size();
                        if new_size.width > 0 && new_size.height > 0 {
                            surface_config.width = new_size.width;
                            surface_config.height = new_size.height;
                            let device = $app.renderer().device();
                            surface.configure(device, &surface_config);

                            let logical_width = new_size.width as f32 / scale_factor as f32;
                            let logical_height = new_size.height as f32 / scale_factor as f32;

                            $app.set_buffer_size(new_size.width, new_size.height);
                            $app.set_viewport(logical_width, logical_height);
                        }
                    }
                    WindowEvent::CursorMoved { position, .. } => {
                        let logical = platform.pointer_position(position);
                        $app.set_cursor(logical.x, logical.y);
                    }
                    WindowEvent::MouseInput {
                        state,
                        button: MouseButton::Left,
                        ..
                    } => match state {
                        ElementState::Pressed => {
                            $app.pointer_pressed();
                        }
                        ElementState::Released => {
                            $app.pointer_released();
                        }
                    },
                    WindowEvent::KeyboardInput { event, .. } => {
                        use winit::keyboard::{KeyCode, PhysicalKey};
                        if event.state == ElementState::Pressed {
                            if let PhysicalKey::Code(KeyCode::KeyD) = event.physical_key {
                                $app.log_debug_info();
                            }
                        }
                    }
                    WindowEvent::RedrawRequested => {
                        $app.update();

                        let output = match surface.get_current_texture() {
                            Ok(output) => output,
                            Err(wgpu::SurfaceError::Lost) | Err(wgpu::SurfaceError::Outdated) => {
                                // Reconfigure surface with current window size
                                let size = window.inner_size();
                                if size.width > 0 && size.height > 0 {
                                    surface_config.width = size.width;
                                    surface_config.height = size.height;
                                    let device = $app.renderer().device();
                                    surface.configure(device, &surface_config);
                                }
                                return;
                            }
                            Err(wgpu::SurfaceError::OutOfMemory) => {
                                log::error!("Out of memory, exiting");
                                elwt.exit();
                                return;
                            }
                            Err(wgpu::SurfaceError::Timeout) => {
                                log::debug!("Surface timeout, skipping frame");
                                return;
                            }
                        };

                        let view = output
                            .texture
                            .create_view(&wgpu::TextureViewDescriptor::default());

                        if let Err(err) =
                            $app.renderer()
                                .render(&view, surface_config.width, surface_config.height)
                        {
                            log::error!("render failed: {err:?}");
                            return;
                        }

                        output.present();
                    }
                    _ => {}
                },
                Event::AboutToWait => {
                    if $app.needs_redraw() {
                        window.request_redraw();
                    }
                    // Use Poll for animations, Wait for idle
                    if $app.has_active_animations() {
                        elwt.set_control_flow(ControlFlow::Poll);
                    } else {
                        elwt.set_control_flow(ControlFlow::Wait);
                    }
                }
                _ => {}
            }
        });
    }};
}

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

    desktop_event_loop! {
        settings: settings,
        content: content,
        event_loop_type: (),
        event_loop_builder: event_loop,
        frame_wake_event: (),
        user_event_handler: |_app, _window, _elwt, _cmd| {
            // No user event handling in normal mode
        }
    }

    std::process::exit(0)
}

/// Runs a desktop Compose application with robot control for testing.
///
/// This function runs THE SAME desktop app as `run()`, but adds robot command
/// support for automated testing. The app runs in a background thread and
/// returns a `RobotAppHandle` for programmatic control.
///
/// **Note:** Only available with the "robot" feature enabled.
#[cfg(feature = "robot")]
pub fn run_with_robot<F>(settings: AppSettings, content: F) -> RobotAppHandle
where
    F: FnMut() + 'static + Send,
{
    let (command_tx, _command_rx) = mpsc::channel::<RobotCommand>();
    let (proxy_tx, proxy_rx) = mpsc::sync_channel::<EventLoopProxy<RobotCommand>>(1);

    // Spawn the app in a background thread
    std::thread::spawn(move || {
        let event_loop = winit::event_loop::EventLoopBuilder::<RobotCommand>::with_user_event()
            .build()
            .expect("failed to create event loop");

        // Send the proxy back to the caller
        let proxy = event_loop.create_proxy();
        proxy_tx.send(proxy.clone()).expect("Failed to send proxy");

        desktop_event_loop! {
            settings: settings,
            content: content,
            event_loop_type: RobotCommand,
            event_loop_builder: event_loop,
            frame_wake_event: RobotCommand::Shutdown,
            user_event_handler: |app, window, elwt, cmd| {
                match cmd {
                    RobotCommand::Click { x, y } => {
                        let scale_factor = window.scale_factor();
                        let logical_x = x / scale_factor as f32;
                        let logical_y = y / scale_factor as f32;

                        app.set_cursor(logical_x, logical_y);
                        app.pointer_pressed();
                        app.pointer_released();
                        window.request_redraw();
                    }
                    RobotCommand::MoveTo { x, y } => {
                        let scale_factor = window.scale_factor();
                        let logical_x = x / scale_factor as f32;
                        let logical_y = y / scale_factor as f32;

                        app.set_cursor(logical_x, logical_y);
                        window.request_redraw();
                    }
                    RobotCommand::Drag { from_x, from_y, to_x, to_y } => {
                        let scale_factor = window.scale_factor();
                        let logical_from_x = from_x / scale_factor as f32;
                        let logical_from_y = from_y / scale_factor as f32;
                        let logical_to_x = to_x / scale_factor as f32;
                        let logical_to_y = to_y / scale_factor as f32;

                        app.set_cursor(logical_from_x, logical_from_y);
                        app.pointer_pressed();
                        app.set_cursor(logical_to_x, logical_to_y);
                        app.pointer_released();
                        window.request_redraw();
                    }
                    RobotCommand::Screenshot { path } => {
                        log::warn!("Screenshot to {} not yet implemented", path);
                    }
                    RobotCommand::Shutdown => {
                        elwt.exit();
                    }
                }
            }
        }
    });

    // Wait for the app to start and send us the proxy
    let proxy = proxy_rx.recv().expect("Failed to receive proxy from app thread");

    RobotAppHandle {
        command_tx,
        proxy,
    }
}
