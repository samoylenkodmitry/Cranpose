//! Desktop runtime for Compose applications.
//!
//! This module provides the desktop event loop implementation using winit.

use crate::launcher::AppSettings;
#[cfg(feature = "robot")]
use cranpose_app_shell::RuntimeLeakDebugStats;
use cranpose_app_shell::{default_root_key, AppShell};
use cranpose_platform_desktop_winit::DesktopWinitPlatform;
use cranpose_render_wgpu::WgpuRenderer;
#[cfg(feature = "robot")]
use cranpose_render_wgpu::{DebugCpuAllocationStats, RenderStatsSnapshot};
#[cfg(feature = "robot")]
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ButtonSource, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

#[cfg(feature = "robot")]
use cranpose_ui::{SemanticsAction, SemanticsNode, SemanticsRole};

#[cfg(feature = "robot")]
use std::sync::mpsc;

fn desktop_input_debug_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CRANPOSE_INPUT_DEBUG").is_some())
}

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
#[derive(Debug, Clone, Copy)]
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

#[cfg(feature = "robot")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SemanticTextMatchKind {
    Contains,
    Exact,
    Prefix,
}

#[cfg(feature = "robot")]
#[derive(Debug, Clone)]
struct SemanticQueryResult {
    node_id: cranpose_core::NodeId,
    bounds: SemanticRect,
    text: Option<String>,
}

#[cfg(feature = "robot")]
type TextMatchBounds = (f32, f32, f32, f32, String);

#[cfg(feature = "robot")]
fn pump_robot_frame(app: &mut AppShell<WgpuRenderer>) {
    if app.needs_redraw() || app.has_active_animations() {
        app.update();
    }
}

/// RGBA screenshot captured from the current render scene.
#[cfg(feature = "robot")]
#[derive(Debug, Clone)]
pub struct RobotScreenshot {
    /// Screenshot width in pixels.
    pub width: u32,
    /// Screenshot height in pixels.
    pub height: u32,
    /// Logical width covered by the screenshot.
    pub logical_width: f32,
    /// Logical height covered by the screenshot.
    pub logical_height: f32,
    /// Packed RGBA8 pixel buffer in row-major order.
    pub pixels: Vec<u8>,
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
    MouseScroll {
        delta_x: f32,
        delta_y: f32,
    },
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
    FindText {
        text: String,
        match_kind: SemanticTextMatchKind,
    },
    FindButton {
        text: String,
        match_kind: SemanticTextMatchKind,
    },
    GetScreenshot,
    GetScreenshotWithScale(f32),
    GetRenderStats,
    GetRenderCpuAllocationStats,
    GetRuntimeLeakDebugStats,
    SetSemanticsEnabled(bool),
    InvokeAppHook {
        name: String,
        argument: String,
    },
    Exit,
}

/// Robot response
#[cfg(feature = "robot")]
#[derive(Debug)]
enum RobotResponse {
    Ok,
    Semantics(Vec<SemanticElement>),
    SemanticQuery(Option<SemanticQueryResult>),
    Screenshot(RobotScreenshot),
    RenderStats(Box<Option<RenderStatsSnapshot>>),
    RenderCpuAllocationStats(Box<DebugCpuAllocationStats>),
    RuntimeLeakDebugStats(Box<RuntimeLeakDebugStats>),
    AppHookResult(Option<String>),
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

    /// Dispatch a mouse wheel / trackpad scroll delta at the current cursor position.
    ///
    /// Positive `delta_y` scrolls backward (content moves down), negative `delta_y`
    /// scrolls forward (content moves up), matching desktop event semantics.
    pub fn mouse_scroll(&self, delta_x: f32, delta_y: f32) -> Result<(), String> {
        self.tx
            .send(RobotCommand::MouseScroll { delta_x, delta_y })
            .map_err(|e| format!("Failed to send mouse scroll command: {}", e))?;
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

    fn request_semantic_query(
        &self,
        command: RobotCommand,
    ) -> Result<Option<SemanticQueryResult>, String> {
        self.tx
            .send(command)
            .map_err(|e| format!("Failed to send semantic query: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::SemanticQuery(result)) => Ok(result),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Find the first semantic node whose text contains the provided substring.
    pub fn find_text_bounds(&self, text: &str) -> Result<Option<(f32, f32, f32, f32)>, String> {
        Ok(self
            .request_semantic_query(RobotCommand::FindText {
                text: text.to_string(),
                match_kind: SemanticTextMatchKind::Contains,
            })?
            .map(|result| {
                (
                    result.bounds.x,
                    result.bounds.y,
                    result.bounds.width,
                    result.bounds.height,
                )
            }))
    }

    /// Find the first semantic node whose text starts with the provided prefix.
    pub fn find_text_by_prefix(&self, prefix: &str) -> Result<Option<TextMatchBounds>, String> {
        Ok(self
            .request_semantic_query(RobotCommand::FindText {
                text: prefix.to_string(),
                match_kind: SemanticTextMatchKind::Prefix,
            })?
            .and_then(|result| {
                result.text.map(|text| {
                    (
                        result.bounds.x,
                        result.bounds.y,
                        result.bounds.width,
                        result.bounds.height,
                        text,
                    )
                })
            }))
    }

    /// Find the first clickable semantic node whose subtree contains the provided substring.
    pub fn find_button_bounds(&self, text: &str) -> Result<Option<(f32, f32, f32, f32)>, String> {
        Ok(self
            .request_semantic_query(RobotCommand::FindButton {
                text: text.to_string(),
                match_kind: SemanticTextMatchKind::Contains,
            })?
            .map(|result| {
                (
                    result.bounds.x,
                    result.bounds.y,
                    result.bounds.width,
                    result.bounds.height,
                )
            }))
    }

    /// Find the first clickable semantic node whose subtree contains exactly matching text.
    pub fn find_button_bounds_exact(
        &self,
        text: &str,
    ) -> Result<Option<(f32, f32, f32, f32)>, String> {
        Ok(self
            .request_semantic_query(RobotCommand::FindButton {
                text: text.to_string(),
                match_kind: SemanticTextMatchKind::Exact,
            })?
            .map(|result| {
                (
                    result.bounds.x,
                    result.bounds.y,
                    result.bounds.width,
                    result.bounds.height,
                )
            }))
    }

    /// Capture a screenshot of the current render scene.
    pub fn screenshot(&self) -> Result<RobotScreenshot, String> {
        self.tx
            .send(RobotCommand::GetScreenshot)
            .map_err(|e| format!("Failed to send screenshot command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Screenshot(image)) => Ok(image),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Capture a screenshot at a specific device pixel scale (e.g., 2.0 for HiDPI).
    pub fn screenshot_with_scale(&self, scale: f32) -> Result<RobotScreenshot, String> {
        self.tx
            .send(RobotCommand::GetScreenshotWithScale(scale))
            .map_err(|e| format!("Failed to send screenshot command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Screenshot(image)) => Ok(image),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Get the most recent renderer frame stats, if available.
    pub fn get_render_stats(&self) -> Result<Option<RenderStatsSnapshot>, String> {
        self.tx
            .send(RobotCommand::GetRenderStats)
            .map_err(|e| format!("Failed to send render stats command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::RenderStats(stats)) => Ok(*stats),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Get a snapshot of CPU-side renderer allocation capacities.
    pub fn get_render_cpu_allocation_stats(&self) -> Result<DebugCpuAllocationStats, String> {
        self.tx
            .send(RobotCommand::GetRenderCpuAllocationStats)
            .map_err(|e| format!("Failed to send render CPU allocation stats command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::RenderCpuAllocationStats(stats)) => Ok(*stats),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Get a snapshot of runtime/applier allocation stats for leak diagnostics.
    pub fn get_runtime_leak_debug_stats(&self) -> Result<RuntimeLeakDebugStats, String> {
        self.tx
            .send(RobotCommand::GetRuntimeLeakDebugStats)
            .map_err(|e| format!("Failed to send runtime leak debug stats command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::RuntimeLeakDebugStats(stats)) => Ok(*stats),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Enable or disable eager semantics extraction for robot queries.
    pub fn set_semantics_enabled(&self, enabled: bool) -> Result<(), String> {
        self.tx
            .send(RobotCommand::SetSemanticsEnabled(enabled))
            .map_err(|e| format!("Failed to send semantics toggle command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Ok) => Ok(()),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Invoke an application-defined robot hook on the app thread.
    pub fn invoke_app_hook(&self, name: &str, argument: &str) -> Result<Option<String>, String> {
        self.tx
            .send(RobotCommand::InvokeAppHook {
                name: name.to_string(),
                argument: argument.to_string(),
            })
            .map_err(|e| format!("Failed to send app hook command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::AppHookResult(result)) => Ok(result),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
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
        let (x, y, w, h) = self
            .find_button_bounds(text)?
            .ok_or_else(|| format!("Button '{}' not found in semantic tree", text))?;
        let center_x = x + w / 2.0;
        let center_y = y + h / 2.0;

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
        if self.find_text_bounds(expected)?.is_some() {
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
    /// Last known cursor position in logical pixels
    last_cursor_position: Option<(f32, f32)>,
    /// Robot controller
    #[cfg(feature = "robot")]
    robot_controller: Option<RobotController>,
    /// Optional robot hook executed on the app thread for deterministic test control.
    #[cfg(feature = "robot")]
    robot_app_hook: Option<Box<crate::RobotAppHook>>,
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
        #[cfg(feature = "robot")]
        let robot_app_hook = settings.robot_app_hook.take();

        Self {
            settings,
            content: Some(Box::new(content)),
            window: None,
            surface: None,
            surface_config: None,
            app: None,
            platform: None,
            current_modifiers: winit::keyboard::ModifiersState::empty(),
            last_cursor_position: None,
            #[cfg(feature = "robot")]
            robot_controller: None,
            #[cfg(feature = "robot")]
            robot_app_hook,
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

        let window: Arc<dyn Window> = event_loop
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
            .expect("failed to create window")
            .into();

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
        let adapter_info = adapter.get_info();

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Main Device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
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
        let fonts: &[&[u8]] = self.settings.fonts.take().unwrap_or(&[]);
        let mut renderer = WgpuRenderer::new(fonts);
        renderer.init_gpu(
            Arc::new(device),
            Arc::new(queue),
            surface_format,
            adapter_info.backend,
        );
        let initial_scale = window.scale_factor();
        renderer.set_root_scale(initial_scale as f32);
        cranpose_ui::set_density(initial_scale as f32);

        // Take the content closure (can only be called once)
        let content = self.content.take().expect("content already taken");
        let mut app = AppShell::new(renderer, default_root_key(), content);
        #[cfg(feature = "robot")]
        app.set_semantics_enabled(self.robot_controller.is_some());

        // Apply dev options (FPS counter, etc.)
        app.set_dev_options(self.settings.dev_options.clone());

        // Runtime-driven frame scheduling: Compose invalidations and animations
        // request frames via the runtime waker, not per-input host redraw forcing.
        let frame_waker_window = window.clone();
        app.set_frame_waker(move || {
            frame_waker_window.request_redraw();
        });

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
            WindowEvent::SurfaceResized(new_size) if new_size.width > 0 && new_size.height > 0 => {
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
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                mut surface_size_writer,
            } => {
                platform.set_scale_factor(scale_factor);
                app.renderer().set_root_scale(scale_factor as f32);
                cranpose_ui::set_density(scale_factor as f32);

                let new_size = window.surface_size();
                let _ = surface_size_writer.request_surface_size(new_size);
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
            WindowEvent::PointerMoved { position, .. } => {
                let logical = platform.pointer_position(position);
                self.last_cursor_position = Some((logical.x, logical.y));
                if desktop_input_debug_enabled() {
                    eprintln!(
                        "[CRANPOSE_INPUT_DEBUG] desktop pointer move ({:.2},{:.2})",
                        logical.x, logical.y
                    );
                }
                app.set_cursor(logical.x, logical.y);
                if let Some(recorder) = &mut self.recorder {
                    recorder.record_mouse_move(logical.x, logical.y);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                // Track current keyboard modifiers for key events
                self.current_modifiers = modifiers.state();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some((x, y)) = self.last_cursor_position {
                    app.set_cursor(x, y);
                }

                let mut logical_delta = platform.scroll_delta(delta);
                let alt_pressed = self
                    .current_modifiers
                    .contains(winit::keyboard::ModifiersState::ALT);
                if alt_pressed {
                    if logical_delta.x.abs() <= f32::EPSILON {
                        logical_delta.x = logical_delta.y;
                    }
                    logical_delta.y = 0.0;
                }

                if desktop_input_debug_enabled() {
                    eprintln!(
                        "[CRANPOSE_INPUT_DEBUG] desktop wheel delta ({:.2},{:.2}) alt={}",
                        logical_delta.x, logical_delta.y, alt_pressed
                    );
                }

                app.pointer_scrolled(logical_delta.x, logical_delta.y);
            }
            WindowEvent::PointerButton {
                state,
                button: ButtonSource::Mouse(MouseButton::Left),
                ..
            } => {
                if let Some((x, y)) = self.last_cursor_position {
                    if desktop_input_debug_enabled() {
                        eprintln!(
                            "[CRANPOSE_INPUT_DEBUG] desktop pointer button {:?} at ({:.2},{:.2})",
                            state, x, y
                        );
                    }
                    app.set_cursor(x, y);
                }
                match state {
                    ElementState::Pressed => {
                        app.pointer_pressed();
                        if let Some(recorder) = &mut self.recorder {
                            recorder.record_mouse_down();
                        }
                    }
                    ElementState::Released => {
                        app.pointer_released();
                        app.sync_selection_to_primary();
                        if let Some(recorder) = &mut self.recorder {
                            recorder.record_mouse_up();
                        }
                    }
                }
            }
            // Middle-click paste from Linux primary selection
            WindowEvent::PointerButton {
                state: ElementState::Pressed,
                button: ButtonSource::Mouse(MouseButton::Middle),
                ..
            } => {
                if let Some((x, y)) = self.last_cursor_position {
                    app.set_cursor(x, y);
                }
                #[cfg(all(
                    not(target_arch = "wasm32"),
                    not(target_os = "android"),
                    not(target_os = "ios")
                ))]
                if let Some(text) = app.get_primary_selection() {
                    app.on_paste(&text);
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
                app.on_key_event(&key_event);
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
                        app.on_ime_preedit(&text, cursor);
                    }
                    Ime::Commit(text) => {
                        // IME finished - commit the final text
                        // First clear composition state, then insert the final text
                        let _ = app.on_ime_preedit("", None);
                        app.on_paste(&text);
                    }
                    Ime::Enabled => {
                        // IME was enabled - no action needed
                    }
                    Ime::Disabled => {
                        // IME was disabled - clear any composition state
                        app.on_ime_preedit("", None);
                    }
                    Ime::DeleteSurrounding { .. } => {
                        // IME asks to delete surrounding text; not yet handled by app shell.
                    }
                }
            }
            WindowEvent::PointerLeft { .. } => {
                app.cancel_gesture();
            }
            WindowEvent::RedrawRequested => {
                if desktop_input_debug_enabled() {
                    eprintln!("[CRANPOSE_INPUT_DEBUG] desktop redraw requested");
                }
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
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::MoveTo { x, y } => {
                        app.set_cursor(x, y);
                        // Record for robot test generation
                        if let Some(recorder) = &mut self.recorder {
                            recorder.record_mouse_move(x, y);
                        }
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
                    RobotCommand::MouseScroll { delta_x, delta_y } => {
                        app.pointer_scrolled(delta_x, delta_y);
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
                        pump_robot_frame(app);
                        let semantics = extract_semantics(app);
                        let _ = controller.tx.send(RobotResponse::Semantics(semantics));
                    }
                    RobotCommand::FindText { text, match_kind } => {
                        pump_robot_frame(app);
                        let result = find_text_in_app(app, &text, match_kind);
                        let _ = controller.tx.send(RobotResponse::SemanticQuery(result));
                    }
                    RobotCommand::FindButton { text, match_kind } => {
                        pump_robot_frame(app);
                        let result = find_button_in_app(app, &text, match_kind);
                        let _ = controller.tx.send(RobotResponse::SemanticQuery(result));
                    }
                    RobotCommand::GetScreenshot => {
                        pump_robot_frame(app);
                        match capture_screenshot(app) {
                            Ok(screenshot) => {
                                let _ = controller.tx.send(RobotResponse::Screenshot(screenshot));
                            }
                            Err(err) => {
                                let _ = controller.tx.send(RobotResponse::Error(err));
                            }
                        }
                    }
                    RobotCommand::GetScreenshotWithScale(scale) => {
                        pump_robot_frame(app);
                        match capture_screenshot_with_scale(app, scale) {
                            Ok(screenshot) => {
                                let _ = controller.tx.send(RobotResponse::Screenshot(screenshot));
                            }
                            Err(err) => {
                                let _ = controller.tx.send(RobotResponse::Error(err));
                            }
                        }
                    }
                    RobotCommand::GetRenderStats => {
                        let _ = controller.tx.send(RobotResponse::RenderStats(Box::new(
                            app.renderer().last_frame_stats(),
                        )));
                    }
                    RobotCommand::GetRenderCpuAllocationStats => {
                        let _ =
                            controller
                                .tx
                                .send(RobotResponse::RenderCpuAllocationStats(Box::new(
                                    app.renderer().debug_cpu_allocation_stats(),
                                )));
                    }
                    RobotCommand::GetRuntimeLeakDebugStats => {
                        let _ = controller
                            .tx
                            .send(RobotResponse::RuntimeLeakDebugStats(Box::new(
                                app.debug_runtime_leak_stats(),
                            )));
                    }
                    RobotCommand::SetSemanticsEnabled(enabled) => {
                        app.set_semantics_enabled(enabled);
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::InvokeAppHook { name, argument } => {
                        let response = match self.robot_app_hook.as_mut() {
                            Some(hook) => hook(name, argument).map(RobotResponse::AppHookResult),
                            None => Err("robot app hook not configured".to_string()),
                        };
                        match response {
                            Ok(response) => {
                                let _ = controller.tx.send(response);
                            }
                            Err(err) => {
                                let _ = controller.tx.send(RobotResponse::Error(err));
                            }
                        }
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

        let needs_redraw = app.needs_redraw();
        if desktop_input_debug_enabled() && needs_redraw {
            eprintln!(
                "[CRANPOSE_INPUT_DEBUG] about_to_wait needs_redraw={}",
                needs_redraw
            );
        }
        if needs_redraw {
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

#[cfg(feature = "robot")]
fn capture_screenshot(app: &mut AppShell<WgpuRenderer>) -> Result<RobotScreenshot, String> {
    let logical_size = app.root_layout_size();
    let (width, height, capture_scale) =
        resolve_robot_screenshot_params(app.buffer_size(), logical_size);

    let captured = app
        .renderer()
        .capture_frame_with_scale(width, height, capture_scale)
        .map_err(|err| format!("Failed to capture GPU screenshot: {err:?}"))?;

    let (logical_width, logical_height) = logical_size.unwrap_or_else(|| {
        let fallback_scale = if capture_scale.is_finite() && capture_scale > 0.0 {
            capture_scale
        } else {
            1.0
        };
        (
            (captured.width as f32 / fallback_scale).max(1.0),
            (captured.height as f32 / fallback_scale).max(1.0),
        )
    });

    Ok(RobotScreenshot {
        width: captured.width,
        height: captured.height,
        logical_width,
        logical_height,
        pixels: captured.pixels,
    })
}

#[cfg(feature = "robot")]
fn capture_screenshot_with_scale(
    app: &mut AppShell<WgpuRenderer>,
    scale: f32,
) -> Result<RobotScreenshot, String> {
    let logical_size = app.root_layout_size();
    let (logical_width, logical_height) = logical_size.unwrap_or((1.0, 1.0));
    let width = (logical_width * scale).ceil() as u32;
    let height = (logical_height * scale).ceil() as u32;

    let captured = app
        .renderer()
        .capture_frame_with_scale(width, height, scale)
        .map_err(|err| format!("Failed to capture GPU screenshot: {err:?}"))?;

    Ok(RobotScreenshot {
        width: captured.width,
        height: captured.height,
        logical_width,
        logical_height,
        pixels: captured.pixels,
    })
}

#[cfg(feature = "robot")]
fn resolve_robot_screenshot_params(
    buffer_size: (u32, u32),
    fallback_logical_size: Option<(f32, f32)>,
) -> (u32, u32, f32) {
    if let Some((logical_width, logical_height)) = fallback_logical_size {
        let width = logical_width.ceil().max(1.0) as u32;
        let height = logical_height.ceil().max(1.0) as u32;
        return (width, height, 1.0);
    }

    let (buffer_width, buffer_height) = buffer_size;
    (buffer_width.max(1), buffer_height.max(1), 1.0)
}

/// Extract semantic elements by combining semantic tree with an on-demand layout snapshot.
#[cfg(feature = "robot")]
fn extract_semantics(app: &mut AppShell<WgpuRenderer>) -> Vec<SemanticElement> {
    let Some(layout_tree) = app.layout_tree().cloned() else {
        return Vec::new();
    };
    let Some(semantic_root) = app.semantics_tree().map(|tree| tree.root().clone()) else {
        return Vec::new();
    };
    let bounds_by_node = build_semantic_bounds_index(layout_tree.root());
    let mut bounds_for = |node_id| semantic_rect_for_node(&bounds_by_node, node_id);
    vec![semantic_element_from_semantics_node(
        &semantic_root,
        &mut bounds_for,
    )]
}

#[cfg(feature = "robot")]
fn semantic_element_from_semantics_node<F>(
    sem_node: &SemanticsNode,
    bounds_for: &mut F,
) -> SemanticElement
where
    F: FnMut(cranpose_core::NodeId) -> SemanticRect,
{
    let role = match &sem_node.role {
        SemanticsRole::Button => "Button",
        SemanticsRole::Text { .. } => "Text",
        SemanticsRole::Layout => "Layout",
        SemanticsRole::Subcompose => "Subcompose",
        SemanticsRole::Spacer => "Spacer",
        SemanticsRole::Unknown => "Unknown",
    }
    .to_string();

    let text = match &sem_node.role {
        SemanticsRole::Text { value } => Some(value.clone()),
        _ => sem_node.description.clone(),
    };

    let clickable = sem_node
        .actions
        .iter()
        .any(|action| matches!(action, SemanticsAction::Click { .. }));
    let bounds = bounds_for(sem_node.node_id);
    let children = sem_node
        .children
        .iter()
        .map(|child| semantic_element_from_semantics_node(child, bounds_for))
        .collect();

    SemanticElement {
        role,
        text,
        bounds,
        clickable,
        children,
    }
}

#[cfg(feature = "robot")]
fn find_text_in_app(
    app: &mut AppShell<WgpuRenderer>,
    query: &str,
    match_kind: SemanticTextMatchKind,
) -> Option<SemanticQueryResult> {
    let layout_tree = app.layout_tree()?.clone();
    let root = app.semantics_tree()?.root().clone();
    let bounds_by_node = build_semantic_bounds_index(layout_tree.root());
    let result = find_text_in_semantics_tree(&bounds_by_node, &root, query, match_kind);
    if desktop_input_debug_enabled() {
        eprintln!(
            "[CRANPOSE_INPUT_DEBUG] find_text query={query:?} result={:?}",
            result
                .as_ref()
                .map(|result| (result.node_id, result.bounds, result.text.clone()))
        );
    }
    result
}

#[cfg(feature = "robot")]
fn find_button_in_app(
    app: &mut AppShell<WgpuRenderer>,
    query: &str,
    match_kind: SemanticTextMatchKind,
) -> Option<SemanticQueryResult> {
    let layout_tree = app.layout_tree()?.clone();
    let root = app.semantics_tree()?.root().clone();
    let bounds_by_node = build_semantic_bounds_index(layout_tree.root());
    let result = find_button_in_semantics_tree(&bounds_by_node, &root, query, match_kind);
    if desktop_input_debug_enabled() {
        eprintln!(
            "[CRANPOSE_INPUT_DEBUG] find_button query={query:?} result={:?}",
            result
                .as_ref()
                .map(|result| (result.node_id, result.bounds, result.text.clone()))
        );
    }
    result
}

#[cfg(feature = "robot")]
fn semantic_rect_for_node(
    bounds_by_node: &HashMap<cranpose_core::NodeId, SemanticRect>,
    node_id: cranpose_core::NodeId,
) -> SemanticRect {
    bounds_by_node
        .get(&node_id)
        .copied()
        .unwrap_or(SemanticRect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        })
}

#[cfg(feature = "robot")]
fn find_text_in_semantics_tree(
    bounds_by_node: &HashMap<cranpose_core::NodeId, SemanticRect>,
    sem_node: &SemanticsNode,
    query: &str,
    match_kind: SemanticTextMatchKind,
) -> Option<SemanticQueryResult> {
    if let Some(text) = semantics_node_text(sem_node) {
        if semantics_text_matches(text, query, match_kind) {
            return Some(SemanticQueryResult {
                node_id: sem_node.node_id,
                bounds: semantic_rect_for_node(bounds_by_node, sem_node.node_id),
                text: Some(text.to_string()),
            });
        }
    }

    for child in &sem_node.children {
        if let Some(result) = find_text_in_semantics_tree(bounds_by_node, child, query, match_kind)
        {
            return Some(result);
        }
    }

    None
}

#[cfg(feature = "robot")]
fn find_button_in_semantics_tree(
    bounds_by_node: &HashMap<cranpose_core::NodeId, SemanticRect>,
    sem_node: &SemanticsNode,
    query: &str,
    match_kind: SemanticTextMatchKind,
) -> Option<SemanticQueryResult> {
    if semantics_node_clickable(sem_node)
        && subtree_contains_matching_text(sem_node, query, match_kind)
    {
        return Some(SemanticQueryResult {
            node_id: sem_node.node_id,
            bounds: semantic_rect_for_node(bounds_by_node, sem_node.node_id),
            text: semantics_node_text(sem_node).map(str::to_string),
        });
    }

    for child in &sem_node.children {
        if let Some(result) =
            find_button_in_semantics_tree(bounds_by_node, child, query, match_kind)
        {
            return Some(result);
        }
    }

    None
}

#[cfg(feature = "robot")]
fn semantics_text_matches(actual: &str, query: &str, match_kind: SemanticTextMatchKind) -> bool {
    match match_kind {
        SemanticTextMatchKind::Contains => actual.contains(query),
        SemanticTextMatchKind::Exact => actual == query,
        SemanticTextMatchKind::Prefix => actual.starts_with(query),
    }
}

#[cfg(feature = "robot")]
fn semantics_node_text(sem_node: &SemanticsNode) -> Option<&str> {
    match &sem_node.role {
        SemanticsRole::Text { value } => Some(value.as_str()),
        _ => sem_node.description.as_deref(),
    }
}

#[cfg(feature = "robot")]
fn semantics_node_clickable(sem_node: &SemanticsNode) -> bool {
    sem_node
        .actions
        .iter()
        .any(|action| matches!(action, SemanticsAction::Click { .. }))
}

#[cfg(feature = "robot")]
fn build_semantic_bounds_index(
    root: &cranpose_ui::LayoutBox,
) -> HashMap<cranpose_core::NodeId, SemanticRect> {
    let mut bounds = HashMap::new();
    collect_semantic_bounds(root, &mut bounds);
    bounds
}

#[cfg(feature = "robot")]
fn collect_semantic_bounds(
    layout_box: &cranpose_ui::LayoutBox,
    bounds: &mut HashMap<cranpose_core::NodeId, SemanticRect>,
) {
    bounds.insert(layout_box.node_id, bounds_from_layout_box(layout_box));
    for child in &layout_box.children {
        collect_semantic_bounds(child, bounds);
    }
}

#[cfg(feature = "robot")]
fn bounds_from_layout_box(layout_box: &cranpose_ui::LayoutBox) -> SemanticRect {
    SemanticRect {
        x: layout_box.rect.x,
        y: layout_box.rect.y,
        width: layout_box.rect.width,
        height: layout_box.rect.height,
    }
}

#[cfg(all(feature = "robot", test))]
fn find_text_in_trees(
    sem_node: &SemanticsNode,
    layout_box: &cranpose_ui::LayoutBox,
    query: &str,
    match_kind: SemanticTextMatchKind,
) -> Option<SemanticQueryResult> {
    if let Some(text) = semantics_node_text(sem_node) {
        if semantics_text_matches(text, query, match_kind) {
            return Some(SemanticQueryResult {
                node_id: layout_box.node_id,
                bounds: bounds_from_layout_box(layout_box),
                text: Some(text.to_string()),
            });
        }
    }

    sem_node
        .children
        .iter()
        .zip(layout_box.children.iter())
        .find_map(|(sem_child, layout_child)| {
            find_text_in_trees(sem_child, layout_child, query, match_kind)
        })
}

#[cfg(feature = "robot")]
fn subtree_contains_matching_text(
    sem_node: &SemanticsNode,
    query: &str,
    match_kind: SemanticTextMatchKind,
) -> bool {
    if let Some(text) = semantics_node_text(sem_node) {
        if semantics_text_matches(text, query, match_kind) {
            return true;
        }
    }

    sem_node
        .children
        .iter()
        .any(|child| subtree_contains_matching_text(child, query, match_kind))
}

#[cfg(all(feature = "robot", test))]
fn find_button_in_trees(
    sem_node: &SemanticsNode,
    layout_box: &cranpose_ui::LayoutBox,
    query: &str,
    match_kind: SemanticTextMatchKind,
) -> Option<SemanticQueryResult> {
    if semantics_node_clickable(sem_node)
        && subtree_contains_matching_text(sem_node, query, match_kind)
    {
        return Some(SemanticQueryResult {
            node_id: layout_box.node_id,
            bounds: bounds_from_layout_box(layout_box),
            text: semantics_node_text(sem_node).map(str::to_string),
        });
    }

    sem_node
        .children
        .iter()
        .zip(layout_box.children.iter())
        .find_map(|(sem_child, layout_child)| {
            find_button_in_trees(sem_child, layout_child, query, match_kind)
        })
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

#[cfg(test)]
mod tests {
    #[cfg(feature = "robot")]
    use super::{
        find_button_in_trees, find_text_in_trees, resolve_robot_screenshot_params,
        semantic_element_from_semantics_node, subtree_contains_matching_text, SemanticRect,
        SemanticTextMatchKind,
    };
    #[cfg(feature = "robot")]
    use cranpose_core::NodeId;
    #[cfg(feature = "robot")]
    use cranpose_ui::{
        LayoutBox, LayoutNodeData, LayoutNodeKind, Modifier, ModifierNodeSlices, Point, Rect,
        ResolvedModifiers, SemanticsAction, SemanticsCallback, SemanticsNode, SemanticsRole,
    };
    #[cfg(feature = "robot")]
    use std::rc::Rc;

    #[cfg(feature = "robot")]
    #[test]
    fn robot_screenshot_prefers_logical_layout_size() {
        let resolved = resolve_robot_screenshot_params((1600, 1200), Some((800.0, 600.0)));
        assert_eq!(resolved, (800, 600, 1.0));
    }

    #[cfg(feature = "robot")]
    #[test]
    fn robot_screenshot_uses_ceil_on_fractional_logical_size() {
        let resolved = resolve_robot_screenshot_params((0, 0), Some((801.2, 601.3)));
        assert_eq!(resolved, (802, 602, 1.0));
    }

    #[cfg(feature = "robot")]
    #[test]
    fn robot_screenshot_falls_back_to_physical_buffer_when_layout_is_missing() {
        let resolved = resolve_robot_screenshot_params((1600, 1200), None);
        assert_eq!(resolved, (1600, 1200, 1.0));
    }

    #[cfg(feature = "robot")]
    #[test]
    fn robot_screenshot_clamps_to_non_zero_target() {
        let resolved = resolve_robot_screenshot_params((0, 0), Some((10.0, 20.0)));
        assert_eq!(resolved, (10, 20, 1.0));
    }

    #[cfg(feature = "robot")]
    fn sample_layout_box(
        node_id: u64,
        rect: (f32, f32, f32, f32),
        children: Vec<LayoutBox>,
    ) -> LayoutBox {
        LayoutBox::new(
            node_id as NodeId,
            Rect {
                x: rect.0,
                y: rect.1,
                width: rect.2,
                height: rect.3,
            },
            Point { x: 0.0, y: 0.0 },
            LayoutNodeData::new(
                Modifier::empty(),
                ResolvedModifiers::default(),
                Rc::new(ModifierNodeSlices::default()),
                LayoutNodeKind::Spacer,
            ),
            children,
        )
    }

    #[cfg(feature = "robot")]
    fn sample_semantics_node(
        node_id: u64,
        role: SemanticsRole,
        clickable: bool,
        description: Option<&str>,
        children: Vec<SemanticsNode>,
    ) -> SemanticsNode {
        let mut actions = Vec::new();
        if clickable {
            actions.push(SemanticsAction::Click {
                handler: SemanticsCallback::new(node_id as NodeId),
            });
        }
        SemanticsNode {
            node_id: node_id as NodeId,
            role,
            actions,
            children,
            description: description.map(str::to_string),
        }
    }

    #[cfg(feature = "robot")]
    fn sample_semantics_and_layout() -> (SemanticsNode, LayoutBox) {
        let button_label = sample_semantics_node(
            3,
            SemanticsRole::Text {
                value: "Increase depth".to_string(),
            },
            false,
            None,
            Vec::new(),
        );
        let depth_label = sample_semantics_node(
            4,
            SemanticsRole::Text {
                value: "Current depth: 15".to_string(),
            },
            false,
            None,
            Vec::new(),
        );
        let root = sample_semantics_node(
            1,
            SemanticsRole::Layout,
            false,
            Some("Root"),
            vec![
                sample_semantics_node(2, SemanticsRole::Button, true, None, vec![button_label]),
                depth_label,
            ],
        );
        let layout = sample_layout_box(
            1,
            (0.0, 0.0, 100.0, 100.0),
            vec![
                sample_layout_box(
                    2,
                    (10.0, 10.0, 40.0, 20.0),
                    vec![sample_layout_box(3, (12.0, 12.0, 36.0, 12.0), Vec::new())],
                ),
                sample_layout_box(4, (10.0, 40.0, 60.0, 12.0), Vec::new()),
            ],
        );
        (root, layout)
    }

    #[cfg(feature = "robot")]
    #[test]
    fn robot_text_query_finds_prefix_without_building_snapshot() {
        let (semantics, layout) = sample_semantics_and_layout();
        let result = find_text_in_trees(
            &semantics,
            &layout,
            "Current depth:",
            SemanticTextMatchKind::Prefix,
        )
        .expect("prefix match");

        assert_eq!(result.text.as_deref(), Some("Current depth: 15"));
        assert_eq!(result.bounds.x, 10.0);
        assert_eq!(result.bounds.y, 40.0);
    }

    #[cfg(feature = "robot")]
    #[test]
    fn robot_button_query_matches_descendant_text() {
        let (semantics, layout) = sample_semantics_and_layout();
        let result = find_button_in_trees(
            &semantics,
            &layout,
            "Increase depth",
            SemanticTextMatchKind::Exact,
        )
        .expect("button match");

        assert_eq!(result.bounds.width, 40.0);
        assert_eq!(result.bounds.height, 20.0);
    }

    #[cfg(feature = "robot")]
    #[test]
    fn robot_subtree_text_match_honors_exact_mode() {
        let (semantics, _) = sample_semantics_and_layout();

        assert!(subtree_contains_matching_text(
            &semantics,
            "Current depth: 15",
            SemanticTextMatchKind::Exact,
        ));
        assert!(!subtree_contains_matching_text(
            &semantics,
            "Current depth:",
            SemanticTextMatchKind::Exact,
        ));
    }

    #[cfg(feature = "robot")]
    #[test]
    fn robot_semantics_export_uses_node_ids_for_bounds() {
        let (semantics, _) = sample_semantics_and_layout();
        let mut bounds_for = |node_id: NodeId| SemanticRect {
            x: node_id as f32,
            y: node_id as f32 * 2.0,
            width: 10.0,
            height: 5.0,
        };

        let exported = semantic_element_from_semantics_node(&semantics, &mut bounds_for);

        assert_eq!(exported.bounds.x, 1.0);
        assert_eq!(exported.children.len(), 2);
        assert_eq!(exported.children[0].bounds.x, 2.0);
        assert_eq!(exported.children[0].children[0].bounds.x, 3.0);
        assert_eq!(
            exported.children[1].text.as_deref(),
            Some("Current depth: 15")
        );
    }
}
