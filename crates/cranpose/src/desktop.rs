//! Desktop runtime for Compose applications.
//!
//! This module provides the desktop event loop implementation using winit.

use crate::launcher::AppSettings;
use cranpose_app_shell::{default_root_key, AppShell};
use cranpose_platform_desktop_winit::DesktopWinitPlatform;
use cranpose_render_wgpu::WgpuRenderer;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ButtonSource, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

#[cfg(feature = "robot")]
use cranpose_ui::{LayoutBox, SemanticsAction, SemanticsNode, SemanticsRole};

#[cfg(feature = "robot")]
use std::sync::mpsc;

/// Serializable semantic element combining semantics + geometry
///
/// This structure combines semantic information (role, text, actions) with
/// geometric bounds from the layout tree, enabling robot scripts to find
/// and interact with UI elements by their semantic properties.
#[cfg(feature = "robot")]
#[derive(Debug, Clone)]
pub struct SemanticElement {
    /// Semantic role (e.g., "Button", "Text", "Layout")
    pub role: String,
    /// Text content if available
    pub text: Option<String>,
    /// Geometric bounds in logical pixels
    pub bounds: SemanticRect,
    /// Whether this element has click actions
    pub clickable: bool,
    /// Child semantic elements
    pub children: Vec<SemanticElement>,
}

/// Geometric bounds for a semantic element
#[cfg(feature = "robot")]
#[derive(Debug, Clone)]
pub struct SemanticRect {
    /// X coordinate in logical pixels
    pub x: f32,
    /// Y coordinate in logical pixels
    pub y: f32,
    /// Width in logical pixels
    pub width: f32,
    /// Height in logical pixels
    pub height: f32,
}

/// Robot command for controlling the application
#[cfg(feature = "robot")]
#[derive(Debug)]
#[allow(dead_code)] // TouchDown, TouchMove, TouchUp reserved for future use
enum RobotCommand {
    Click {
        x: f32,
        y: f32,
    },
    MoveTo {
        x: f32,
        y: f32,
    },
    MouseDown,
    MouseUp,
    TouchDown {
        x: f32,
        y: f32,
    },
    TouchMove {
        x: f32,
        y: f32,
    },
    TouchUp {
        x: f32,
        y: f32,
    },
    TypeText(String),
    SendKey(String), // Key code like "Up", "Down", "Home", "End", "Return", "a", etc.
    SendKeyWithModifiers {
        key: String,
        shift: bool,
        ctrl: bool,
        alt: bool,
        meta: bool,
    },
    WaitForIdle,
    GetSemantics,
    Exit,
}

/// Robot response
#[cfg(feature = "robot")]
#[derive(Debug)]
enum RobotResponse {
    Ok,
    Semantics(Vec<SemanticElement>),
    Error(String),
}

/// Robot controller for the event loop
#[cfg(feature = "robot")]
struct RobotController {
    rx: mpsc::Receiver<RobotCommand>,
    tx: mpsc::Sender<RobotResponse>,
    waiting_for_idle: bool,
    idle_iterations: u32,
}

#[cfg(feature = "robot")]
impl RobotController {
    fn new() -> (Self, Robot) {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (resp_tx, resp_rx) = mpsc::channel();

        let controller = RobotController {
            rx: cmd_rx,
            tx: resp_tx,
            waiting_for_idle: false,
            idle_iterations: 0,
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
    ///
    /// This simulates a full click (mouse down then mouse up) at the given location.
    ///
    /// # Example
    /// ```text
    /// robot.click(100.0, 200.0)?;
    /// ```
    pub fn click(&self, x: f32, y: f32) -> Result<(), String> {
        self.tx
            .send(RobotCommand::Click { x, y })
            .map_err(|e| format!("Failed to send click command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Ok) => Ok(()),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Move cursor to the specified coordinates (logical pixels)
    ///
    /// # Example
    /// ```text
    /// robot.move_to(150.0, 250.0)?;
    /// ```
    pub fn move_to(&self, x: f32, y: f32) -> Result<(), String> {
        self.tx
            .send(RobotCommand::MoveTo { x, y })
            .map_err(|e| format!("Failed to send move command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Ok) => Ok(()),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Alias for move_to
    pub fn mouse_move(&self, x: f32, y: f32) -> Result<(), String> {
        self.move_to(x, y)
    }

    /// Press the left mouse button at the current cursor position
    ///
    /// # Example
    /// ```text
    /// robot.mouse_down()?;
    /// ```
    pub fn mouse_down(&self) -> Result<(), String> {
        self.tx
            .send(RobotCommand::MouseDown)
            .map_err(|e| format!("Failed to send mouse down command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Ok) => Ok(()),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Release the left mouse button at the current cursor position
    ///
    /// # Example
    /// ```text
    /// robot.mouse_up()?;
    /// ```
    pub fn mouse_up(&self) -> Result<(), String> {
        self.tx
            .send(RobotCommand::MouseUp)
            .map_err(|e| format!("Failed to send mouse up command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Ok) => Ok(()),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Perform a drag gesture from one point to another
    ///
    /// This simulates a pointer down, move, and up sequence with multiple intermediate
    /// steps to create a smooth drag gesture.
    ///
    /// # Arguments
    /// * `from_x` - Starting x coordinate (logical pixels)
    /// * `from_y` - Starting y coordinate (logical pixels)
    /// * `to_x` - Ending x coordinate (logical pixels)
    /// * `to_y` - Ending y coordinate (logical pixels)
    ///
    /// # Example
    /// ```text
    /// // Drag from left to right to scroll
    /// robot.drag(400.0, 200.0, 100.0, 200.0)?;
    /// ```
    pub fn drag(&self, from_x: f32, from_y: f32, to_x: f32, to_y: f32) -> Result<(), String> {
        // Touch down at start position
        self.tx
            .send(RobotCommand::TouchDown {
                x: from_x,
                y: from_y,
            })
            .map_err(|e| format!("Failed to send touch down: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Ok) => {}
            Ok(RobotResponse::Error(e)) => return Err(e),
            Ok(_) => return Err("Unexpected response".to_string()),
            Err(e) => return Err(format!("Failed to receive response: {}", e)),
        }

        // Move in steps to simulate smooth drag
        let steps = 10;
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let x = from_x + (to_x - from_x) * t;
            let y = from_y + (to_y - from_y) * t;

            self.tx
                .send(RobotCommand::TouchMove { x, y })
                .map_err(|e| format!("Failed to send touch move: {}", e))?;
            match self.rx.recv() {
                Ok(RobotResponse::Ok) => {}
                Ok(RobotResponse::Error(e)) => return Err(e),
                Ok(_) => return Err("Unexpected response".to_string()),
                Err(e) => return Err(format!("Failed to receive response: {}", e)),
            }
        }

        // Touch up at end position
        self.tx
            .send(RobotCommand::TouchUp { x: to_x, y: to_y })
            .map_err(|e| format!("Failed to send touch up: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Ok) => Ok(()),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Wait for the application to be idle (no redraws, no animations)
    ///
    /// This is crucial for synchronizing tests with the app state.
    /// It blocks until the app reports no pending updates.
    ///
    /// # Example
    /// ```text
    /// robot.click(10.0, 10.0)?;
    /// robot.wait_for_idle()?; // Wait for click to be processed
    /// ```
    pub fn wait_for_idle(&self) -> Result<(), String> {
        self.tx
            .send(RobotCommand::WaitForIdle)
            .map_err(|e| format!("Failed to send wait command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Ok) => Ok(()),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Type text into the currently focused text field
    ///
    /// This sends synthetic keyboard events for each character in the string.
    /// The text field must already be focused (e.g., via a click).
    ///
    /// # Example
    /// ```text
    /// robot.click(100.0, 200.0)?; // Focus the text field
    /// robot.type_text("Hello World")?;
    /// ```
    pub fn type_text(&self, text: &str) -> Result<(), String> {
        self.tx
            .send(RobotCommand::TypeText(text.to_string()))
            .map_err(|e| format!("Failed to send type_text command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Ok) => Ok(()),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Send a key press event
    ///
    /// Simulates pressing and releasing a key. Supports:
    /// - Letters: "a" to "z"
    /// - Navigation: "Up", "Down", "Left", "Right", "Home", "End"
    /// - Editing: "Return" (Enter), "BackSpace", "Delete"
    ///
    /// # Example
    /// ```text
    /// robot.send_key("Return")?; // Press Enter
    /// robot.send_key("Up")?; // Press Up arrow
    /// ```
    pub fn send_key(&self, key: &str) -> Result<(), String> {
        self.tx
            .send(RobotCommand::SendKey(key.to_string()))
            .map_err(|e| format!("Failed to send send_key command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Ok) => Ok(()),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Send a key press event with modifier keys
    ///
    /// Simulates pressing a key with modifiers (Shift, Ctrl, Alt, Meta).
    /// Useful for selection (Shift+Arrow), copy (Ctrl+C), paste (Ctrl+V).
    ///
    /// # Example
    /// ```text
    /// robot.send_key_with_modifiers("Left", true, false, false, false)?; // Shift+Left (select)
    /// robot.send_key_with_modifiers("c", false, true, false, false)?; // Ctrl+C (copy)
    /// robot.send_key_with_modifiers("v", false, true, false, false)?; // Ctrl+V (paste)
    /// ```
    pub fn send_key_with_modifiers(
        &self,
        key: &str,
        shift: bool,
        ctrl: bool,
        alt: bool,
        meta: bool,
    ) -> Result<(), String> {
        self.tx
            .send(RobotCommand::SendKeyWithModifiers {
                key: key.to_string(),
                shift,
                ctrl,
                alt,
                meta,
            })
            .map_err(|e| format!("Failed to send send_key_with_modifiers command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Ok) => Ok(()),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Exit the application
    ///
    /// This checks if the app is still running and sends an exit command.
    ///
    /// # Example
    /// ```text
    /// robot.exit()?;
    /// ```
    pub fn exit(&self) -> Result<(), String> {
        self.tx
            .send(RobotCommand::Exit)
            .map_err(|e| format!("Failed to send exit command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Ok) => Ok(()),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Get semantic tree with geometric bounds
    ///
    /// Returns the current accessibility/semantic tree of the application.
    /// This is the primary way to inspect the UI state.
    ///
    /// # Example
    /// ```text
    /// let elements = robot.get_semantics()?;
    /// assert!(!elements.is_empty());
    /// ```
    pub fn get_semantics(&self) -> Result<Vec<SemanticElement>, String> {
        self.tx
            .send(RobotCommand::GetSemantics)
            .map_err(|e| format!("Failed to send get_semantics: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Semantics(elements)) => Ok(elements),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive: {}", e)),
        }
    }

    /// Find any element by text content (recursive search)
    pub fn find_by_text<'a>(
        elements: &'a [SemanticElement],
        text: &str,
    ) -> Option<&'a SemanticElement> {
        for elem in elements {
            if let Some(elem_text) = &elem.text {
                if elem_text.contains(text) {
                    return Some(elem);
                }
            }
            if let Some(found) = Self::find_by_text(&elem.children, text) {
                return Some(found);
            }
        }
        None
    }

    /// Find clickable element by text content (recursive search)
    ///
    /// In Compose, buttons are often Layout elements with clickable actions
    /// containing Text children. This searches for clickable elements where
    /// either the element itself or its children contain the text.
    pub fn find_button<'a>(
        elements: &'a [SemanticElement],
        text: &str,
    ) -> Option<&'a SemanticElement> {
        for elem in elements {
            if elem.clickable {
                // Check if this clickable element or its children have the text
                if Self::contains_text(elem, text) {
                    return Some(elem);
                }
            }
            // Recurse into children
            if let Some(found) = Self::find_button(&elem.children, text) {
                return Some(found);
            }
        }
        None
    }

    /// Helper: check if element or any descendants contain text
    fn contains_text(elem: &SemanticElement, text: &str) -> bool {
        // Check element itself
        if let Some(elem_text) = &elem.text {
            if elem_text.contains(text) {
                return true;
            }
        }
        // Check children recursively
        for child in &elem.children {
            if Self::contains_text(child, text) {
                return true;
            }
        }
        false
    }

    /// Click element by finding it in semantic tree
    ///
    /// This is a convenience method that combines `get_semantics()`, `find_button()`,
    /// and `click()` in one call. It finds a clickable element by text and clicks
    /// its center point.
    ///
    /// # Example
    /// ```text
    /// robot.click_by_text("Increment")?;
    /// ```
    pub fn click_by_text(&self, text: &str) -> Result<(), String> {
        let semantics = self.get_semantics()?;
        let elem = Self::find_button(&semantics, text)
            .ok_or_else(|| format!("Button '{}' not found in semantic tree", text))?;

        // Click center of bounds
        let center_x = elem.bounds.x + elem.bounds.width / 2.0;
        let center_y = elem.bounds.y + elem.bounds.height / 2.0;

        self.click(center_x, center_y)
    }

    /// Validate that content exists in semantic tree
    ///
    /// Returns Ok if the text is found anywhere in the semantic tree,
    /// Err otherwise. Useful for assertions in tests.
    ///
    /// # Example
    /// ```text
    /// robot.validate_content("Expected Text")?;
    /// ```
    pub fn validate_content(&self, expected: &str) -> Result<(), String> {
        let semantics = self.get_semantics()?;
        if Self::find_by_text(&semantics, expected).is_some() {
            Ok(())
        } else {
            Err(format!("Validation failed: '{}' not found", expected))
        }
    }

    /// Print semantic tree structure for debugging
    ///
    /// Prints a hierarchical view of the semantic tree showing roles,
    /// text content, and clickable elements.
    ///
    /// # Example
    /// ```text
    /// let semantics = robot.get_semantics()?;
    /// Robot::print_semantics(&semantics, 0);
    /// ```
    pub fn print_semantics(elements: &[SemanticElement], indent: usize) {
        for elem in elements {
            let prefix = "  ".repeat(indent);
            let text_info = elem
                .text
                .as_ref()
                .map(|t| format!(" text=\"{}\"", t))
                .unwrap_or_default();
            let clickable = if elem.clickable { " [CLICKABLE]" } else { "" };
            println!("{}role={}{}{}", prefix, elem.role, text_info, clickable);
            Self::print_semantics(&elem.children, indent + 1);
        }
    }
}

/// Application state that implements winit's ApplicationHandler
struct App {
    /// Settings for the application
    settings: AppSettings,
    /// Content function to be called (taken on first resume)
    content: Option<Box<dyn FnMut()>>,
    /// Window (created when surfaces can be created)
    window: Option<Arc<dyn Window>>,
    /// WGPU surface
    surface: Option<wgpu::Surface<'static>>,
    /// Surface configuration
    surface_config: Option<wgpu::SurfaceConfiguration>,
    /// Compose app shell
    app: Option<AppShell<WgpuRenderer>>,
    /// Platform adapter
    platform: Option<DesktopWinitPlatform>,
    /// Current keyboard modifiers (shift, ctrl, alt, meta)
    current_modifiers: winit::keyboard::ModifiersState,
    /// Robot controller
    #[cfg(feature = "robot")]
    robot_controller: Option<RobotController>,
    /// Input recorder for generating robot tests
    recorder: Option<crate::recorder::InputRecorder>,
}

impl App {
    fn new(mut settings: AppSettings, content: impl FnMut() + 'static) -> Self {
        // Create recorder if recording is enabled
        let recorder = settings
            .record_to
            .take()
            .map(crate::recorder::InputRecorder::new);

        Self {
            settings,
            content: Some(Box::new(content)),
            window: None,
            surface: None,
            surface_config: None,
            app: None,
            platform: None,
            current_modifiers: winit::keyboard::ModifiersState::empty(),
            #[cfg(feature = "robot")]
            robot_controller: None,
            recorder,
        }
    }

    #[cfg(feature = "robot")]
    fn set_robot_controller(&mut self, controller: RobotController) {
        self.robot_controller = Some(controller);
    }
}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        // Create window if not already created
        if self.window.is_some() {
            return;
        }

        let initial_width = self.settings.initial_width;
        let initial_height = self.settings.initial_height;
        let headless = self.settings.headless;

        let window: Arc<dyn Window> = Arc::from(
            event_loop
                .create_window(
                    WindowAttributes::default()
                        .with_title(self.settings.window_title.clone())
                        .with_surface_size(LogicalSize::new(
                            initial_width as f64,
                            initial_height as f64,
                        ))
                        // Hide window in headless mode for parallel robot testing
                        .with_visible(!headless),
                )
                .expect("failed to create window"),
        );

        // Initialize WGPU
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance
            .create_surface(window.clone())
            .expect("failed to create surface");

        let adapter =
            match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })) {
                Ok(adapter) => adapter,
                Err(e) => {
                    // Provide helpful error message for GPU issues
                    eprintln!(
                        "\n╔══════════════════════════════════════════════════════════════════╗"
                    );
                    eprintln!(
                        "║                    GPU ADAPTER NOT FOUND                          ║"
                    );
                    eprintln!(
                        "╠══════════════════════════════════════════════════════════════════╣"
                    );
                    eprintln!(
                        "║ No suitable graphics adapter was found. This usually means:      ║"
                    );
                    eprintln!(
                        "║                                                                  ║"
                    );
                    eprintln!(
                        "║   • GPU drivers are not installed or not working                 ║"
                    );
                    eprintln!(
                        "║   • Vulkan/OpenGL support is missing or broken                   ║"
                    );
                    eprintln!(
                        "║   • A recent system update broke graphics drivers                ║"
                    );
                    eprintln!(
                        "║                                                                  ║"
                    );
                    eprintln!(
                        "║ To fix this on Linux:                                            ║"
                    );
                    eprintln!(
                        "║   1. Check Vulkan: vulkaninfo | head -20                         ║"
                    );
                    eprintln!(
                        "║   2. Reinstall drivers:                                          ║"
                    );
                    eprintln!(
                        "║      - Mesa: sudo pacman -S mesa vulkan-mesa-layers              ║"
                    );
                    eprintln!(
                        "║      - NVIDIA: sudo pacman -S nvidia-utils                       ║"
                    );
                    eprintln!(
                        "║   3. Reboot your system                                          ║"
                    );
                    eprintln!(
                        "║                                                                  ║"
                    );
                    eprintln!("║ Technical details: {:?}", e);
                    eprintln!(
                        "╚══════════════════════════════════════════════════════════════════╝\n"
                    );
                    event_loop.exit();
                    return;
                }
            };

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Main Device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .expect("failed to create device");

        let size = window.surface_size();
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let present_mode = crate::present_mode::select_present_mode(&surface_caps);
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &surface_config);

        // Create renderer with fonts from settings
        let mut renderer = if let Some(fonts) = self.settings.fonts.take() {
            WgpuRenderer::new_with_fonts(fonts)
        } else {
            WgpuRenderer::new()
        };
        renderer.init_gpu(Arc::new(device), Arc::new(queue), surface_format);
        let initial_scale = window.scale_factor();
        renderer.set_root_scale(initial_scale as f32);
        cranpose_ui::set_density(initial_scale as f32);

        // Take the content closure (can only be called once)
        let content = self.content.take().expect("content already taken");
        let mut app = AppShell::new(renderer, default_root_key(), content);

        // Apply dev options (FPS counter, etc.)
        app.set_dev_options(self.settings.dev_options.clone());

        let mut platform = DesktopWinitPlatform::default();
        platform.set_scale_factor(initial_scale);

        // Set buffer_size to physical pixels and viewport to logical dp
        app.set_buffer_size(size.width, size.height);
        let logical_width = size.width as f32 / initial_scale as f32;
        let logical_height = size.height as f32 / initial_scale as f32;
        app.set_viewport(logical_width, logical_height);

        self.window = Some(window);
        self.surface = Some(surface);
        self.surface_config = Some(surface_config);
        self.app = Some(app);
        self.platform = Some(platform);
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = &self.window else { return };
        if window_id != window.id() {
            return;
        }

        let Some(app) = &mut self.app else { return };
        let Some(platform) = &mut self.platform else {
            return;
        };
        let Some(surface) = &self.surface else { return };
        let Some(surface_config) = &mut self.surface_config else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => {
                // Save recording if active
                if let Some(recorder) = self.recorder.take() {
                    if let Err(e) = recorder.finish() {
                        eprintln!("[Recorder] Error saving recording: {}", e);
                    }
                }
                event_loop.exit();
            }
            WindowEvent::SurfaceResized(new_size) => {
                if new_size.width > 0 && new_size.height > 0 {
                    surface_config.width = new_size.width;
                    surface_config.height = new_size.height;
                    let device = app.renderer().device();
                    surface.configure(device, surface_config);

                    let scale_factor = window.scale_factor();
                    let logical_width = new_size.width as f32 / scale_factor as f32;
                    let logical_height = new_size.height as f32 / scale_factor as f32;

                    app.set_buffer_size(new_size.width, new_size.height);
                    app.set_viewport(logical_width, logical_height);
                }
            }
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                surface_size_writer,
            } => {
                platform.set_scale_factor(scale_factor);
                app.renderer().set_root_scale(scale_factor as f32);
                cranpose_ui::set_density(scale_factor as f32);

                let new_size = surface_size_writer
                    .surface_size()
                    .unwrap_or_else(|_| window.surface_size());
                if new_size.width > 0 && new_size.height > 0 {
                    surface_config.width = new_size.width;
                    surface_config.height = new_size.height;
                    let device = app.renderer().device();
                    surface.configure(device, surface_config);

                    let logical_width = new_size.width as f32 / scale_factor as f32;
                    let logical_height = new_size.height as f32 / scale_factor as f32;

                    app.set_buffer_size(new_size.width, new_size.height);
                    app.set_viewport(logical_width, logical_height);
                }
            }
            WindowEvent::PointerMoved {
                position, primary, ..
            } => {
                if primary {
                    let logical = platform.pointer_position(position);
                    app.set_cursor(logical.x, logical.y);
                    // Record mouse move
                    if let Some(recorder) = &mut self.recorder {
                        recorder.record_mouse_move(logical.x, logical.y);
                    }
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                // Track current keyboard modifiers for key events
                self.current_modifiers = modifiers.state();
            }
            WindowEvent::PointerButton {
                state,
                position,
                primary,
                button: ButtonSource::Mouse(MouseButton::Left),
                ..
            } => {
                if primary {
                    let logical = platform.pointer_position(position);
                    app.set_cursor(logical.x, logical.y);
                    match state {
                        ElementState::Pressed => {
                            app.pointer_pressed();
                            // Record mouse down
                            if let Some(recorder) = &mut self.recorder {
                                recorder.record_mouse_down();
                            }
                        }
                        ElementState::Released => {
                            app.pointer_released();
                            // Sync selection to PRIMARY (Linux X11 middle-click paste)
                            app.sync_selection_to_primary();
                            // Record mouse up
                            if let Some(recorder) = &mut self.recorder {
                                recorder.record_mouse_up();
                            }
                        }
                    }
                }
            }
            // Middle-click paste from Linux primary selection
            WindowEvent::PointerButton {
                state: ElementState::Pressed,
                position,
                primary,
                button: ButtonSource::Mouse(MouseButton::Middle),
                ..
            } => {
                if primary {
                    let logical = platform.pointer_position(position);
                    app.set_cursor(logical.x, logical.y);
                }
                #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
                if let Some(text) = app.get_primary_selection() {
                    if app.on_paste(&text) {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                use cranpose_app_shell::{KeyCode, KeyEvent, KeyEventType, Modifiers};
                use winit::keyboard::{Key, PhysicalKey};

                // Convert winit key event to cranpose-ui KeyEvent
                let event_type = match event.state {
                    ElementState::Pressed => KeyEventType::KeyDown,
                    ElementState::Released => KeyEventType::KeyUp,
                };

                // Get text from logical key
                let text = match &event.logical_key {
                    Key::Character(s) => s.to_string(),
                    _ => String::new(),
                };

                // Convert physical key to KeyCode
                let key_code = match event.physical_key {
                    PhysicalKey::Code(code) => match code {
                        winit::keyboard::KeyCode::KeyA => KeyCode::A,
                        winit::keyboard::KeyCode::KeyB => KeyCode::B,
                        winit::keyboard::KeyCode::KeyC => KeyCode::C,
                        winit::keyboard::KeyCode::KeyD => KeyCode::D,
                        winit::keyboard::KeyCode::KeyE => KeyCode::E,
                        winit::keyboard::KeyCode::KeyF => KeyCode::F,
                        winit::keyboard::KeyCode::KeyG => KeyCode::G,
                        winit::keyboard::KeyCode::KeyH => KeyCode::H,
                        winit::keyboard::KeyCode::KeyI => KeyCode::I,
                        winit::keyboard::KeyCode::KeyJ => KeyCode::J,
                        winit::keyboard::KeyCode::KeyK => KeyCode::K,
                        winit::keyboard::KeyCode::KeyL => KeyCode::L,
                        winit::keyboard::KeyCode::KeyM => KeyCode::M,
                        winit::keyboard::KeyCode::KeyN => KeyCode::N,
                        winit::keyboard::KeyCode::KeyO => KeyCode::O,
                        winit::keyboard::KeyCode::KeyP => KeyCode::P,
                        winit::keyboard::KeyCode::KeyQ => KeyCode::Q,
                        winit::keyboard::KeyCode::KeyR => KeyCode::R,
                        winit::keyboard::KeyCode::KeyS => KeyCode::S,
                        winit::keyboard::KeyCode::KeyT => KeyCode::T,
                        winit::keyboard::KeyCode::KeyU => KeyCode::U,
                        winit::keyboard::KeyCode::KeyV => KeyCode::V,
                        winit::keyboard::KeyCode::KeyW => KeyCode::W,
                        winit::keyboard::KeyCode::KeyX => KeyCode::X,
                        winit::keyboard::KeyCode::KeyY => KeyCode::Y,
                        winit::keyboard::KeyCode::KeyZ => KeyCode::Z,
                        winit::keyboard::KeyCode::Digit0 => KeyCode::Digit0,
                        winit::keyboard::KeyCode::Digit1 => KeyCode::Digit1,
                        winit::keyboard::KeyCode::Digit2 => KeyCode::Digit2,
                        winit::keyboard::KeyCode::Digit3 => KeyCode::Digit3,
                        winit::keyboard::KeyCode::Digit4 => KeyCode::Digit4,
                        winit::keyboard::KeyCode::Digit5 => KeyCode::Digit5,
                        winit::keyboard::KeyCode::Digit6 => KeyCode::Digit6,
                        winit::keyboard::KeyCode::Digit7 => KeyCode::Digit7,
                        winit::keyboard::KeyCode::Digit8 => KeyCode::Digit8,
                        winit::keyboard::KeyCode::Digit9 => KeyCode::Digit9,
                        winit::keyboard::KeyCode::Backspace => KeyCode::Backspace,
                        winit::keyboard::KeyCode::Delete => KeyCode::Delete,
                        winit::keyboard::KeyCode::Enter => KeyCode::Enter,
                        winit::keyboard::KeyCode::Tab => KeyCode::Tab,
                        winit::keyboard::KeyCode::Space => KeyCode::Space,
                        winit::keyboard::KeyCode::Escape => KeyCode::Escape,
                        winit::keyboard::KeyCode::ArrowUp => KeyCode::ArrowUp,
                        winit::keyboard::KeyCode::ArrowDown => KeyCode::ArrowDown,
                        winit::keyboard::KeyCode::ArrowLeft => KeyCode::ArrowLeft,
                        winit::keyboard::KeyCode::ArrowRight => KeyCode::ArrowRight,
                        winit::keyboard::KeyCode::Home => KeyCode::Home,
                        winit::keyboard::KeyCode::End => KeyCode::End,
                        _ => KeyCode::Unknown,
                    },
                    _ => KeyCode::Unknown,
                };

                // Convert winit modifier state to our Modifiers struct
                let modifiers = Modifiers {
                    shift: self
                        .current_modifiers
                        .contains(winit::keyboard::ModifiersState::SHIFT),
                    ctrl: self
                        .current_modifiers
                        .contains(winit::keyboard::ModifiersState::CONTROL),
                    alt: self
                        .current_modifiers
                        .contains(winit::keyboard::ModifiersState::ALT),
                    meta: self
                        .current_modifiers
                        .contains(winit::keyboard::ModifiersState::META),
                };

                let key_event = KeyEvent::new(key_code, text, modifiers, event_type);

                // Special: still handle D for debug info
                if key_code == KeyCode::D && event_type == KeyEventType::KeyDown {
                    app.log_debug_info();
                }

                // Dispatch to text fields
                if app.on_key_event(&key_event) {
                    window.request_redraw();
                }
            }
            WindowEvent::Focused(false) => {
                // Window lost focus - cancel any in-progress gestures
                app.cancel_gesture();
                // Clear any active IME composition
                let _ = app.on_ime_preedit("", None);
            }
            WindowEvent::Ime(ime_event) => {
                use winit::event::Ime;
                match ime_event {
                    Ime::Preedit(text, cursor) => {
                        // IME is composing - show preedit text with underline
                        if app.on_ime_preedit(&text, cursor) {
                            window.request_redraw();
                        }
                    }
                    Ime::Commit(text) => {
                        // IME finished - commit the final text
                        // First clear composition state, then insert the final text
                        let _ = app.on_ime_preedit("", None);
                        if app.on_paste(&text) {
                            window.request_redraw();
                        }
                    }
                    Ime::DeleteSurrounding {
                        before_bytes,
                        after_bytes,
                    } => {
                        if app.on_ime_delete_surrounding(before_bytes, after_bytes) {
                            window.request_redraw();
                        }
                    }
                    Ime::Enabled => {
                        // IME was enabled - no action needed
                    }
                    Ime::Disabled => {
                        // IME was disabled - clear any composition state
                        if app.on_ime_preedit("", None) {
                            window.request_redraw();
                        }
                    }
                }
            }
            WindowEvent::PointerLeft { primary, .. } => {
                if primary {
                    // Pointer left the window - cancel any in-progress gestures
                    app.cancel_gesture();
                }
            }
            WindowEvent::RedrawRequested => {
                app.update();

                let output = match surface.get_current_texture() {
                    Ok(output) => output,
                    Err(wgpu::SurfaceError::Lost) | Err(wgpu::SurfaceError::Outdated) => {
                        // Reconfigure surface with current window size
                        let size = window.surface_size();
                        if size.width > 0 && size.height > 0 {
                            surface_config.width = size.width;
                            surface_config.height = size.height;
                            let device = app.renderer().device();
                            surface.configure(device, surface_config);
                        }
                        return;
                    }
                    Err(wgpu::SurfaceError::OutOfMemory) => {
                        log::error!("Out of memory, exiting");
                        event_loop.exit();
                        return;
                    }
                    Err(wgpu::SurfaceError::Timeout) => {
                        log::debug!("Surface timeout, skipping frame");
                        return;
                    }
                    Err(wgpu::SurfaceError::Other) => {
                        log::error!("Surface other error, skipping frame");
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
        }
    }

    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        let Some(app) = &mut self.app else { return };
        let Some(window) = &self.window else { return };

        // Handle pending robot commands
        #[cfg(feature = "robot")]
        if let Some(controller) = &mut self.robot_controller {
            // Process new commands
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
                        // Record for robot test generation
                        if let Some(recorder) = &mut self.recorder {
                            recorder.record_mouse_move(x, y);
                        }
                        window.request_redraw();
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::MouseDown => {
                        app.pointer_pressed();
                        // Record for robot test generation
                        if let Some(recorder) = &mut self.recorder {
                            recorder.record_mouse_down();
                        }
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::MouseUp => {
                        app.pointer_released();
                        // Record for robot test generation
                        if let Some(recorder) = &mut self.recorder {
                            recorder.record_mouse_up();
                        }
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
                    RobotCommand::GetSemantics => {
                        let semantics = extract_semantics(app);
                        let _ = controller.tx.send(RobotResponse::Semantics(semantics));
                    }
                    RobotCommand::TypeText(text) => {
                        use cranpose_app_shell::{KeyEvent, KeyEventType, Modifiers};

                        // Send key events for each character
                        for ch in text.chars() {
                            // Map character to key code (simplified)
                            let key_code = char_to_key_code(ch);
                            let key_event = KeyEvent::new(
                                key_code,
                                ch.to_string(),
                                Modifiers::NONE,
                                KeyEventType::KeyDown,
                            );
                            app.on_key_event(&key_event);
                        }
                        // Process the key events immediately to update layout/semantics
                        app.update();
                        window.request_redraw();
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::SendKey(key) => {
                        use cranpose_app_shell::{KeyCode, KeyEvent, KeyEventType, Modifiers};

                        // Map key string to KeyCode and text
                        let (key_code, text) = match key.as_str() {
                            // Navigation keys
                            "Up" => (KeyCode::ArrowUp, String::new()),
                            "Down" => (KeyCode::ArrowDown, String::new()),
                            "Left" => (KeyCode::ArrowLeft, String::new()),
                            "Right" => (KeyCode::ArrowRight, String::new()),
                            "Home" => (KeyCode::Home, String::new()),
                            "End" => (KeyCode::End, String::new()),
                            // Editing keys
                            "Return" => (KeyCode::Enter, String::from("\n")),
                            "BackSpace" => (KeyCode::Backspace, String::new()),
                            "Delete" => (KeyCode::Delete, String::new()),
                            "Tab" => (KeyCode::Tab, String::from("\t")),
                            "space" => (KeyCode::Space, String::from(" ")),
                            // Letters
                            "a" => (KeyCode::A, String::from("a")),
                            "b" => (KeyCode::B, String::from("b")),
                            "c" => (KeyCode::C, String::from("c")),
                            "d" => (KeyCode::D, String::from("d")),
                            "e" => (KeyCode::E, String::from("e")),
                            "f" => (KeyCode::F, String::from("f")),
                            "g" => (KeyCode::G, String::from("g")),
                            "h" => (KeyCode::H, String::from("h")),
                            "i" => (KeyCode::I, String::from("i")),
                            "j" => (KeyCode::J, String::from("j")),
                            "k" => (KeyCode::K, String::from("k")),
                            "l" => (KeyCode::L, String::from("l")),
                            "m" => (KeyCode::M, String::from("m")),
                            "n" => (KeyCode::N, String::from("n")),
                            "o" => (KeyCode::O, String::from("o")),
                            "p" => (KeyCode::P, String::from("p")),
                            "q" => (KeyCode::Q, String::from("q")),
                            "r" => (KeyCode::R, String::from("r")),
                            "s" => (KeyCode::S, String::from("s")),
                            "t" => (KeyCode::T, String::from("t")),
                            "u" => (KeyCode::U, String::from("u")),
                            "v" => (KeyCode::V, String::from("v")),
                            "w" => (KeyCode::W, String::from("w")),
                            "x" => (KeyCode::X, String::from("x")),
                            "y" => (KeyCode::Y, String::from("y")),
                            "z" => (KeyCode::Z, String::from("z")),
                            _ => (KeyCode::Unknown, String::new()),
                        };

                        let key_event =
                            KeyEvent::new(key_code, text, Modifiers::NONE, KeyEventType::KeyDown);
                        app.on_key_event(&key_event);
                        app.update();
                        window.request_redraw();
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::SendKeyWithModifiers {
                        key,
                        shift,
                        ctrl,
                        alt,
                        meta,
                    } => {
                        use cranpose_app_shell::{KeyCode, KeyEvent, KeyEventType, Modifiers};

                        // Map key string to KeyCode and text (same as SendKey)
                        let (key_code, text) = match key.as_str() {
                            // Navigation keys
                            "Up" => (KeyCode::ArrowUp, String::new()),
                            "Down" => (KeyCode::ArrowDown, String::new()),
                            "Left" => (KeyCode::ArrowLeft, String::new()),
                            "Right" => (KeyCode::ArrowRight, String::new()),
                            "Home" => (KeyCode::Home, String::new()),
                            "End" => (KeyCode::End, String::new()),
                            // Editing keys
                            "Return" => (KeyCode::Enter, String::from("\n")),
                            "BackSpace" => (KeyCode::Backspace, String::new()),
                            "Delete" => (KeyCode::Delete, String::new()),
                            "Tab" => (KeyCode::Tab, String::from("\t")),
                            "space" => (KeyCode::Space, String::from(" ")),
                            // Letters
                            "a" => (KeyCode::A, String::from("a")),
                            "b" => (KeyCode::B, String::from("b")),
                            "c" => (KeyCode::C, String::from("c")),
                            "d" => (KeyCode::D, String::from("d")),
                            "e" => (KeyCode::E, String::from("e")),
                            "f" => (KeyCode::F, String::from("f")),
                            "g" => (KeyCode::G, String::from("g")),
                            "h" => (KeyCode::H, String::from("h")),
                            "i" => (KeyCode::I, String::from("i")),
                            "j" => (KeyCode::J, String::from("j")),
                            "k" => (KeyCode::K, String::from("k")),
                            "l" => (KeyCode::L, String::from("l")),
                            "m" => (KeyCode::M, String::from("m")),
                            "n" => (KeyCode::N, String::from("n")),
                            "o" => (KeyCode::O, String::from("o")),
                            "p" => (KeyCode::P, String::from("p")),
                            "q" => (KeyCode::Q, String::from("q")),
                            "r" => (KeyCode::R, String::from("r")),
                            "s" => (KeyCode::S, String::from("s")),
                            "t" => (KeyCode::T, String::from("t")),
                            "u" => (KeyCode::U, String::from("u")),
                            "v" => (KeyCode::V, String::from("v")),
                            "w" => (KeyCode::W, String::from("w")),
                            "x" => (KeyCode::X, String::from("x")),
                            "y" => (KeyCode::Y, String::from("y")),
                            "z" => (KeyCode::Z, String::from("z")),
                            _ => (KeyCode::Unknown, String::new()),
                        };

                        let modifiers = Modifiers {
                            shift,
                            ctrl,
                            alt,
                            meta,
                        };
                        let key_event =
                            KeyEvent::new(key_code, text, modifiers, KeyEventType::KeyDown);
                        app.on_key_event(&key_event);
                        app.update();
                        window.request_redraw();
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::WaitForIdle => {
                        // Start waiting for idle
                        controller.waiting_for_idle = true;
                        controller.idle_iterations = 0;
                    }
                    RobotCommand::Exit => {
                        let _ = controller.tx.send(RobotResponse::Ok);
                        event_loop.exit();
                    }
                }
            }

            // Handle ongoing wait_for_idle
            if controller.waiting_for_idle {
                const MAX_IDLE_ITERATIONS: u32 = 200;

                let needs_draw = app.needs_redraw();
                let has_anim = app.has_active_animations();

                if !needs_draw && !has_anim {
                    // App is idle - respond and stop waiting
                    controller.waiting_for_idle = false;
                    let _ = controller.tx.send(RobotResponse::Ok);
                } else {
                    // Not idle yet - update and check iteration limit
                    app.update();
                    controller.idle_iterations += 1;

                    // Periodic diagnostic logging
                    if controller.idle_iterations % 50 == 0 {
                        log::debug!(
                            "wait_for_idle iteration {}: needs_redraw={}, has_animations={}",
                            controller.idle_iterations,
                            app.needs_redraw(),
                            app.has_active_animations()
                        );
                    }

                    if controller.idle_iterations >= MAX_IDLE_ITERATIONS {
                        controller.waiting_for_idle = false;
                        let _ = controller.tx.send(RobotResponse::Error(
                            "wait_for_idle: timed out after 200 iterations".to_string(),
                        ));
                    }
                }
            }
        }

        if app.needs_redraw() {
            window.request_redraw();
        }

        // Smart ControlFlow: only Poll when necessary
        #[cfg(feature = "robot")]
        let robot_needs_poll = self.robot_controller.is_some();

        #[cfg(not(feature = "robot"))]
        let robot_needs_poll = false;

        // Poll continuously when:
        // - Active animations are running
        // - Robot test is active
        if app.has_active_animations() || robot_needs_poll {
            event_loop.set_control_flow(ControlFlow::Poll);
        } else if let Some(next_time) = app.next_event_time() {
            // Cursor blink uses timer-based scheduling (not continuous poll)
            event_loop.set_control_flow(ControlFlow::WaitUntil(next_time));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}

/// Runs a desktop Compose application with wgpu rendering.
///
/// Called by `AppLauncher::run_desktop()`. This is the framework-level
/// entrypoint that manages the desktop event loop and rendering.
///
/// **Note:** Applications should use `AppLauncher` instead of calling this directly.
#[allow(unused_mut)]
pub fn run(mut settings: AppSettings, content: impl FnMut() + 'static) -> ! {
    let event_loop = EventLoop::builder()
        .build()
        .expect("failed to create event loop");

    // Spawn test driver if present
    #[cfg(feature = "robot")]
    let robot_controller = if let Some(driver) = settings.test_driver.take() {
        let (controller, robot) = RobotController::new();
        std::thread::spawn(move || {
            driver(robot);
        });
        Some(controller)
    } else {
        None
    };

    let mut app = App::new(settings, content);

    #[cfg(feature = "robot")]
    if let Some(controller) = robot_controller {
        app.set_robot_controller(controller);
    }

    let _ = event_loop.run_app(app);

    std::process::exit(0)
}

/// Extract semantic elements by combining semantic tree with layout tree
#[cfg(feature = "robot")]
fn extract_semantics(app: &AppShell<WgpuRenderer>) -> Vec<SemanticElement> {
    match (app.semantics_tree(), app.layout_tree()) {
        (Some(sem_tree), Some(layout_tree)) => {
            vec![combine_trees(sem_tree.root(), layout_tree.root())]
        }
        _ => Vec::new(),
    }
}

/// Recursively combine SemanticsNode + LayoutBox into SemanticElement
#[cfg(feature = "robot")]
fn combine_trees(sem_node: &SemanticsNode, layout_box: &LayoutBox) -> SemanticElement {
    // Extract role as string
    let role = match &sem_node.role {
        SemanticsRole::Button => "Button",
        SemanticsRole::Text { .. } => "Text",
        SemanticsRole::Layout => "Layout",
        SemanticsRole::Subcompose => "Subcompose",
        SemanticsRole::Spacer => "Spacer",
        SemanticsRole::Unknown => "Unknown",
    }
    .to_string();

    // Extract text content
    let text = match &sem_node.role {
        SemanticsRole::Text { value } => Some(value.clone()),
        _ => sem_node.description.clone(),
    };

    // Check if clickable
    let clickable = sem_node
        .actions
        .iter()
        .any(|action| matches!(action, SemanticsAction::Click { .. }));

    // Get bounds from layout
    let bounds = SemanticRect {
        x: layout_box.rect.x,
        y: layout_box.rect.y,
        width: layout_box.rect.width,
        height: layout_box.rect.height,
    };

    // Recursively process children
    let children = sem_node
        .children
        .iter()
        .zip(layout_box.children.iter())
        .map(|(sem_child, layout_child)| combine_trees(sem_child, layout_child))
        .collect();

    SemanticElement {
        role,
        text,
        bounds,
        clickable,
        children,
    }
}

/// Map a character to a KeyCode for robot typing
#[cfg(feature = "robot")]
fn char_to_key_code(ch: char) -> cranpose_app_shell::KeyCode {
    use cranpose_app_shell::KeyCode;

    match ch.to_ascii_lowercase() {
        'a' => KeyCode::A,
        'b' => KeyCode::B,
        'c' => KeyCode::C,
        'd' => KeyCode::D,
        'e' => KeyCode::E,
        'f' => KeyCode::F,
        'g' => KeyCode::G,
        'h' => KeyCode::H,
        'i' => KeyCode::I,
        'j' => KeyCode::J,
        'k' => KeyCode::K,
        'l' => KeyCode::L,
        'm' => KeyCode::M,
        'n' => KeyCode::N,
        'o' => KeyCode::O,
        'p' => KeyCode::P,
        'q' => KeyCode::Q,
        'r' => KeyCode::R,
        's' => KeyCode::S,
        't' => KeyCode::T,
        'u' => KeyCode::U,
        'v' => KeyCode::V,
        'w' => KeyCode::W,
        'x' => KeyCode::X,
        'y' => KeyCode::Y,
        'z' => KeyCode::Z,
        '0' => KeyCode::Digit0,
        '1' => KeyCode::Digit1,
        '2' => KeyCode::Digit2,
        '3' => KeyCode::Digit3,
        '4' => KeyCode::Digit4,
        '5' => KeyCode::Digit5,
        '6' => KeyCode::Digit6,
        '7' => KeyCode::Digit7,
        '8' => KeyCode::Digit8,
        '9' => KeyCode::Digit9,
        ' ' => KeyCode::Space,
        _ => KeyCode::Unknown,
    }
}
