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

/// Robot command for controlling the application
#[cfg(feature = "robot")]
#[derive(Debug)]
enum RobotCommand {
    Click { x: f32, y: f32 },
    MoveTo { x: f32, y: f32 },
    TouchDown { x: f32, y: f32 },
    TouchMove { x: f32, y: f32 },
    TouchUp { x: f32, y: f32 },
    WaitForIdle,
    Exit,
}

/// Robot response
#[cfg(feature = "robot")]
#[derive(Debug)]
enum RobotResponse {
    Ok,
    Error(String),
}

/// Robot controller for the event loop
#[cfg(feature = "robot")]
struct RobotController {
    rx: mpsc::Receiver<RobotCommand>,
    tx: mpsc::Sender<RobotResponse>,
}

#[cfg(feature = "robot")]
impl RobotController {
    fn new() -> (Self, Robot) {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (resp_tx, resp_rx) = mpsc::channel();

        let controller = RobotController {
            rx: cmd_rx,
            tx: resp_tx,
        };

        let robot = Robot {
            tx: cmd_tx,
            rx: resp_rx,
        };

        (controller, robot)
    }
}

/// Robot handle for test drivers
#[cfg(feature = "robot")]
pub struct Robot {
    tx: mpsc::Sender<RobotCommand>,
    rx: mpsc::Receiver<RobotResponse>,
}

#[cfg(feature = "robot")]
impl Robot {
    /// Click at the specified coordinates (logical pixels)
    pub fn click(&self, x: f32, y: f32) -> Result<(), String> {
        self.tx.send(RobotCommand::Click { x, y })
            .map_err(|e| format!("Failed to send click command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Ok) => Ok(()),
            Ok(RobotResponse::Error(e)) => Err(e),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Move cursor to the specified coordinates (logical pixels)
    pub fn move_to(&self, x: f32, y: f32) -> Result<(), String> {
        self.tx.send(RobotCommand::MoveTo { x, y })
            .map_err(|e| format!("Failed to send move command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Ok) => Ok(()),
            Ok(RobotResponse::Error(e)) => Err(e),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Wait for the application to be idle (no redraws, no animations)
    pub fn wait_for_idle(&self) -> Result<(), String> {
        self.tx.send(RobotCommand::WaitForIdle)
            .map_err(|e| format!("Failed to send wait command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Ok) => Ok(()),
            Ok(RobotResponse::Error(e)) => Err(e),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Exit the application
    pub fn exit(&self) -> Result<(), String> {
        self.tx.send(RobotCommand::Exit)
            .map_err(|e| format!("Failed to send exit command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Ok) => Ok(()),
            Ok(RobotResponse::Error(e)) => Err(e),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }
}

/// Runs a desktop Compose application with wgpu rendering.
///
/// Called by `AppLauncher::run_desktop()`. This is the framework-level
/// entrypoint that manages the desktop event loop and rendering.
///
/// **Note:** Applications should use `AppLauncher` instead of calling this directly.
pub fn run(settings: AppSettings, content: impl FnMut() + 'static) -> ! {
    let mut builder = EventLoopBuilder::new();

    // On Linux, allow creating event loop on any thread
    #[cfg(target_os = "linux")]
    {
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_any_thread(true);
    }

    let event_loop = builder
        .build()
        .expect("failed to create event loop");
    let frame_proxy = event_loop.create_proxy();

    // Spawn test driver if present
    #[cfg(feature = "robot")]
    let robot_controller = if let Some(driver) = settings.test_driver {
        let (controller, robot) = RobotController::new();
        std::thread::spawn(move || {
            driver(robot);
        });
        Some(controller)
    } else {
        None
    };

    #[cfg(not(feature = "robot"))]
    let robot_controller: Option<()> = None;

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

    // Create renderer with fonts from settings
    let mut renderer = if let Some(fonts) = settings.fonts {
        WgpuRenderer::new_with_fonts(fonts)
    } else {
        WgpuRenderer::new()
    };
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
                }
                WindowEvent::MouseInput {
                    state,
                    button: MouseButton::Left,
                    ..
                } => match state {
                    ElementState::Pressed => {
                        app.pointer_pressed();
                    }
                    ElementState::Released => {
                        app.pointer_released();
                    }
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
                        Err(wgpu::SurfaceError::Lost) | Err(wgpu::SurfaceError::Outdated) => {
                            // Reconfigure surface with current window size
                            let size = window.inner_size();
                            if size.width > 0 && size.height > 0 {
                                surface_config.width = size.width;
                                surface_config.height = size.height;
                                let device = app.renderer().device();
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
                // Handle pending robot commands
                #[cfg(feature = "robot")]
                if let Some(controller) = &robot_controller {
                    while let Ok(cmd) = controller.rx.try_recv() {
                        match cmd {
                            RobotCommand::Click { x, y } => {
                                app.set_cursor(x, y);
                                app.pointer_pressed();
                                app.pointer_released();
                                window.request_redraw();
                                let _ = controller.tx.send(RobotResponse::Ok);
                            }
                            RobotCommand::MoveTo { x, y } => {
                                app.set_cursor(x, y);
                                window.request_redraw();
                                let _ = controller.tx.send(RobotResponse::Ok);
                            }
                            RobotCommand::TouchDown { x, y } => {
                                app.set_cursor(x, y);
                                app.pointer_pressed();
                                let _ = controller.tx.send(RobotResponse::Ok);
                            }
                            RobotCommand::TouchMove { x, y } => {
                                app.set_cursor(x, y);
                                let _ = controller.tx.send(RobotResponse::Ok);
                            }
                            RobotCommand::TouchUp { x, y } => {
                                app.set_cursor(x, y);
                                app.pointer_released();
                                let _ = controller.tx.send(RobotResponse::Ok);
                            }
                            RobotCommand::WaitForIdle => {
                                // If idle, respond immediately
                                if !app.needs_redraw() && !app.has_active_animations() {
                                    let _ = controller.tx.send(RobotResponse::Ok);
                                } else {
                                    // Force update and respond
                                    app.update();
                                    let _ = controller.tx.send(RobotResponse::Ok);
                                }
                            }
                            RobotCommand::Exit => {
                                let _ = controller.tx.send(RobotResponse::Ok);
                                elwt.exit();
                            }
                        }
                    }
                }

                if app.needs_redraw() {
                    window.request_redraw();
                }
                // Use Poll for animations or if robot is active, Wait for idle otherwise
                if app.has_active_animations() || robot_controller.is_some() {
                    elwt.set_control_flow(ControlFlow::Poll);
                } else {
                    elwt.set_control_flow(ControlFlow::Wait);
                }
            }
            _ => {}
        }
    });

    std::process::exit(0)
}
