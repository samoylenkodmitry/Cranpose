//! Desktop runtime for Compose applications.
//!
//! This module provides the desktop event loop implementation using winit.

use crate::launcher::{AppSettings, LaunchError};
use crate::native_window::{
    self, NativeWindowEvents, NativeWindowKey, NativeWindowOptions, NativeWindowPositionOrigin,
    NativeWindowRequest, WindowGraphMove, WindowGraphNodeSnapshot, WindowGraphPeerSnapshot,
    WindowGraphState, WindowGroupId, WindowResizeDirection, WindowState,
};
use crate::wgpu_surface::{current_surface_texture, SurfaceFrame};
#[cfg(feature = "robot")]
use cranpose_app_shell::RuntimeLeakDebugStats;
use cranpose_app_shell::{default_root_key, AppShell, FramePacingMode};
use cranpose_platform_desktop_winit::DesktopWinitPlatform;
#[cfg(feature = "robot")]
use cranpose_render_wgpu::{DebugCpuAllocationStats, RenderStatsSnapshot};
use cranpose_render_wgpu::{WgpuRenderer, WgpuTextSystem};
#[cfg(feature = "robot")]
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize, Position};
use winit::event::{ButtonSource, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{
    ResizeDirection, Window, WindowAttributes, WindowId as WinitWindowId, WindowLevel,
};

const NATIVE_WINDOW_DRAG_POLL_INTERVAL: Duration = Duration::from_millis(16);
const NATIVE_WINDOW_POSITION_POLL_INTERVAL: Duration = Duration::from_millis(16);
const NATIVE_WINDOW_PLACEMENT_MARGIN: f32 = 32.0;
#[cfg(feature = "robot")]
const ROBOT_PUMP_FRAME_INTERVAL: Duration = Duration::from_nanos(16_666_667);

#[cfg(feature = "robot")]
use cranpose_ui::{SemanticsAction, SemanticsNode, SemanticsRole};

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
    for _ in 0..3 {
        if !app.needs_redraw() && !app.has_active_animations() {
            break;
        }
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
    PumpFrames {
        count: u32,
    },
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
    DriverPanicked(String),
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

    /// Run a bounded number of frame updates.
    ///
    /// This is intended for robot assertions around live animation, where
    /// `wait_for_idle` is not meaningful because the application is expected to
    /// keep producing frames.
    pub fn pump_frames(&self, count: u32) -> Result<(), String> {
        self.tx
            .send(RobotCommand::PumpFrames { count })
            .map_err(|e| format!("Failed to send pump_frames command: {}", e))?;
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
        let report = Self::format_semantics(elements, indent);
        log::info!(target: "cranpose::robot::semantics", "\n{report}");
    }

    /// Format the semantic tree as a plain-text hierarchy for caller-controlled output.
    pub fn format_semantics(elements: &[SemanticElement], indent: usize) -> String {
        fn format_semantics_into(output: &mut String, elements: &[SemanticElement], indent: usize) {
            for elem in elements {
                let prefix = "  ".repeat(indent);
                let text_info = elem
                    .text
                    .as_ref()
                    .map(|t| format!(" text=\"{}\"", t))
                    .unwrap_or_default();
                let clickable = if elem.clickable { " [CLICKABLE]" } else { "" };
                let _ = std::fmt::Write::write_fmt(
                    output,
                    format_args!("{prefix}role={}{}{}\n", elem.role, text_info, clickable),
                );
                format_semantics_into(output, &elem.children, indent + 1);
            }
        }

        let mut output = String::new();
        format_semantics_into(&mut output, elements, indent);
        output
    }
}

/// Application state that implements winit's ApplicationHandler
struct DesktopGpuContext {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    adapter_backend: wgpu::Backend,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    text_system: WgpuTextSystem,
}

struct NativeWindowSurface {
    key: NativeWindowKey,
    revision: u64,
    options: NativeWindowOptions,
    events: NativeWindowEvents,
    state: Option<WindowState>,
    group: Option<native_window::NativeWindowGroupMembership>,
    window: Arc<dyn Window>,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    surface_caps: wgpu::SurfaceCapabilities,
    app: AppShell<WgpuRenderer>,
    platform: DesktopWinitPlatform,
    last_cursor_position: Option<(f32, f32)>,
    last_cursor_physical_position: Option<PhysicalPosition<f64>>,
    frame_pacing_mode: FramePacingMode,
    last_frame_start_time: Option<Instant>,
    vsync_interval: Duration,
    pending_outer_positions: PendingNativeWindowPositions,
    active_drag: Option<NativeWindowDragSession>,
}

struct NativeWindowShell {
    request: NativeWindowRequest,
    window: Arc<dyn Window>,
    create_started: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct DesktopRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl DesktopRect {
    fn right(self) -> f32 {
        self.x + self.width
    }

    fn bottom(self) -> f32 {
        self.y + self.height
    }

    fn center(self) -> cranpose_ui::Point {
        cranpose_ui::Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    fn contains_rect_with_margin(self, rect: Self, margin: f32) -> bool {
        rect.x >= self.x + margin
            && rect.right() <= self.right() - margin
            && rect.y >= self.y + margin
            && rect.bottom() <= self.bottom() - margin
    }

    fn distance_to_point(self, point: cranpose_ui::Point) -> f32 {
        let dx = if point.x < self.x {
            self.x - point.x
        } else if point.x > self.right() {
            point.x - self.right()
        } else {
            0.0
        };
        let dy = if point.y < self.y {
            self.y - point.y
        } else if point.y > self.bottom() {
            point.y - self.bottom()
        } else {
            0.0
        };
        dx * dx + dy * dy
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum NativeWindowPlacementGroupKey {
    Group(WindowGroupId),
    Window(NativeWindowKey),
}

#[derive(Clone, Copy, Debug)]
enum NativeWindowDragSession {
    Platform { next_poll_at: Instant },
    Polling(NativeWindowPollingDragSession),
}

impl NativeWindowDragSession {
    fn platform(now: Instant) -> Self {
        Self::Platform {
            next_poll_at: now + NATIVE_WINDOW_DRAG_POLL_INTERVAL,
        }
    }

    fn next_poll_at(self) -> Instant {
        match self {
            Self::Platform { next_poll_at } => next_poll_at,
            Self::Polling(session) => session.next_poll_at,
        }
    }

    fn set_next_poll_at(&mut self, next_poll_at: Instant) {
        match self {
            Self::Platform {
                next_poll_at: current,
                ..
            }
            | Self::Polling(NativeWindowPollingDragSession {
                next_poll_at: current,
                ..
            }) => {
                *current = next_poll_at;
            }
        }
    }

    fn polling_mut(&mut self) -> Option<&mut NativeWindowPollingDragSession> {
        match self {
            Self::Platform { .. } => None,
            Self::Polling(session) => Some(session),
        }
    }

    fn finishes_on_global_pointer_release(self) -> bool {
        matches!(self, Self::Platform { .. })
    }
}

#[derive(Clone, Copy, Debug)]
struct NativeWindowPollingDragSession {
    start_pointer_screen: PhysicalPosition<f64>,
    start_window_outer: PhysicalPosition<i32>,
    last_target_outer: PhysicalPosition<i32>,
    next_poll_at: Instant,
}

impl NativeWindowPollingDragSession {
    fn new(
        start_pointer_screen: PhysicalPosition<f64>,
        start_window_outer: PhysicalPosition<i32>,
        now: Instant,
    ) -> Self {
        Self {
            start_pointer_screen,
            start_window_outer,
            last_target_outer: start_window_outer,
            next_poll_at: now + NATIVE_WINDOW_DRAG_POLL_INTERVAL,
        }
    }

    fn target_for_pointer(&self, pointer: PhysicalPosition<f64>) -> PhysicalPosition<i32> {
        PhysicalPosition::new(
            (self.start_window_outer.x as f64 + pointer.x - self.start_pointer_screen.x).round()
                as i32,
            (self.start_window_outer.y as f64 + pointer.y - self.start_pointer_screen.y).round()
                as i32,
        )
    }
}

#[derive(Default)]
struct PendingNativeWindowPositions {
    positions: VecDeque<(f32, f32)>,
}

#[derive(Clone, Copy)]
enum NativeWindowGraphPositionSource {
    CachedThenCurrent,
    CurrentThenCached,
}

impl PendingNativeWindowPositions {
    fn push(&mut self, position: (f32, f32)) {
        if self
            .positions
            .back()
            .is_some_and(|pending| native_window_positions_close(*pending, position))
        {
            return;
        }
        self.positions.push_back(position);
        while self.positions.len() > 16 {
            self.positions.pop_front();
        }
    }

    fn acknowledge(&mut self, position: (f32, f32)) -> bool {
        let Some(index) = self
            .positions
            .iter()
            .position(|pending| native_window_positions_close(*pending, position))
        else {
            return false;
        };
        for _ in 0..=index {
            self.positions.pop_front();
        }
        true
    }

    fn clear(&mut self) {
        self.positions.clear();
    }

    fn has_pending(&self) -> bool {
        !self.positions.is_empty()
    }
}

impl NativeWindowSurface {
    fn frame_interval(&self) -> Option<Duration> {
        frame_interval_for_mode(self.frame_pacing_mode, self.vsync_interval)
    }
}

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
    /// Surface capabilities used when switching present modes at runtime.
    surface_caps: Option<wgpu::SurfaceCapabilities>,
    /// Compose app shell
    app: Option<AppShell<WgpuRenderer>>,
    /// Platform adapter
    platform: Option<DesktopWinitPlatform>,
    /// Shared GPU objects used by all desktop surfaces.
    gpu_context: Option<DesktopGpuContext>,
    /// Native sub-window surfaces keyed by the operating-system window id.
    native_windows: HashMap<WinitWindowId, NativeWindowSurface>,
    /// Declarative native sub-window ids mapped to their current OS window id.
    native_window_ids: HashMap<NativeWindowKey, WinitWindowId>,
    /// Last observed OS positions for declarative native sub-windows.
    native_window_positions: HashMap<NativeWindowKey, (f32, f32)>,
    /// Native sub-windows closed by the user while still declared by composition.
    closed_native_windows: HashSet<NativeWindowKey>,
    /// Framework-owned peer-window topology and drag sessions.
    window_graph: WindowGraphState,
    next_native_window_position_poll_at: Instant,
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
    /// Launch failure captured during window/GPU initialization.
    launch_error: Rc<RefCell<Option<LaunchError>>>,
    /// Event-loop wake path used when the primary declaration host is hidden.
    event_proxy: EventLoopProxy,
    frame_pacing_mode: FramePacingMode,
    last_frame_start_time: Option<Instant>,
    vsync_interval: Duration,
}

impl App {
    fn new(
        mut settings: AppSettings,
        content: impl FnMut() + 'static,
        launch_error: Rc<RefCell<Option<LaunchError>>>,
        event_proxy: EventLoopProxy,
    ) -> Self {
        // Create recorder if recording is enabled
        let recorder = settings
            .record_to
            .take()
            .map(crate::recorder::InputRecorder::new);
        #[cfg(feature = "robot")]
        let robot_app_hook = settings.robot_app_hook.take();
        let frame_pacing_mode = settings.frame_pacing_mode;

        Self {
            settings,
            content: Some(Box::new(content)),
            window: None,
            surface: None,
            surface_config: None,
            surface_caps: None,
            app: None,
            platform: None,
            gpu_context: None,
            native_windows: HashMap::new(),
            native_window_ids: HashMap::new(),
            native_window_positions: HashMap::new(),
            closed_native_windows: HashSet::new(),
            window_graph: WindowGraphState::default(),
            next_native_window_position_poll_at: Instant::now()
                + NATIVE_WINDOW_POSITION_POLL_INTERVAL,
            current_modifiers: winit::keyboard::ModifiersState::empty(),
            last_cursor_position: None,
            #[cfg(feature = "robot")]
            robot_controller: None,
            #[cfg(feature = "robot")]
            robot_app_hook,
            recorder,
            launch_error,
            event_proxy,
            frame_pacing_mode,
            last_frame_start_time: None,
            vsync_interval: default_vsync_interval(),
        }
    }

    fn abort_launch(&self, event_loop: &dyn ActiveEventLoop, error: LaunchError) {
        let mut slot = self.launch_error.borrow_mut();
        if slot.is_none() {
            *slot = Some(error);
        }
        event_loop.exit();
    }

    fn frame_interval(&self) -> Option<Duration> {
        frame_interval_for_mode(self.frame_pacing_mode, self.vsync_interval)
    }

    fn refresh_native_window_requests(&mut self) {
        if let Some(app) = &mut self.app {
            app.update();
        }
    }

    fn refresh_and_sync_native_windows(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.refresh_native_window_requests();
        self.sync_native_windows(event_loop);
    }

    fn handle_primary_frame_requested(&mut self, event_loop: &dyn ActiveEventLoop) {
        let direct_declaration_update = {
            let Some(app) = &mut self.app else {
                return;
            };
            let needs_redraw = app.needs_redraw() || app.has_active_animations();
            if !needs_redraw {
                return;
            }
            let direct_declaration_update = primary_declaration_host_needs_direct_update(
                self.settings.primary_window_visible,
                self.settings.headless,
                needs_redraw,
                false,
            );
            if direct_declaration_update {
                trace_native_window(format_args!(
                    "primary declaration host proxy update visible={} headless={}",
                    self.settings.primary_window_visible, self.settings.headless
                ));
                app.update();
            }
            direct_declaration_update
        };

        if direct_declaration_update {
            self.sync_native_windows(event_loop);
        } else if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn sync_native_windows(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.gpu_context.is_none() {
            return;
        }

        let has_requests = native_window::has_native_window_requests();
        if self.native_windows.is_empty() && self.closed_native_windows.is_empty() && !has_requests
        {
            return;
        }
        if !has_requests
            && self.closed_native_windows.is_empty()
            && self
                .native_windows
                .values()
                .all(|native| !native.options.visible)
        {
            return;
        }

        let sync_started = Instant::now();
        let requests = native_window::native_window_requests();
        let active_keys: HashSet<NativeWindowKey> =
            requests.iter().map(|request| request.key).collect();
        trace_native_window_timing(format_args!(
            "sync start requests={} existing={}",
            requests.len(),
            self.native_windows.len()
        ));

        self.closed_native_windows
            .retain(|key| active_keys.contains(key));

        let stale_window_ids: Vec<WinitWindowId> = self
            .native_windows
            .iter()
            .filter_map(|(window_id, native)| {
                (!active_keys.contains(&native.key)).then_some(*window_id)
            })
            .collect();
        for window_id in stale_window_ids {
            if let Some(native) = self.native_windows.get(&window_id) {
                trace_native_window(format_args!(
                    "sync stale key={:?} title={:?} visible={}",
                    native.key, native.options.title, native.options.visible
                ));
                if let Some((x, y)) = current_native_window_position(native) {
                    self.native_window_positions.insert(native.key, (x, y));
                    notify_native_window_moved(&native.events, x, y);
                }
            }
            if let Some(native) = self.native_windows.get_mut(&window_id) {
                if native.options.visible {
                    native.window.set_visible(false);
                    native.options.visible = false;
                    cancel_app_input(&mut native.app);
                }
            }
        }

        let mut native_windows_to_create = Vec::new();
        for request in requests {
            if self.closed_native_windows.contains(&request.key) {
                trace_native_window(format_args!(
                    "sync skip closed key={:?} title={:?}",
                    request.key, request.options.title
                ));
                continue;
            }

            let request = self.native_window_request_for_host(&request);
            if let Some(window_id) = self.native_window_ids.get(&request.key).copied() {
                if let Some(native) = self.native_windows.get_mut(&window_id) {
                    native.events = request.events.clone();
                    native.state = request.state;
                    native.group = request.group.clone();
                    let revision_changed = native.revision != request.revision;
                    let options_changed = native.options != request.options;
                    Self::apply_native_window_options(
                        native,
                        &request.options,
                        self.settings.headless,
                    );
                    if revision_changed {
                        native.revision = request.revision;
                        trace_native_window(format_args!(
                            "sync update content key={:?} title={:?}",
                            native.key, native.options.title
                        ));
                        native.app.request_root_render();
                        native.window.request_redraw();
                    } else if options_changed && request.options.visible {
                        trace_native_window(format_args!(
                            "sync update options key={:?} title={:?} visible={}",
                            native.key, native.options.title, request.options.visible
                        ));
                        native.window.request_redraw();
                    }
                    continue;
                }
                self.native_window_ids.remove(&request.key);
            }

            native_windows_to_create.push(request);
        }

        self.place_initial_native_windows_on_visible_monitors(
            event_loop,
            &mut native_windows_to_create,
        );

        let mut native_window_shells = Vec::with_capacity(native_windows_to_create.len());
        for request in native_windows_to_create {
            trace_native_window(format_args!(
                "sync create key={:?} title={:?} visible={}",
                request.key, request.options.title, request.options.visible
            ));
            match Self::create_native_window_shell(event_loop, request, self.settings.headless) {
                Ok(shell) => native_window_shells.push(shell),
                Err(error) => {
                    self.abort_launch(event_loop, error);
                    return;
                }
            }
        }

        for shell in native_window_shells {
            match self.create_native_window(shell) {
                Ok(native) => {
                    let window_id = native.window.id();
                    self.remember_native_window_position(&native);
                    self.native_window_ids.insert(native.key, window_id);
                    self.native_windows.insert(window_id, native);
                }
                Err(error) => {
                    self.abort_launch(event_loop, error);
                    return;
                }
            }
        }
        trace_native_window_timing(format_args!(
            "sync done in {}ms",
            sync_started.elapsed().as_millis()
        ));
        if self.hidden_bootstrap_has_no_visible_peer_windows() {
            event_loop.exit();
        }
    }

    fn hidden_bootstrap_has_no_visible_peer_windows(&self) -> bool {
        if self.settings.primary_window_visible || self.settings.headless {
            return false;
        }
        if self
            .native_windows
            .values()
            .any(|native| native.options.visible)
        {
            return false;
        }

        let requests = native_window::native_window_requests();
        requests.is_empty()
            || requests
                .iter()
                .all(|request| self.closed_native_windows.contains(&request.key))
    }

    fn place_initial_native_windows_on_visible_monitors(
        &self,
        event_loop: &dyn ActiveEventLoop,
        requests: &mut [NativeWindowRequest],
    ) {
        let monitors = logical_monitor_rects(event_loop);
        if monitors.is_empty() {
            return;
        }

        let mut groups: HashMap<NativeWindowPlacementGroupKey, Vec<usize>> = HashMap::new();
        for (index, request) in requests.iter().enumerate() {
            if !request.options.visible
                || !Self::native_window_options_have_screen_position(&request.options)
            {
                continue;
            }
            let key = request.group.as_ref().map_or(
                NativeWindowPlacementGroupKey::Window(request.key),
                |group| NativeWindowPlacementGroupKey::Group(group.id),
            );
            groups.entry(key).or_default().push(index);
        }

        for indices in groups.values() {
            let Some(bounds) = native_window_request_bounds(requests, indices) else {
                continue;
            };
            if monitors.iter().any(|monitor| {
                monitor.contains_rect_with_margin(bounds, NATIVE_WINDOW_PLACEMENT_MARGIN)
            }) {
                continue;
            }

            let monitor = nearest_monitor_to_rect(&monitors, bounds);
            let delta =
                clamp_rect_to_monitor_delta(bounds, monitor, NATIVE_WINDOW_PLACEMENT_MARGIN);
            if delta.x.abs() <= f32::EPSILON && delta.y.abs() <= f32::EPSILON {
                continue;
            }

            for index in indices {
                let options = &mut requests[*index].options;
                if let (Some(x), Some(y)) = (options.x, options.y) {
                    options.x = Some(x + delta.x);
                    options.y = Some(y + delta.y);
                }
            }
        }
    }

    fn native_window_request_for_host(&self, request: &NativeWindowRequest) -> NativeWindowRequest {
        let mut request = request.clone();
        Self::apply_native_window_state_to_options(&mut request.options, request.state);
        if Self::native_window_options_have_screen_position(&request.options) {
            request.options = self.resolve_native_window_options(&request.options);
        } else if let Some((x, y)) = self.native_window_positions.get(&request.key).copied() {
            request.options.x = Some(x);
            request.options.y = Some(y);
            request.options.position_origin = NativeWindowPositionOrigin::Screen;
        } else {
            request.options = self.resolve_native_window_options(&request.options);
        }
        request
    }

    fn apply_native_window_state_to_options(
        options: &mut NativeWindowOptions,
        state: Option<WindowState>,
    ) {
        let Some(state) = state else {
            return;
        };
        let size = state.size_non_reactive();
        options.width = size.width;
        options.height = size.height;
        if let Some(position) = state.position_non_reactive() {
            options.x = Some(position.x);
            options.y = Some(position.y);
            options.position_origin = NativeWindowPositionOrigin::Screen;
        }
    }

    fn resolve_native_window_options(&self, options: &NativeWindowOptions) -> NativeWindowOptions {
        let mut options = options.clone();
        if options.position_origin == NativeWindowPositionOrigin::HostWindow {
            if let (Some(x), Some(y), Some((host_x, host_y))) =
                (options.x, options.y, self.host_window_position())
            {
                options.x = Some(host_x + x);
                options.y = Some(host_y + y);
            }
            options.position_origin = NativeWindowPositionOrigin::Screen;
        }
        options
    }

    fn native_window_options_have_screen_position(options: &NativeWindowOptions) -> bool {
        options.position_origin == NativeWindowPositionOrigin::Screen
            && options.x.is_some()
            && options.y.is_some()
    }

    fn host_window_position(&self) -> Option<(f32, f32)> {
        let window = self.window.as_ref()?;
        logical_outer_position(window)
    }

    fn remember_native_window_position(&mut self, native: &NativeWindowSurface) {
        if let Some((x, y)) = Self::initial_native_window_position(
            &native.options,
            current_native_window_position(native),
        ) {
            self.native_window_positions.insert(native.key, (x, y));
            if let Some(state) = native.state {
                if state.position_non_reactive().is_none() {
                    state.set_position(Some(cranpose_ui::Point::new(x, y)));
                }
            }
            notify_native_window_moved(&native.events, x, y);
        }
    }

    fn initial_native_window_position(
        options: &NativeWindowOptions,
        current_position: Option<(f32, f32)>,
    ) -> Option<(f32, f32)> {
        if Self::native_window_options_have_screen_position(options) {
            Some((
                options.x.expect("screen x should exist"),
                options.y.expect("screen y should exist"),
            ))
        } else {
            current_position
        }
    }

    fn native_window_graph_snapshots(&self) -> Vec<WindowGraphPeerSnapshot> {
        self.native_windows
            .values()
            .filter_map(|native| {
                self.native_window_graph_snapshot(
                    native,
                    None,
                    NativeWindowGraphPositionSource::CachedThenCurrent,
                )
            })
            .collect()
    }

    fn native_window_graph_snapshots_with(
        &self,
        native: &NativeWindowSurface,
        position: Option<cranpose_ui::Point>,
    ) -> Vec<WindowGraphPeerSnapshot> {
        self.native_window_graph_snapshots_with_source(
            native,
            position,
            NativeWindowGraphPositionSource::CachedThenCurrent,
        )
    }

    fn native_window_graph_snapshots_with_current_positions(
        &self,
        native: &NativeWindowSurface,
        position: Option<cranpose_ui::Point>,
    ) -> Vec<WindowGraphPeerSnapshot> {
        self.native_window_graph_snapshots_with_source(
            native,
            position,
            NativeWindowGraphPositionSource::CurrentThenCached,
        )
    }

    fn native_window_graph_snapshots_with_source(
        &self,
        native: &NativeWindowSurface,
        position: Option<cranpose_ui::Point>,
        source: NativeWindowGraphPositionSource,
    ) -> Vec<WindowGraphPeerSnapshot> {
        let mut snapshots: Vec<_> = self
            .native_windows
            .values()
            .filter_map(|native| self.native_window_graph_snapshot(native, None, source))
            .collect();
        snapshots.retain(|snapshot| snapshot.node.id != native.key);
        if let Some(snapshot) = self.native_window_graph_snapshot(native, position, source) {
            snapshots.push(snapshot);
        }
        snapshots
    }

    fn native_window_graph_snapshot(
        &self,
        native: &NativeWindowSurface,
        position: Option<cranpose_ui::Point>,
        source: NativeWindowGraphPositionSource,
    ) -> Option<WindowGraphPeerSnapshot> {
        let cached_position = self.native_window_positions.get(&native.key).copied();
        let current_position = current_native_window_position(native);
        let options_position = native_window_options_position(&native.options);
        let position = native_window_graph_position(
            position,
            cached_position,
            current_position,
            options_position,
            source,
        )?;
        Some(WindowGraphPeerSnapshot {
            node: WindowGraphNodeSnapshot {
                id: native.key,
                position,
                size: native
                    .state
                    .map(WindowState::size_non_reactive)
                    .unwrap_or_else(|| {
                        cranpose_ui::Size::new(native.options.width, native.options.height)
                    }),
            },
            group: native.group.clone(),
        })
    }

    fn apply_window_graph_drag(
        &mut self,
        dragged: NativeWindowKey,
        target: cranpose_ui::Point,
    ) -> bool {
        let moves = self.window_graph.drag_to(dragged, target);
        self.apply_window_graph_moves(moves)
    }

    fn finish_window_graph_drag(&mut self) -> bool {
        let snapshots = self.native_window_graph_snapshots();
        let moves = self.window_graph.finish_drag(&snapshots);
        self.apply_window_graph_moves(moves)
    }

    fn apply_window_graph_moves(&mut self, moves: Vec<WindowGraphMove>) -> bool {
        let mut moved = false;
        for window_move in moves {
            let Some(window_id) = self.native_window_ids.get(&window_move.id).copied() else {
                continue;
            };
            let Some(native) = self.native_windows.get_mut(&window_id) else {
                continue;
            };
            if Self::apply_native_window_position(native, window_move.position) {
                self.native_window_positions.insert(
                    window_move.id,
                    (window_move.position.x, window_move.position.y),
                );
                moved = true;
            }
        }
        moved
    }

    fn create_native_window_shell(
        event_loop: &dyn ActiveEventLoop,
        request: NativeWindowRequest,
        headless: bool,
    ) -> Result<NativeWindowShell, LaunchError> {
        let create_started = Instant::now();
        let options = &request.options;
        let attributes = native_window_attributes(options, headless);

        let window: Arc<dyn Window> = event_loop
            .create_window(attributes)
            .map_err(LaunchError::WindowCreate)?
            .into();
        trace_native_window_timing(format_args!(
            "{} create_window {}ms",
            options.title,
            create_started.elapsed().as_millis()
        ));
        Ok(NativeWindowShell {
            request,
            window,
            create_started,
        })
    }

    fn create_native_window(
        &self,
        shell: NativeWindowShell,
    ) -> Result<NativeWindowSurface, LaunchError> {
        let NativeWindowShell {
            request,
            window,
            create_started,
        } = shell;
        let context = self
            .gpu_context
            .as_ref()
            .expect("native windows require an initialized desktop GPU context");

        let options = &request.options;
        let surface = context
            .instance
            .create_surface(window.clone())
            .map_err(LaunchError::SurfaceCreate)?;
        trace_native_window_timing(format_args!(
            "{} create_surface {}ms",
            options.title,
            create_started.elapsed().as_millis()
        ));
        let surface_caps = surface.get_capabilities(&context.adapter);
        let surface_format = select_surface_format(&surface_caps);
        let present_mode = desktop_present_mode(&surface_caps, self.frame_pacing_mode);
        let size = window.surface_size();
        let surface_config = surface_config_for_window(
            &surface_caps,
            surface_format,
            size.width.max(1),
            size.height.max(1),
            present_mode,
            options.transparent,
            self.frame_pacing_mode,
        );
        surface.configure(&context.device, &surface_config);
        trace_native_window_timing(format_args!(
            "{} configure {}ms",
            options.title,
            create_started.elapsed().as_millis()
        ));

        let scale_factor = window.scale_factor();
        let renderer = wgpu_renderer_for_surface(
            context.text_system.clone(),
            Arc::clone(&context.device),
            Arc::clone(&context.queue),
            surface_format,
            context.adapter_backend,
            scale_factor,
        );
        trace_native_window_timing(format_args!(
            "{} renderer {}ms",
            options.title,
            create_started.elapsed().as_millis()
        ));
        cranpose_ui::set_density(scale_factor as f32);

        let content = request.content.clone();
        let viewport = (
            surface_config.width as f32 / scale_factor as f32,
            surface_config.height as f32 / scale_factor as f32,
        );
        let mut app = AppShell::new_with_size(
            renderer,
            default_root_key(),
            move || {
                (content.borrow_mut())();
            },
            (surface_config.width, surface_config.height),
            viewport,
        );
        let mut dev_options = self.settings.dev_options.clone();
        dev_options.frame_pacing_mode = self.frame_pacing_mode;
        dev_options.frame_pacing_controls = false;
        app.set_dev_options(dev_options);
        trace_native_window_timing(format_args!(
            "{} app_shell {}ms",
            options.title,
            create_started.elapsed().as_millis()
        ));

        let frame_waker_window = window.clone();
        app.set_frame_waker(move || {
            frame_waker_window.request_redraw();
        });

        let mut platform = DesktopWinitPlatform::default();
        platform.set_scale_factor(scale_factor);
        window.request_redraw();
        trace_native_window_timing(format_args!(
            "{} create done {}ms",
            options.title,
            create_started.elapsed().as_millis()
        ));

        Ok(NativeWindowSurface {
            key: request.key,
            revision: request.revision,
            options: request.options.clone(),
            events: request.events.clone(),
            state: request.state,
            group: request.group.clone(),
            window,
            surface,
            surface_config,
            surface_caps,
            app,
            platform,
            last_cursor_position: None,
            last_cursor_physical_position: None,
            frame_pacing_mode: self.frame_pacing_mode,
            last_frame_start_time: None,
            vsync_interval: default_vsync_interval(),
            pending_outer_positions: PendingNativeWindowPositions::default(),
            active_drag: None,
        })
    }

    fn apply_native_window_options(
        native: &mut NativeWindowSurface,
        options: &NativeWindowOptions,
        headless: bool,
    ) {
        if native.options.title != options.title {
            native.window.set_title(&options.title);
        }
        if native.options.decorations != options.decorations {
            native.window.set_decorations(options.decorations);
        }
        if native.options.resizable != options.resizable {
            native.window.set_resizable(options.resizable);
        }
        if native.options.transparent != options.transparent {
            native.window.set_transparent(options.transparent);
        }
        if native.options.always_on_top != options.always_on_top {
            native
                .window
                .set_window_level(native_window_level(options.always_on_top));
        }
        if native.options.min_width != options.min_width
            || native.options.min_height != options.min_height
        {
            native
                .window
                .set_min_surface_size(match (options.min_width, options.min_height) {
                    (Some(width), Some(height)) => {
                        Some(LogicalSize::new(width.max(1.0) as f64, height.max(1.0) as f64).into())
                    }
                    _ => None,
                });
        }
        if native.options.max_width != options.max_width
            || native.options.max_height != options.max_height
        {
            native
                .window
                .set_max_surface_size(match (options.max_width, options.max_height) {
                    (Some(width), Some(height)) => {
                        Some(LogicalSize::new(width.max(1.0) as f64, height.max(1.0) as f64).into())
                    }
                    _ => None,
                });
        }
        if native.options.visible != options.visible {
            native.window.set_visible(!headless && options.visible);
        }
        if native.options.x != options.x || native.options.y != options.y {
            if let (Some(x), Some(y)) = (options.x, options.y) {
                native.pending_outer_positions.push((x, y));
                let logical = LogicalPosition::new(x as f64, y as f64);
                let physical = logical.to_physical::<i32>(native.window.scale_factor());
                if !native_window_set_outer_position_physical(&native.window, physical) {
                    native.window.set_outer_position(Position::Logical(logical));
                }
            }
        }
        if native.options.width != options.width || native.options.height != options.height {
            if let Some(size) = native.window.request_surface_size(
                LogicalSize::new(
                    options.width.max(1.0) as f64,
                    options.height.max(1.0) as f64,
                )
                .into(),
            ) {
                Self::resize_native_surface(native, size.width, size.height);
            }
        }
        native.options = options.clone();
    }

    fn resize_native_surface(native: &mut NativeWindowSurface, width: u32, height: u32) {
        let viewport = surface_logical_viewport_size(width, height, native.window.scale_factor());
        configure_app_surface_size(
            &mut native.app,
            &native.surface,
            &mut native.surface_config,
            width,
            height,
            viewport,
        );
    }

    fn sync_native_window_position_from_os(
        native: &mut NativeWindowSurface,
        native_window_positions: &mut HashMap<NativeWindowKey, (f32, f32)>,
    ) -> bool {
        let Some(position) = current_native_window_position(native) else {
            return false;
        };
        if native_window_positions
            .get(&native.key)
            .is_some_and(|known| native_window_positions_close(*known, position))
            && native
                .state
                .and_then(|state| state.position_non_reactive())
                .is_some_and(|known| native_window_positions_close((known.x, known.y), position))
        {
            return false;
        }

        let previous_state_position = native.state.and_then(|state| state.position_non_reactive());
        native_window_positions.insert(native.key, position);
        update_native_options_position(&mut native.options, position.0, position.1);
        native.pending_outer_positions.clear();
        notify_native_window_moved(&native.events, position.0, position.1);
        sync_native_window_state_position(
            native.state,
            previous_state_position,
            position.0,
            position.1,
        );
        true
    }

    fn apply_native_window_position(
        native: &mut NativeWindowSurface,
        position: cranpose_ui::Point,
    ) -> bool {
        let logical_position = (position.x, position.y);
        if current_native_window_position(native).is_some_and(|current| {
            (current.0 - logical_position.0).abs() <= f32::EPSILON
                && (current.1 - logical_position.1).abs() <= f32::EPSILON
        }) {
            update_native_options_position(&mut native.options, position.x, position.y);
            return false;
        }

        native.pending_outer_positions.push(logical_position);
        let logical = LogicalPosition::new(position.x as f64, position.y as f64);
        let physical = logical.to_physical::<i32>(native.window.scale_factor());
        if !native_window_set_outer_position_physical(&native.window, physical) {
            native.window.set_outer_position(Position::Logical(logical));
        }
        update_native_options_position(&mut native.options, position.x, position.y);
        let previous_state_position = native.state.and_then(|state| state.position_non_reactive());
        notify_native_window_moved(&native.events, position.x, position.y);
        sync_native_window_state_position(
            native.state,
            previous_state_position,
            position.x,
            position.y,
        );
        true
    }

    fn handle_native_primary_pressed(&mut self, native: &mut NativeWindowSurface) -> bool {
        let drag_requested = Rc::new(Cell::new(false));
        let drag_requested_for_handler = Rc::clone(&drag_requested);
        let drag_handler: Rc<dyn Fn() -> bool> = Rc::new(move || {
            drag_requested_for_handler.set(true);
            true
        });
        let resize_window = native.window.clone();
        let resize_handler: Rc<dyn Fn(WindowResizeDirection)> = Rc::new(move |direction| {
            if let Err(error) = resize_window.drag_resize_window(native_resize_direction(direction))
            {
                log::debug!("native window resize request failed: {error}");
            }
        });
        let handled =
            native_window::with_native_window_drag_handler(drag_handler, resize_handler, || {
                native_window::with_native_window_surface_origin(
                    native_window_surface_origin(&native.window),
                    || native.app.pointer_pressed(),
                )
            });
        if handled {
            apply_pointer_button_frame_request(
                &native.window,
                &mut native.last_frame_start_time,
                pointer_button_frame_request(handled),
            );
            if drag_requested.get() {
                trace_native_window(format_args!("drag requested key={:?}", native.key));
                Self::sync_native_window_position_from_os(
                    native,
                    &mut self.native_window_positions,
                );
                for other_native in self.native_windows.values_mut() {
                    Self::sync_native_window_position_from_os(
                        other_native,
                        &mut self.native_window_positions,
                    );
                }
                let graph_snapshots = self.native_window_graph_snapshots_with(native, None);
                self.window_graph.start_drag(&graph_snapshots, native.key);
                if !Self::start_native_window_drag(native) {
                    trace_native_window(format_args!(
                        "drag cancel key={:?} reason=start-failed",
                        native.key
                    ));
                    self.window_graph.cancel_drag();
                }
            }
        }
        handled
    }

    fn start_native_window_drag(native: &mut NativeWindowSurface) -> bool {
        let now = Instant::now();
        if let Some(session) = Self::native_window_polling_drag_session(native, now) {
            let pointer = session.start_pointer_screen;
            let window_outer = session.start_window_outer;
            native.active_drag = Some(NativeWindowDragSession::Polling(session));
            trace_native_window(format_args!(
                "drag start polling key={:?} pointer=({:.1},{:.1}) outer=({},{})",
                native.key, pointer.x, pointer.y, window_outer.x, window_outer.y
            ));
            return true;
        }

        match native.window.drag_window() {
            Ok(()) => {
                native.active_drag = Some(NativeWindowDragSession::platform(now));
                trace_native_window(format_args!("drag start platform key={:?}", native.key));
                return true;
            }
            Err(error) => {
                log::debug!("native window drag request failed: {error}");
            }
        }

        false
    }

    fn native_window_polling_drag_session(
        native: &NativeWindowSurface,
        now: Instant,
    ) -> Option<NativeWindowPollingDragSession> {
        let pointer = native_window_global_pointer_state()
            .map(|state| state.position)
            .or_else(|| {
                native.last_cursor_physical_position.and_then(|position| {
                    native_window_screen_pointer_physical(&native.window, position)
                })
            })?;
        let window_outer = current_native_window_physical_position(&native.window)?;
        Some(NativeWindowPollingDragSession::new(
            pointer,
            window_outer,
            now,
        ))
    }

    fn poll_active_native_window_drags(&mut self, now: Instant) -> bool {
        let has_due_drag = self.native_windows.values().any(|native| {
            native
                .active_drag
                .is_some_and(|active_drag| active_drag.next_poll_at() <= now)
        });
        if !has_due_drag {
            return false;
        }

        let pointer = native_window_global_pointer_state();
        let mut updates = Vec::new();
        let mut finish_drag = false;
        for native in self.native_windows.values_mut() {
            let Some(active_drag) = native.active_drag.as_mut() else {
                continue;
            };
            if active_drag.next_poll_at() > now {
                continue;
            }
            active_drag.set_next_poll_at(now + NATIVE_WINDOW_DRAG_POLL_INTERVAL);

            let Some(pointer) = pointer else {
                trace_native_window(format_args!(
                    "drag poll skipped key={:?} reason=no-global-pointer",
                    native.key
                ));
                continue;
            };
            if !pointer.primary_down && active_drag.finishes_on_global_pointer_release() {
                native.active_drag = None;
                finish_drag = true;
                trace_native_window(format_args!(
                    "drag finish key={:?} reason=global-release",
                    native.key
                ));
                if native.app.pointer_released() {
                    native.window.request_redraw();
                }
                native.app.sync_selection_to_primary();
                continue;
            }
            if let Some(update) =
                Self::update_native_window_polling_drag_target(native, pointer.position)
            {
                updates.push(update);
            }
        }

        let mut moved = !updates.is_empty();
        for (key, position) in updates {
            moved |= self.apply_window_graph_drag(key, position);
        }
        if finish_drag {
            moved |= self.finish_window_graph_drag();
        }
        moved
    }

    fn poll_external_native_window_moves(&mut self, now: Instant) -> bool {
        if self.native_windows.is_empty() || self.next_native_window_position_poll_at > now {
            return false;
        }
        self.next_native_window_position_poll_at = now + NATIVE_WINDOW_POSITION_POLL_INTERVAL;

        let mut external_moves = Vec::new();
        let native_window_positions = &mut self.native_window_positions;
        for (window_id, native) in &mut self.native_windows {
            if !native.options.visible || native.active_drag.is_some() {
                continue;
            }
            let Some(position) = current_native_window_position(native) else {
                continue;
            };
            if native.pending_outer_positions.acknowledge(position) {
                native_window_positions.insert(native.key, position);
                update_native_options_position(&mut native.options, position.0, position.1);
                continue;
            }
            if native.pending_outer_positions.has_pending()
                || native_window_positions
                    .get(&native.key)
                    .is_some_and(|known| native_window_positions_close(*known, position))
            {
                continue;
            }

            let previous_state_position =
                native.state.and_then(|state| state.position_non_reactive());
            let previous_graph_position = native_window_positions
                .get(&native.key)
                .map(|(x, y)| cranpose_ui::Point::new(*x, *y))
                .or(previous_state_position)
                .or_else(|| {
                    native_window_options_position(&native.options)
                        .map(|(x, y)| cranpose_ui::Point::new(x, y))
                });
            external_moves.push((
                *window_id,
                native.key,
                position,
                previous_state_position,
                previous_graph_position,
            ));
        }

        let mut moved = false;
        for (window_id, key, position, previous_state_position, previous_graph_position) in
            external_moves
        {
            moved |= self.reconcile_external_native_window_move(
                window_id,
                key,
                position,
                previous_state_position,
                previous_graph_position,
            );
        }
        moved
    }

    fn reconcile_external_native_window_move(
        &mut self,
        window_id: WinitWindowId,
        key: NativeWindowKey,
        position: (f32, f32),
        previous_state_position: Option<cranpose_ui::Point>,
        previous_graph_position: Option<cranpose_ui::Point>,
    ) -> bool {
        let Some(native) = self.native_windows.get(&window_id) else {
            return false;
        };
        let previous_graph_snapshots = self
            .native_window_graph_snapshots_with_current_positions(native, previous_graph_position);

        let Some(native) = self.native_windows.get_mut(&window_id) else {
            return false;
        };
        if native.active_drag.is_some() || native.pending_outer_positions.has_pending() {
            return false;
        }

        trace_native_window(format_args!(
            "poll external move key={:?} pos=({:.1},{:.1})",
            key, position.0, position.1
        ));
        self.native_window_positions.insert(key, position);
        update_native_options_position(&mut native.options, position.0, position.1);
        let position = cranpose_ui::Point::new(position.0, position.1);
        notify_native_window_moved(&native.events, position.x, position.y);
        sync_native_window_state_position(
            native.state,
            previous_state_position,
            position.x,
            position.y,
        );

        let graph_moves = self
            .window_graph
            .external_move(&previous_graph_snapshots, key, position);
        self.apply_window_graph_moves(graph_moves);
        true
    }

    fn update_native_window_polling_drag_target(
        native: &mut NativeWindowSurface,
        pointer: PhysicalPosition<f64>,
    ) -> Option<(NativeWindowKey, cranpose_ui::Point)> {
        let active_drag = native.active_drag.as_mut()?.polling_mut()?;
        let target = active_drag.target_for_pointer(pointer);
        if target == active_drag.last_target_outer {
            return None;
        }
        active_drag.last_target_outer = target;
        let logical = target.to_logical::<f64>(native.window.scale_factor());
        trace_native_window(format_args!(
            "drag target key={:?} logical=({:.1},{:.1}) physical=({},{})",
            native.key, logical.x, logical.y, target.x, target.y
        ));
        Some((
            native.key,
            cranpose_ui::Point::new(logical.x as f32, logical.y as f32),
        ))
    }

    fn native_window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: WinitWindowId,
        event: WindowEvent,
    ) {
        let Some(mut native) = self.native_windows.remove(&window_id) else {
            return;
        };

        let mut keep_window = true;
        let mut sync_after_event = false;
        let mut graph_drag_after_insert = None::<(NativeWindowKey, cranpose_ui::Point)>;
        let mut graph_moves_after_insert = Vec::<WindowGraphMove>::new();
        let mut finish_graph_drag_after_insert = false;
        match event {
            WindowEvent::CloseRequested => {
                trace_native_window(format_args!("event close-request key={:?}", native.key));
                notify_native_window_close_requested(&native.events);
                self.remember_native_window_position(&native);
                self.native_window_ids.remove(&native.key);
                self.closed_native_windows.insert(native.key);
                keep_window = false;
            }
            WindowEvent::SurfaceResized(new_size) => {
                let previous_state_size = native.state.map(|state| state.size_non_reactive());
                update_native_options_size(
                    &mut native.options,
                    &native.window,
                    new_size.width,
                    new_size.height,
                );
                notify_native_window_resized(
                    &native.events,
                    &native.window,
                    new_size.width,
                    new_size.height,
                );
                sync_native_window_state_size(
                    native.state,
                    previous_state_size,
                    &native.window,
                    new_size.width,
                    new_size.height,
                );
                Self::resize_native_surface(&mut native, new_size.width, new_size.height);
                sync_after_event = true;
            }
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                mut surface_size_writer,
            } => {
                let previous_state_size = native.state.map(|state| state.size_non_reactive());
                update_app_scale_factor(&mut native.app, &mut native.platform, scale_factor);

                let new_size = native.window.surface_size();
                let _ = surface_size_writer.request_surface_size(new_size);
                update_native_options_size(
                    &mut native.options,
                    &native.window,
                    new_size.width,
                    new_size.height,
                );
                notify_native_window_resized(
                    &native.events,
                    &native.window,
                    new_size.width,
                    new_size.height,
                );
                sync_native_window_state_size(
                    native.state,
                    previous_state_size,
                    &native.window,
                    new_size.width,
                    new_size.height,
                );
                Self::resize_native_surface(&mut native, new_size.width, new_size.height);
                sync_after_event = true;
            }
            WindowEvent::Moved(position) => {
                native.vsync_interval = monitor_refresh_interval(&native.window);
                let previous_state_position =
                    native.state.and_then(|state| state.position_non_reactive());
                let previous_graph_position = self
                    .native_window_positions
                    .get(&native.key)
                    .map(|(x, y)| cranpose_ui::Point::new(*x, *y))
                    .or(previous_state_position)
                    .or_else(|| match (native.options.x, native.options.y) {
                        (Some(x), Some(y)) => Some(cranpose_ui::Point::new(x, y)),
                        _ => None,
                    });
                let previous_graph_snapshots = self
                    .native_window_graph_snapshots_with_current_positions(
                        &native,
                        previous_graph_position,
                    );
                let position = current_native_window_position(&native).unwrap_or_else(|| {
                    let logical = position.to_logical::<f64>(native.window.scale_factor());
                    (logical.x as f32, logical.y as f32)
                });
                let acknowledged_programmatic_move =
                    native.pending_outer_positions.acknowledge(position);
                trace_native_window(format_args!(
                    "event moved key={:?} pos=({:.1},{:.1}) acknowledged={} active_drag={}",
                    native.key,
                    position.0,
                    position.1,
                    acknowledged_programmatic_move,
                    native.active_drag.is_some()
                ));
                self.native_window_positions.insert(native.key, position);
                update_native_options_position(&mut native.options, position.0, position.1);
                if !acknowledged_programmatic_move {
                    let position = cranpose_ui::Point::new(position.0, position.1);
                    native.pending_outer_positions.clear();
                    notify_native_window_moved(&native.events, position.x, position.y);
                    sync_native_window_state_position(
                        native.state,
                        previous_state_position,
                        position.x,
                        position.y,
                    );
                    if native.active_drag.is_some() {
                        graph_moves_after_insert = self.window_graph.drag_to(native.key, position);
                    } else if native_window_global_pointer_state().is_some_and(|pointer| {
                        pointer.primary_down
                            && native_window_surface_contains_pointer(&native, pointer.position)
                            && previous_graph_position.is_some_and(|previous| {
                                native_window_surface_at_logical_position_contains_pointer(
                                    &native.window,
                                    previous,
                                    pointer.position,
                                )
                            })
                    }) {
                        self.window_graph
                            .start_drag(&previous_graph_snapshots, native.key);
                        native.active_drag =
                            Some(NativeWindowDragSession::platform(Instant::now()));
                        trace_native_window(format_args!(
                            "drag start inferred-platform key={:?}",
                            native.key
                        ));
                        graph_moves_after_insert = self.window_graph.drag_to(native.key, position);
                    } else {
                        graph_moves_after_insert = self.window_graph.external_move(
                            &previous_graph_snapshots,
                            native.key,
                            position,
                        );
                    }
                    sync_after_event = true;
                }
            }
            WindowEvent::PointerMoved { position, .. } => {
                let logical = native.platform.pointer_position(position);
                native.last_cursor_position = Some((logical.x, logical.y));
                native.last_cursor_physical_position = Some(position);
                let fallback_pointer =
                    native_window_screen_pointer_physical(&native.window, position);
                if let Some(pointer) = native_window_global_pointer_state()
                    .map(|state| state.position)
                    .or(fallback_pointer)
                {
                    if let Some((key, position)) =
                        Self::update_native_window_polling_drag_target(&mut native, pointer)
                    {
                        graph_drag_after_insert = Some((key, position));
                        sync_after_event = true;
                    }
                }
                let handled = native_window::with_native_window_surface_origin(
                    native_window_surface_origin(&native.window),
                    || native.app.set_cursor(logical.x, logical.y),
                );
                if handled {
                    native.window.request_redraw();
                    sync_after_event = true;
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.current_modifiers = modifiers.state();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                dispatch_mouse_wheel(
                    &mut native.app,
                    &native.platform,
                    self.current_modifiers,
                    native.last_cursor_position,
                    delta,
                );
            }
            WindowEvent::PointerButton {
                state,
                button: ButtonSource::Mouse(MouseButton::Left),
                ..
            } => {
                trace_native_window(format_args!(
                    "event pointer-button key={:?} state={:?} cursor={:?}",
                    native.key, state, native.last_cursor_position
                ));
                if let Some((x, y)) = native.last_cursor_position {
                    native_window::with_native_window_surface_origin(
                        native_window_surface_origin(&native.window),
                        || native.app.set_cursor(x, y),
                    );
                }
                match state {
                    ElementState::Pressed => {
                        if self.handle_native_primary_pressed(&mut native) {
                            sync_after_event = true;
                        }
                    }
                    ElementState::Released => {
                        let fallback_pointer =
                            native.last_cursor_physical_position.and_then(|position| {
                                native_window_screen_pointer_physical(&native.window, position)
                            });
                        if let Some(pointer) = native_window_global_pointer_state()
                            .map(|state| state.position)
                            .or(fallback_pointer)
                        {
                            if let Some((key, position)) =
                                Self::update_native_window_polling_drag_target(&mut native, pointer)
                            {
                                graph_drag_after_insert = Some((key, position));
                            }
                        }
                        finish_graph_drag_after_insert = native.active_drag.take().is_some();
                        if finish_graph_drag_after_insert {
                            trace_native_window(format_args!(
                                "drag finish key={:?} reason=local-release",
                                native.key
                            ));
                        }
                        let handled = native_window::with_native_window_surface_origin(
                            native_window_surface_origin(&native.window),
                            || native.app.pointer_released(),
                        );
                        native.app.sync_selection_to_primary();
                        if handled {
                            apply_pointer_button_frame_request(
                                &native.window,
                                &mut native.last_frame_start_time,
                                pointer_button_frame_request(handled),
                            );
                            sync_after_event = true;
                        }
                    }
                }
            }
            WindowEvent::PointerButton {
                state: ElementState::Pressed,
                button: ButtonSource::Mouse(MouseButton::Middle),
                ..
            } => {
                dispatch_middle_click_paste(&mut native.app, native.last_cursor_position);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                dispatch_keyboard_input(&mut native.app, self.current_modifiers, event);
            }
            WindowEvent::Focused(false) if native.active_drag.is_none() => {
                cancel_app_input(&mut native.app);
            }
            WindowEvent::Focused(false) => {}
            WindowEvent::Ime(ime_event) => {
                dispatch_ime_event(&mut native.app, ime_event);
            }
            WindowEvent::PointerLeft { .. } if native.active_drag.is_none() => {
                native.app.cancel_gesture();
            }
            WindowEvent::PointerLeft { .. } => {}
            WindowEvent::RedrawRequested => {
                if let Some(deadline) = native.last_frame_start_time.and_then(|started_at| {
                    native
                        .frame_interval()
                        .map(|interval| started_at + interval)
                }) {
                    if deadline > Instant::now() {
                        event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
                        self.native_windows.insert(window_id, native);
                        return;
                    }
                }
                Self::redraw_native_window(&mut native);
            }
            _ => {}
        }

        if keep_window {
            self.native_windows.insert(window_id, native);
            if !graph_moves_after_insert.is_empty()
                && self.apply_window_graph_moves(graph_moves_after_insert)
            {
                sync_after_event = true;
            }
            if let Some((key, position)) = graph_drag_after_insert {
                if self.apply_window_graph_drag(key, position) {
                    sync_after_event = true;
                }
            }
            if finish_graph_drag_after_insert && self.finish_window_graph_drag() {
                sync_after_event = true;
            }
            if sync_after_event {
                self.refresh_and_sync_native_windows(event_loop);
            }
        }
    }

    fn redraw_native_window(native: &mut NativeWindowSurface) {
        let frame_started_at = Instant::now();
        let scale_factor = native.window.scale_factor();
        cranpose_ui::set_density(scale_factor as f32);
        native.app.update();

        let output = match current_surface_texture(&native.surface, "native window") {
            SurfaceFrame::Ready(output) => output,
            SurfaceFrame::Reconfigure => {
                let size = native.window.surface_size();
                Self::resize_native_surface(native, size.width, size.height);
                return;
            }
            SurfaceFrame::Skip => {
                return;
            }
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        if let Err(error) = native.app.renderer().render(
            &view,
            native.surface_config.width,
            native.surface_config.height,
        ) {
            log::error!("native window render failed: {error:?}");
            return;
        }

        output.present();
        native.last_frame_start_time = Some(frame_started_at);
    }

    #[cfg(feature = "robot")]
    fn set_robot_controller(&mut self, controller: RobotController) {
        self.robot_controller = Some(controller);
    }
}

fn apply_frame_pacing_mode(
    app: &mut AppShell<WgpuRenderer>,
    surface: &wgpu::Surface<'static>,
    surface_config: &mut wgpu::SurfaceConfiguration,
    surface_caps: Option<&wgpu::SurfaceCapabilities>,
    mode: FramePacingMode,
) {
    app.set_frame_pacing_mode(mode);
    if let Some(caps) = surface_caps {
        let present_mode = crate::present_mode::select_present_mode_for_frame_pacing(caps, mode);
        let frame_latency = desired_frame_latency(mode);
        if surface_config.present_mode != present_mode
            || surface_config.desired_maximum_frame_latency != frame_latency
        {
            surface_config.present_mode = present_mode;
            surface_config.desired_maximum_frame_latency = frame_latency;
            let device = app.renderer().device();
            surface.configure(device, surface_config);
        }
    }
}

fn desired_frame_latency(mode: FramePacingMode) -> u32 {
    match mode {
        FramePacingMode::Vsync | FramePacingMode::Hard60 | FramePacingMode::Hard120 => 1,
        FramePacingMode::NoVsync => 2,
    }
}

fn frame_interval_for_mode(mode: FramePacingMode, vsync_interval: Duration) -> Option<Duration> {
    match mode {
        FramePacingMode::Vsync => Some(vsync_interval),
        FramePacingMode::Hard60 => Some(Duration::from_nanos(16_666_667)),
        FramePacingMode::Hard120 => Some(Duration::from_nanos(8_333_333)),
        FramePacingMode::NoVsync => None,
    }
}

fn logical_monitor_rects(event_loop: &dyn ActiveEventLoop) -> Vec<DesktopRect> {
    event_loop
        .available_monitors()
        .filter_map(|monitor| logical_monitor_rect(&monitor))
        .collect()
}

fn logical_monitor_rect(monitor: &winit::monitor::MonitorHandle) -> Option<DesktopRect> {
    let position = monitor.position()?;
    let scale_factor = monitor.scale_factor() as f32;
    if scale_factor <= 0.0 {
        return None;
    }
    let size = monitor.current_video_mode()?.size();
    Some(DesktopRect {
        x: position.x as f32 / scale_factor,
        y: position.y as f32 / scale_factor,
        width: size.width as f32 / scale_factor,
        height: size.height as f32 / scale_factor,
    })
}

fn native_window_request_bounds(
    requests: &[NativeWindowRequest],
    indices: &[usize],
) -> Option<DesktopRect> {
    let mut bounds = None::<DesktopRect>;
    for index in indices {
        let options = &requests[*index].options;
        let (Some(x), Some(y)) = (options.x, options.y) else {
            continue;
        };
        let rect = DesktopRect {
            x,
            y,
            width: options.width.max(1.0),
            height: options.height.max(1.0),
        };
        bounds = Some(match bounds {
            Some(current) => union_desktop_rect(current, rect),
            None => rect,
        });
    }
    bounds
}

fn union_desktop_rect(a: DesktopRect, b: DesktopRect) -> DesktopRect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let right = a.right().max(b.right());
    let bottom = a.bottom().max(b.bottom());
    DesktopRect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    }
}

fn nearest_monitor_to_rect(monitors: &[DesktopRect], rect: DesktopRect) -> DesktopRect {
    let center = rect.center();
    *monitors
        .iter()
        .min_by(|a, b| {
            a.distance_to_point(center)
                .total_cmp(&b.distance_to_point(center))
        })
        .expect("at least one monitor")
}

fn clamp_rect_to_monitor_delta(
    rect: DesktopRect,
    monitor: DesktopRect,
    margin: f32,
) -> cranpose_ui::Point {
    let target_x = clamped_axis_origin(rect.x, rect.width, monitor.x, monitor.width, margin);
    let target_y = clamped_axis_origin(rect.y, rect.height, monitor.y, monitor.height, margin);
    cranpose_ui::Point::new(target_x - rect.x, target_y - rect.y)
}

fn clamped_axis_origin(
    origin: f32,
    length: f32,
    monitor_origin: f32,
    monitor_length: f32,
    margin: f32,
) -> f32 {
    let min = monitor_origin + margin;
    let max = monitor_origin + monitor_length - margin - length;
    if max >= min {
        origin.clamp(min, max)
    } else {
        monitor_origin + (monitor_length - length) / 2.0
    }
}

fn native_window_attributes(options: &NativeWindowOptions, headless: bool) -> WindowAttributes {
    let mut attributes = WindowAttributes::default()
        .with_title(options.title.clone())
        .with_surface_size(LogicalSize::new(
            options.width.max(1.0) as f64,
            options.height.max(1.0) as f64,
        ))
        .with_decorations(options.decorations)
        .with_transparent(options.transparent)
        .with_resizable(options.resizable)
        .with_visible(!headless && options.visible)
        .with_window_level(native_window_level(options.always_on_top));
    if let (Some(width), Some(height)) = (options.min_width, options.min_height) {
        attributes = attributes.with_min_surface_size(LogicalSize::new(
            width.max(1.0) as f64,
            height.max(1.0) as f64,
        ));
    }
    if let (Some(width), Some(height)) = (options.max_width, options.max_height) {
        attributes = attributes.with_max_surface_size(LogicalSize::new(
            width.max(1.0) as f64,
            height.max(1.0) as f64,
        ));
    }
    if let (Some(x), Some(y)) = (options.x, options.y) {
        attributes =
            attributes.with_position(Position::Logical(LogicalPosition::new(x as f64, y as f64)));
    }
    attributes
}

fn desktop_present_mode(
    surface_caps: &wgpu::SurfaceCapabilities,
    frame_pacing_mode: FramePacingMode,
) -> wgpu::PresentMode {
    if std::env::var_os("CRANPOSE_PRESENT_MODE").is_some() {
        crate::present_mode::select_present_mode(surface_caps)
    } else {
        crate::present_mode::select_present_mode_for_frame_pacing(surface_caps, frame_pacing_mode)
    }
}

fn surface_config_for_window(
    surface_caps: &wgpu::SurfaceCapabilities,
    surface_format: wgpu::TextureFormat,
    width: u32,
    height: u32,
    present_mode: wgpu::PresentMode,
    transparent: bool,
    frame_pacing_mode: FramePacingMode,
) -> wgpu::SurfaceConfiguration {
    wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width,
        height,
        present_mode,
        alpha_mode: select_alpha_mode(surface_caps, transparent),
        view_formats: vec![],
        desired_maximum_frame_latency: desired_frame_latency(frame_pacing_mode),
    }
}

fn wgpu_renderer_for_surface(
    text_system: WgpuTextSystem,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface_format: wgpu::TextureFormat,
    backend: wgpu::Backend,
    scale_factor: f64,
) -> WgpuRenderer {
    let mut renderer = WgpuRenderer::with_text_system(text_system);
    renderer.set_root_scale(scale_factor as f32);
    renderer.init_gpu(device, queue, surface_format, backend);
    renderer
}

fn select_surface_format(surface_caps: &wgpu::SurfaceCapabilities) -> wgpu::TextureFormat {
    surface_caps
        .formats
        .iter()
        .copied()
        .find(|f| f.is_srgb())
        .unwrap_or(surface_caps.formats[0])
}

fn select_alpha_mode(
    surface_caps: &wgpu::SurfaceCapabilities,
    transparent: bool,
) -> wgpu::CompositeAlphaMode {
    if transparent {
        surface_caps
            .alpha_modes
            .iter()
            .copied()
            .find(|mode| *mode == wgpu::CompositeAlphaMode::PreMultiplied)
            .unwrap_or(surface_caps.alpha_modes[0])
    } else {
        surface_caps
            .alpha_modes
            .iter()
            .copied()
            .find(|mode| *mode == wgpu::CompositeAlphaMode::Opaque)
            .unwrap_or(surface_caps.alpha_modes[0])
    }
}

fn native_window_level(always_on_top: bool) -> WindowLevel {
    if always_on_top {
        WindowLevel::AlwaysOnTop
    } else {
        WindowLevel::Normal
    }
}

fn current_native_window_physical_position(
    window: &Arc<dyn Window>,
) -> Option<PhysicalPosition<i32>> {
    native_window_x11_outer_position_physical(window).or_else(|| window.outer_position().ok())
}

fn current_native_window_position(native: &NativeWindowSurface) -> Option<(f32, f32)> {
    current_native_window_physical_position(&native.window).map(|position| {
        let logical = position.to_logical::<f64>(native.window.scale_factor());
        (logical.x as f32, logical.y as f32)
    })
}

fn native_window_options_position(options: &NativeWindowOptions) -> Option<(f32, f32)> {
    match (options.x, options.y) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    }
}

fn native_window_graph_position(
    override_position: Option<cranpose_ui::Point>,
    cached_position: Option<(f32, f32)>,
    current_position: Option<(f32, f32)>,
    options_position: Option<(f32, f32)>,
    source: NativeWindowGraphPositionSource,
) -> Option<cranpose_ui::Point> {
    override_position.or_else(|| {
        let selected = match source {
            NativeWindowGraphPositionSource::CachedThenCurrent => {
                cached_position.or(current_position).or(options_position)
            }
            NativeWindowGraphPositionSource::CurrentThenCached => {
                current_position.or(cached_position).or(options_position)
            }
        }?;
        Some(cranpose_ui::Point::new(selected.0, selected.1))
    })
}

fn native_window_positions_close(a: (f32, f32), b: (f32, f32)) -> bool {
    (a.0 - b.0).abs() <= 1.0 && (a.1 - b.1).abs() <= 1.0
}

fn native_window_surface_origin(window: &Arc<dyn Window>) -> Option<cranpose_ui::Point> {
    let outer = current_native_window_physical_position(window)?;
    let surface = window.surface_position();
    let scale_factor = window.scale_factor();
    let physical = winit::dpi::PhysicalPosition::new(outer.x + surface.x, outer.y + surface.y);
    let logical = physical.to_logical::<f64>(scale_factor);
    Some(cranpose_ui::Point::new(logical.x as f32, logical.y as f32))
}

fn native_window_screen_pointer_physical(
    window: &Arc<dyn Window>,
    local: PhysicalPosition<f64>,
) -> Option<PhysicalPosition<f64>> {
    let outer = current_native_window_physical_position(window)?;
    let surface = window.surface_position();
    Some(PhysicalPosition::new(
        outer.x as f64 + surface.x as f64 + local.x,
        outer.y as f64 + surface.y as f64 + local.y,
    ))
}

fn native_window_surface_contains_pointer(
    native: &NativeWindowSurface,
    pointer: PhysicalPosition<f64>,
) -> bool {
    let Some(outer) = current_native_window_physical_position(&native.window) else {
        return false;
    };
    physical_surface_rect_contains_pointer(
        outer,
        native.window.surface_position(),
        native.window.surface_size(),
        pointer,
    )
}

fn native_window_surface_at_logical_position_contains_pointer(
    window: &Arc<dyn Window>,
    position: cranpose_ui::Point,
    pointer: PhysicalPosition<f64>,
) -> bool {
    let outer = LogicalPosition::new(position.x as f64, position.y as f64)
        .to_physical::<i32>(window.scale_factor());
    physical_surface_rect_contains_pointer(
        outer,
        window.surface_position(),
        window.surface_size(),
        pointer,
    )
}

fn physical_surface_rect_contains_pointer(
    outer: PhysicalPosition<i32>,
    surface: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    pointer: PhysicalPosition<f64>,
) -> bool {
    let x = outer.x as f64 + surface.x as f64;
    let y = outer.y as f64 + surface.y as f64;
    let right = x + size.width as f64;
    let bottom = y + size.height as f64;
    pointer.x >= x && pointer.x <= right && pointer.y >= y && pointer.y <= bottom
}

#[derive(Clone, Copy, Debug)]
struct NativeWindowPointerState {
    position: PhysicalPosition<f64>,
    primary_down: bool,
}

#[cfg(all(target_os = "linux", not(target_arch = "wasm32")))]
struct X11WindowClient {
    connection: x11rb::rust_connection::RustConnection,
    root: u32,
}

#[cfg(all(target_os = "linux", not(target_arch = "wasm32")))]
enum X11WindowClientState {
    Available(Box<X11WindowClient>),
    Unavailable,
}

#[cfg(all(target_os = "linux", not(target_arch = "wasm32")))]
thread_local! {
    static X11_WINDOW_CLIENT: RefCell<Option<X11WindowClientState>> = const { RefCell::new(None) };
}

#[cfg(all(target_os = "linux", not(target_arch = "wasm32")))]
impl X11WindowClient {
    fn connect() -> Option<Self> {
        use x11rb::connection::Connection;

        let (connection, screen_num) = x11rb::connect(None).ok()?;
        let root = connection.setup().roots.get(screen_num)?.root;
        Some(Self { connection, root })
    }

    fn pointer_state(&self) -> Option<NativeWindowPointerState> {
        use x11rb::protocol::xproto::{ConnectionExt, KeyButMask};

        let reply = self
            .connection
            .query_pointer(self.root)
            .ok()?
            .reply()
            .ok()?;
        let mask = u16::from(reply.mask);
        Some(NativeWindowPointerState {
            position: PhysicalPosition::new(reply.root_x as f64, reply.root_y as f64),
            primary_down: mask & u16::from(KeyButMask::BUTTON1) != 0,
        })
    }

    fn configure_window(&self, window: u32, position: PhysicalPosition<i32>) -> Option<()> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{ConfigureWindowAux, ConnectionExt};

        self.connection
            .configure_window(
                window,
                &ConfigureWindowAux::new().x(position.x).y(position.y),
            )
            .ok()?;
        self.connection.flush().ok()?;
        Some(())
    }

    fn window_position(&self, window: u32) -> Option<PhysicalPosition<i32>> {
        use x11rb::protocol::xproto::ConnectionExt;

        let reply = self
            .connection
            .translate_coordinates(window, self.root, 0, 0)
            .ok()?
            .reply()
            .ok()?;
        Some(PhysicalPosition::new(
            reply.dst_x as i32,
            reply.dst_y as i32,
        ))
    }
}

#[cfg(all(target_os = "linux", not(target_arch = "wasm32")))]
fn with_x11_window_client<R>(f: impl FnOnce(&X11WindowClient) -> R) -> Option<R> {
    X11_WINDOW_CLIENT.with(|slot| {
        if slot.borrow().is_none() {
            *slot.borrow_mut() = Some(
                X11WindowClient::connect()
                    .map(Box::new)
                    .map(X11WindowClientState::Available)
                    .unwrap_or(X11WindowClientState::Unavailable),
            );
        }

        match slot.borrow().as_ref()? {
            X11WindowClientState::Available(client) => Some(f(client)),
            X11WindowClientState::Unavailable => None,
        }
    })
}

#[cfg(all(target_os = "linux", not(target_arch = "wasm32")))]
fn native_window_global_pointer_state() -> Option<NativeWindowPointerState> {
    with_x11_window_client(X11WindowClient::pointer_state).flatten()
}

#[cfg(not(all(target_os = "linux", not(target_arch = "wasm32"))))]
fn native_window_global_pointer_state() -> Option<NativeWindowPointerState> {
    None
}

#[cfg(all(target_os = "linux", not(target_arch = "wasm32")))]
fn native_window_x11_id(window: &Arc<dyn Window>) -> Option<u32> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Xlib(handle) => Some(handle.window as u32),
        RawWindowHandle::Xcb(handle) => Some(handle.window.get()),
        _ => None,
    }
}

#[cfg(all(target_os = "linux", not(target_arch = "wasm32")))]
fn native_window_x11_outer_position_physical(
    window: &Arc<dyn Window>,
) -> Option<PhysicalPosition<i32>> {
    let window_id = native_window_x11_id(window)?;
    with_x11_window_client(|client| client.window_position(window_id)).flatten()
}

#[cfg(not(all(target_os = "linux", not(target_arch = "wasm32"))))]
fn native_window_x11_outer_position_physical(
    _window: &Arc<dyn Window>,
) -> Option<PhysicalPosition<i32>> {
    None
}

#[cfg(all(target_os = "linux", not(target_arch = "wasm32")))]
fn native_window_set_outer_position_physical(
    window: &Arc<dyn Window>,
    position: PhysicalPosition<i32>,
) -> bool {
    let Some(window_id) = native_window_x11_id(window) else {
        return false;
    };
    with_x11_window_client(|client| client.configure_window(window_id, position).is_some())
        .unwrap_or(false)
}

#[cfg(not(all(target_os = "linux", not(target_arch = "wasm32"))))]
fn native_window_set_outer_position_physical(
    _window: &Arc<dyn Window>,
    _position: PhysicalPosition<i32>,
) -> bool {
    false
}

fn update_native_options_position(options: &mut NativeWindowOptions, x: f32, y: f32) {
    options.x = Some(x);
    options.y = Some(y);
    options.position_origin = NativeWindowPositionOrigin::Screen;
}

fn update_native_options_size(
    options: &mut NativeWindowOptions,
    window: &Arc<dyn Window>,
    width: u32,
    height: u32,
) {
    let scale_factor = window.scale_factor() as f32;
    if scale_factor > 0.0 {
        options.width = width.max(1) as f32 / scale_factor;
        options.height = height.max(1) as f32 / scale_factor;
    }
}

fn sync_native_window_state_position(
    state: Option<WindowState>,
    previous_position: Option<cranpose_ui::Point>,
    x: f32,
    y: f32,
) {
    let Some(state) = state else {
        return;
    };
    if state.position_non_reactive() == previous_position {
        state.set_position(Some(cranpose_ui::Point::new(x, y)));
    }
}

fn sync_native_window_state_size(
    state: Option<WindowState>,
    previous_size: Option<cranpose_ui::Size>,
    window: &Arc<dyn Window>,
    width: u32,
    height: u32,
) {
    let Some(state) = state else {
        return;
    };
    let Some(previous_size) = previous_size else {
        return;
    };
    if state.size_non_reactive() != previous_size {
        return;
    }
    let scale_factor = window.scale_factor() as f32;
    if scale_factor > 0.0 {
        state.set_size(cranpose_ui::Size::new(
            width.max(1) as f32 / scale_factor,
            height.max(1) as f32 / scale_factor,
        ));
    }
}

fn trace_native_window_timing(args: std::fmt::Arguments<'_>) {
    if std::env::var_os("CRANPOSE_NATIVE_WINDOW_TIMING").is_some() {
        println!("native window timing: {args}");
    }
}

fn trace_native_window(args: std::fmt::Arguments<'_>) {
    if std::env::var_os("CRANPOSE_NATIVE_TRACE").is_some() {
        println!("native window trace: {args}");
    }
}

fn primary_surface_redraw_drives_app(primary_window_visible: bool, headless: bool) -> bool {
    primary_window_visible && !headless
}

fn primary_frame_waker_uses_event_proxy(primary_window_visible: bool, headless: bool) -> bool {
    !primary_surface_redraw_drives_app(primary_window_visible, headless)
}

fn primary_declaration_host_needs_direct_update(
    primary_window_visible: bool,
    headless: bool,
    needs_redraw: bool,
    waiting_for_frame_cap: bool,
) -> bool {
    needs_redraw
        && !waiting_for_frame_cap
        && !primary_surface_redraw_drives_app(primary_window_visible, headless)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PointerButtonFrameRequest {
    request_redraw: bool,
    reset_frame_cap: bool,
}

fn pointer_button_frame_request(input_handled: bool) -> PointerButtonFrameRequest {
    PointerButtonFrameRequest {
        request_redraw: input_handled,
        reset_frame_cap: input_handled,
    }
}

fn apply_pointer_button_frame_request(
    window: &Arc<dyn Window>,
    last_frame_start_time: &mut Option<Instant>,
    request: PointerButtonFrameRequest,
) {
    if request.reset_frame_cap {
        *last_frame_start_time = None;
    }
    if request.request_redraw {
        window.request_redraw();
    }
}

fn configure_app_surface_size(
    app: &mut AppShell<WgpuRenderer>,
    surface: &wgpu::Surface<'static>,
    surface_config: &mut wgpu::SurfaceConfiguration,
    width: u32,
    height: u32,
    viewport: (f32, f32),
) {
    if width == 0 || height == 0 {
        return;
    }

    surface_config.width = width;
    surface_config.height = height;
    let device = app.renderer().device();
    surface.configure(device, surface_config);
    update_app_viewport(app, width, height, viewport);
}

fn update_app_viewport(
    app: &mut AppShell<WgpuRenderer>,
    width: u32,
    height: u32,
    viewport: (f32, f32),
) {
    let (logical_width, logical_height) = viewport;
    app.set_buffer_size(width, height);
    app.set_viewport(logical_width, logical_height);
}

fn surface_logical_viewport_size(width: u32, height: u32, scale_factor: f64) -> (f32, f32) {
    (
        width as f32 / scale_factor as f32,
        height as f32 / scale_factor as f32,
    )
}

fn headless_requested_viewport(settings: &AppSettings) -> Option<(f32, f32)> {
    settings.headless.then_some((
        settings.initial_width.max(1) as f32,
        settings.initial_height.max(1) as f32,
    ))
}

fn viewport_for_surface_size(
    requested_viewport: Option<(f32, f32)>,
    width: u32,
    height: u32,
    scale_factor: f64,
) -> (f32, f32) {
    requested_viewport.unwrap_or_else(|| surface_logical_viewport_size(width, height, scale_factor))
}

fn primary_viewport_for_surface_size(
    settings: &AppSettings,
    width: u32,
    height: u32,
    scale_factor: f64,
) -> (f32, f32) {
    viewport_for_surface_size(
        headless_requested_viewport(settings),
        width,
        height,
        scale_factor,
    )
}

fn update_app_scale_factor(
    app: &mut AppShell<WgpuRenderer>,
    platform: &mut DesktopWinitPlatform,
    scale_factor: f64,
) {
    platform.set_scale_factor(scale_factor);
    app.renderer().set_root_scale(scale_factor as f32);
    cranpose_ui::set_density(scale_factor as f32);
}

fn dispatch_mouse_wheel(
    app: &mut AppShell<WgpuRenderer>,
    platform: &DesktopWinitPlatform,
    current_modifiers: winit::keyboard::ModifiersState,
    cursor_position: Option<(f32, f32)>,
    delta: winit::event::MouseScrollDelta,
) {
    if let Some((x, y)) = cursor_position {
        app.set_cursor(x, y);
    }

    let mut logical_delta = platform.scroll_delta(delta);
    let alt_pressed = current_modifiers.contains(winit::keyboard::ModifiersState::ALT);
    if alt_pressed {
        if logical_delta.x.abs() <= f32::EPSILON {
            logical_delta.x = logical_delta.y;
        }
        logical_delta.y = 0.0;
    }

    log::trace!(
        target: "cranpose::input",
        "desktop wheel delta ({:.2},{:.2}) alt={}",
        logical_delta.x,
        logical_delta.y,
        alt_pressed
    );

    app.pointer_scrolled(logical_delta.x, logical_delta.y);
}

fn dispatch_middle_click_paste(
    app: &mut AppShell<WgpuRenderer>,
    cursor_position: Option<(f32, f32)>,
) {
    if let Some((x, y)) = cursor_position {
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

fn cancel_app_input(app: &mut AppShell<WgpuRenderer>) {
    app.cancel_gesture();
    let _ = app.on_ime_preedit("", None);
}

fn logical_outer_position(window: &Arc<dyn Window>) -> Option<(f32, f32)> {
    window.outer_position().ok().map(|position| {
        let logical = position.to_logical::<f64>(window.scale_factor());
        (logical.x as f32, logical.y as f32)
    })
}

fn notify_native_window_moved(events: &NativeWindowEvents, x: f32, y: f32) {
    if let Some(on_moved) = &events.on_moved {
        on_moved(x, y);
    }
}

fn notify_native_window_resized(
    events: &NativeWindowEvents,
    window: &Arc<dyn Window>,
    width: u32,
    height: u32,
) {
    if let Some(on_resized) = &events.on_resized {
        let scale_factor = window.scale_factor() as f32;
        if scale_factor > 0.0 {
            on_resized(width as f32 / scale_factor, height as f32 / scale_factor);
        }
    }
}

fn notify_native_window_close_requested(events: &NativeWindowEvents) {
    if let Some(on_close_requested) = &events.on_close_requested {
        on_close_requested();
    }
}

fn native_resize_direction(direction: WindowResizeDirection) -> ResizeDirection {
    match direction {
        WindowResizeDirection::East => ResizeDirection::East,
        WindowResizeDirection::North => ResizeDirection::North,
        WindowResizeDirection::NorthEast => ResizeDirection::NorthEast,
        WindowResizeDirection::NorthWest => ResizeDirection::NorthWest,
        WindowResizeDirection::South => ResizeDirection::South,
        WindowResizeDirection::SouthEast => ResizeDirection::SouthEast,
        WindowResizeDirection::SouthWest => ResizeDirection::SouthWest,
        WindowResizeDirection::West => ResizeDirection::West,
    }
}

fn default_vsync_interval() -> Duration {
    Duration::from_nanos(16_666_667)
}

fn monitor_refresh_interval(window: &Arc<dyn Window>) -> Duration {
    window
        .current_monitor()
        .and_then(|monitor| monitor.current_video_mode())
        .and_then(|mode| mode.refresh_rate_millihertz())
        .map(|millihertz| {
            let nanos = 1_000_000_000_000u64 / u64::from(millihertz.get());
            Duration::from_nanos(nanos)
        })
        .unwrap_or_else(default_vsync_interval)
}

fn dispatch_keyboard_input(
    app: &mut AppShell<WgpuRenderer>,
    current_modifiers: winit::keyboard::ModifiersState,
    event: winit::event::KeyEvent,
) {
    use cranpose_app_shell::{KeyEvent, KeyEventType};
    use winit::keyboard::Key;

    let event_type = match event.state {
        ElementState::Pressed => KeyEventType::KeyDown,
        ElementState::Released => KeyEventType::KeyUp,
    };
    let text = match &event.logical_key {
        Key::Character(s) => s.to_string(),
        _ => String::new(),
    };
    let key_code = app_key_code(event.physical_key);
    let key_event = KeyEvent::new(key_code, text, app_modifiers(current_modifiers), event_type);

    if key_code == cranpose_app_shell::KeyCode::D && event_type == KeyEventType::KeyDown {
        app.log_debug_info();
    }

    app.on_key_event(&key_event);
}

fn app_key_code(physical_key: winit::keyboard::PhysicalKey) -> cranpose_app_shell::KeyCode {
    use cranpose_app_shell::KeyCode;
    use winit::keyboard::PhysicalKey;

    match physical_key {
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
    }
}

fn app_modifiers(
    current_modifiers: winit::keyboard::ModifiersState,
) -> cranpose_app_shell::Modifiers {
    cranpose_app_shell::Modifiers {
        shift: current_modifiers.contains(winit::keyboard::ModifiersState::SHIFT),
        ctrl: current_modifiers.contains(winit::keyboard::ModifiersState::CONTROL),
        alt: current_modifiers.contains(winit::keyboard::ModifiersState::ALT),
        meta: current_modifiers.contains(winit::keyboard::ModifiersState::META),
    }
}

fn dispatch_ime_event(app: &mut AppShell<WgpuRenderer>, ime_event: winit::event::Ime) {
    use winit::event::Ime;

    match ime_event {
        Ime::Preedit(text, cursor) => {
            app.on_ime_preedit(&text, cursor);
        }
        Ime::Commit(text) => {
            let _ = app.on_ime_preedit("", None);
            app.on_paste(&text);
        }
        Ime::Enabled => {}
        Ime::Disabled => {
            app.on_ime_preedit("", None);
        }
        Ime::DeleteSurrounding { .. } => {}
    }
}

impl ApplicationHandler for App {
    fn proxy_wake_up(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.handle_primary_frame_requested(event_loop);
    }

    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        // Create window if not already created
        if self.window.is_some() {
            return;
        }

        let initial_width = self.settings.initial_width;
        let initial_height = self.settings.initial_height;
        let headless = self.settings.headless;
        let primary_window_visible = self.settings.primary_window_visible;

        let window: Arc<dyn Window> = match event_loop.create_window(
            WindowAttributes::default()
                .with_title(self.settings.window_title.clone())
                .with_surface_size(LogicalSize::new(
                    initial_width as f64,
                    initial_height as f64,
                ))
                // Hide window in headless mode for parallel robot testing
                .with_visible(!headless && primary_window_visible),
        ) {
            Ok(window) => window.into(),
            Err(error) => {
                self.abort_launch(event_loop, LaunchError::WindowCreate(error));
                return;
            }
        };

        // Initialize WGPU
        let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_descriptor.backends = wgpu::Backends::all();
        let instance = wgpu::Instance::new(instance_descriptor);

        let surface = match instance.create_surface(window.clone()) {
            Ok(surface) => surface,
            Err(error) => {
                self.abort_launch(event_loop, LaunchError::SurfaceCreate(error));
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
                    self.abort_launch(event_loop, LaunchError::NoAdapter(error));
                    return;
                }
            };
        let adapter_info = adapter.get_info();
        self.vsync_interval = monitor_refresh_interval(&window);

        let (device, queue) =
            match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("Main Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })) {
                Ok(pair) => pair,
                Err(error) => {
                    self.abort_launch(event_loop, LaunchError::DeviceCreate(error));
                    return;
                }
            };

        let size = window.surface_size();
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = select_surface_format(&surface_caps);

        let present_mode = desktop_present_mode(&surface_caps, self.frame_pacing_mode);
        let surface_config = surface_config_for_window(
            &surface_caps,
            surface_format,
            size.width.max(1),
            size.height.max(1),
            present_mode,
            false,
            self.frame_pacing_mode,
        );

        let device = Arc::new(device);
        let queue = Arc::new(queue);

        surface.configure(&device, &surface_config);

        // Create renderer with fonts from settings
        let fonts: &[&[u8]] = self.settings.fonts.unwrap_or(&[]);
        let text_system = WgpuTextSystem::from_fonts(fonts);
        let initial_scale = window.scale_factor();
        let renderer = wgpu_renderer_for_surface(
            text_system.clone(),
            Arc::clone(&device),
            Arc::clone(&queue),
            surface_format,
            adapter_info.backend,
            initial_scale,
        );
        cranpose_ui::set_density(initial_scale as f32);

        let viewport = primary_viewport_for_surface_size(
            &self.settings,
            size.width,
            size.height,
            initial_scale,
        );

        // Take the content closure (can only be called once)
        let content = self.content.take().expect("content already taken");
        let mut app = AppShell::new_with_size(
            renderer,
            default_root_key(),
            content,
            (size.width, size.height),
            viewport,
        );
        #[cfg(feature = "robot")]
        app.set_semantics_enabled(self.robot_controller.is_some());

        // Apply dev options (FPS counter, etc.)
        let mut dev_options = self.settings.dev_options.clone();
        dev_options.frame_pacing_mode = self.frame_pacing_mode;
        app.set_dev_options(dev_options);

        // Runtime-driven frame scheduling: Compose invalidations and animations
        // request frames via the runtime waker, not per-input host redraw forcing.
        let frame_waker_window = window.clone();
        let frame_waker_event_proxy = self.event_proxy.clone();
        let use_event_proxy = primary_frame_waker_uses_event_proxy(
            self.settings.primary_window_visible,
            self.settings.headless,
        );
        app.set_frame_waker(move || {
            if use_event_proxy {
                frame_waker_event_proxy.wake_up();
            } else {
                frame_waker_window.request_redraw();
            }
        });

        let mut platform = DesktopWinitPlatform::default();
        platform.set_scale_factor(initial_scale);

        self.window = Some(window);
        self.surface = Some(surface);
        self.surface_config = Some(surface_config);
        self.surface_caps = Some(surface_caps);
        self.app = Some(app);
        self.platform = Some(platform);
        self.gpu_context = Some(DesktopGpuContext {
            instance,
            adapter,
            adapter_backend: adapter_info.backend,
            device,
            queue,
            text_system,
        });
        self.refresh_native_window_requests();
        self.sync_native_windows(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: WinitWindowId,
        event: WindowEvent,
    ) {
        let Some(window) = &self.window else {
            self.native_window_event(event_loop, window_id, event);
            return;
        };
        if window_id != window.id() {
            self.native_window_event(event_loop, window_id, event);
            return;
        }

        let frame_interval = self.frame_interval();
        let last_frame_start_time = self.last_frame_start_time;
        let frame_cap_deadline = last_frame_start_time
            .and_then(|started_at| frame_interval.map(|interval| started_at + interval));

        let primary_viewport_override = headless_requested_viewport(&self.settings);

        let Some(app) = &mut self.app else { return };
        let Some(platform) = &mut self.platform else {
            return;
        };
        let Some(surface) = &self.surface else { return };
        let Some(surface_config) = &mut self.surface_config else {
            return;
        };

        let mut sync_native_windows_after_event = false;
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
                let viewport = viewport_for_surface_size(
                    primary_viewport_override,
                    new_size.width,
                    new_size.height,
                    window.scale_factor(),
                );
                configure_app_surface_size(
                    app,
                    surface,
                    surface_config,
                    new_size.width,
                    new_size.height,
                    viewport,
                );
            }
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                mut surface_size_writer,
            } => {
                update_app_scale_factor(app, platform, scale_factor);

                let new_size = window.surface_size();
                let _ = surface_size_writer.request_surface_size(new_size);
                if new_size.width > 0 && new_size.height > 0 {
                    let viewport = viewport_for_surface_size(
                        primary_viewport_override,
                        new_size.width,
                        new_size.height,
                        window.scale_factor(),
                    );
                    configure_app_surface_size(
                        app,
                        surface,
                        surface_config,
                        new_size.width,
                        new_size.height,
                        viewport,
                    );
                }
            }
            WindowEvent::Moved(_) => {
                self.vsync_interval = monitor_refresh_interval(window);
            }
            WindowEvent::PointerMoved { position, .. } => {
                let logical = platform.pointer_position(position);
                self.last_cursor_position = Some((logical.x, logical.y));
                log::trace!(
                    target: "cranpose::input",
                    "desktop pointer move ({:.2},{:.2})",
                    logical.x,
                    logical.y
                );
                if app.set_cursor(logical.x, logical.y) {
                    window.request_redraw();
                }
                if let Some(recorder) = &mut self.recorder {
                    recorder.record_mouse_move(logical.x, logical.y);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                // Track current keyboard modifiers for key events
                self.current_modifiers = modifiers.state();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                dispatch_mouse_wheel(
                    app,
                    platform,
                    self.current_modifiers,
                    self.last_cursor_position,
                    delta,
                );
            }
            WindowEvent::PointerButton {
                state,
                button: ButtonSource::Mouse(MouseButton::Left),
                ..
            } => {
                let cursor_position = self.last_cursor_position;
                if let Some((x, y)) = self.last_cursor_position {
                    log::trace!(
                        target: "cranpose::input",
                        "desktop pointer button {:?} at ({:.2},{:.2})",
                        state,
                        x,
                        y
                    );
                    if app.set_cursor(x, y) {
                        window.request_redraw();
                    }
                }
                match state {
                    ElementState::Pressed => {
                        if let Some((x, y)) = cursor_position {
                            if let Some(mode) = app.handle_dev_overlay_click(x, y) {
                                apply_frame_pacing_mode(
                                    app,
                                    surface,
                                    surface_config,
                                    self.surface_caps.as_ref(),
                                    mode,
                                );
                                self.frame_pacing_mode = mode;
                                self.last_frame_start_time = None;
                                window.request_redraw();
                                for native in self.native_windows.values_mut() {
                                    apply_frame_pacing_mode(
                                        &mut native.app,
                                        &native.surface,
                                        &mut native.surface_config,
                                        Some(&native.surface_caps),
                                        mode,
                                    );
                                    native.frame_pacing_mode = mode;
                                    native.last_frame_start_time = None;
                                    native.window.request_redraw();
                                }
                                return;
                            }
                        }
                        let request = pointer_button_frame_request(app.pointer_pressed());
                        apply_pointer_button_frame_request(
                            window,
                            &mut self.last_frame_start_time,
                            request,
                        );
                        if let Some(recorder) = &mut self.recorder {
                            recorder.record_mouse_down();
                        }
                    }
                    ElementState::Released => {
                        let request = pointer_button_frame_request(app.pointer_released());
                        app.sync_selection_to_primary();
                        apply_pointer_button_frame_request(
                            window,
                            &mut self.last_frame_start_time,
                            request,
                        );
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
                dispatch_middle_click_paste(app, self.last_cursor_position);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                dispatch_keyboard_input(app, self.current_modifiers, event);
            }
            WindowEvent::Focused(false) => {
                cancel_app_input(app);
            }
            WindowEvent::Ime(ime_event) => {
                dispatch_ime_event(app, ime_event);
            }
            WindowEvent::PointerLeft { .. } => {
                app.cancel_gesture();
            }
            WindowEvent::RedrawRequested => {
                if let Some(deadline) = frame_cap_deadline {
                    if deadline > Instant::now() {
                        event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
                        return;
                    }
                }
                log::trace!(target: "cranpose::input", "desktop redraw requested");
                let frame_started_at = Instant::now();
                cranpose_ui::set_density(window.scale_factor() as f32);
                app.update();
                sync_native_windows_after_event = true;

                let output = match current_surface_texture(surface, "primary window") {
                    SurfaceFrame::Ready(output) => output,
                    SurfaceFrame::Reconfigure => {
                        let size = window.surface_size();
                        let viewport = viewport_for_surface_size(
                            primary_viewport_override,
                            size.width,
                            size.height,
                            window.scale_factor(),
                        );
                        configure_app_surface_size(
                            app,
                            surface,
                            surface_config,
                            size.width,
                            size.height,
                            viewport,
                        );
                        return;
                    }
                    SurfaceFrame::Skip => {
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
                self.last_frame_start_time = Some(frame_started_at);
            }
            _ => {}
        }

        if sync_native_windows_after_event {
            self.refresh_native_window_requests();
            self.sync_native_windows(event_loop);
        }
    }

    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        let now = Instant::now();
        if self.poll_active_native_window_drags(now) {
            self.refresh_native_window_requests();
            self.sync_native_windows(event_loop);
        }
        if self.poll_external_native_window_moves(now) {
            self.refresh_native_window_requests();
            self.sync_native_windows(event_loop);
        }

        let frame_interval = self.frame_interval();
        let last_frame_start_time = self.last_frame_start_time;
        let Some(app) = &mut self.app else { return };
        let Some(window) = self.window.clone() else {
            return;
        };

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
                    RobotCommand::DriverPanicked(message) => {
                        self.abort_launch(event_loop, LaunchError::TestDriverPanic(message));
                        return;
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
                    RobotCommand::PumpFrames { count } => {
                        for _ in 0..count {
                            app.update_after_frame_interval(ROBOT_PUMP_FRAME_INTERVAL);
                        }
                        let _ = controller.tx.send(RobotResponse::Ok);
                    }
                    RobotCommand::Exit => {
                        let _ = controller.tx.send(RobotResponse::Ok);
                        event_loop.exit();
                    }
                }
            }

            // Handle ongoing wait_for_idle
            if controller.waiting_for_idle {
                const MAX_IDLE_ITERATIONS: u32 = 600;

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
                        let _ = controller.tx.send(RobotResponse::Error(format!(
                            "wait_for_idle: timed out after {} iterations",
                            MAX_IDLE_ITERATIONS
                        )));
                    }
                }
            }
        }

        let has_active_animations = app.has_active_animations();
        let needs_redraw = app.needs_redraw() || has_active_animations;
        if needs_redraw {
            log::trace!(
                target: "cranpose::input",
                "about_to_wait needs_redraw={needs_redraw}"
            );
        }
        let next_frame_time = last_frame_start_time
            .and_then(|started_at| frame_interval.map(|interval| started_at + interval));
        let waiting_for_frame_cap =
            needs_redraw && next_frame_time.is_some_and(|deadline| deadline > now);
        let direct_declaration_update = primary_declaration_host_needs_direct_update(
            self.settings.primary_window_visible,
            self.settings.headless,
            needs_redraw,
            waiting_for_frame_cap,
        );
        if needs_redraw && !waiting_for_frame_cap {
            if direct_declaration_update {
                trace_native_window(format_args!(
                    "primary declaration host direct update visible={} headless={}",
                    self.settings.primary_window_visible, self.settings.headless
                ));
                app.update();
            } else {
                window.request_redraw();
            }
        }
        let primary_next_event_time = app.next_event_time();
        if direct_declaration_update {
            self.sync_native_windows(event_loop);
        }

        let mut native_has_active_animations = false;
        let mut native_drag_deadline: Option<Instant> = None;
        let native_position_poll_deadline = self
            .native_windows
            .values()
            .any(|native| native.options.visible && native.active_drag.is_none())
            .then_some(self.next_native_window_position_poll_at);
        let mut native_frame_cap_deadline: Option<Instant> = None;
        let mut native_next_event_time: Option<Instant> = None;
        for native in self.native_windows.values_mut() {
            if !native.options.visible {
                continue;
            }

            let has_active_animations = native.app.has_active_animations();
            native_has_active_animations |= has_active_animations;
            let needs_redraw = native.app.needs_redraw() || has_active_animations;
            let next_frame_time = native.last_frame_start_time.and_then(|started_at| {
                native
                    .frame_interval()
                    .map(|interval| started_at + interval)
            });
            let waiting_for_frame_cap =
                needs_redraw && next_frame_time.is_some_and(|deadline| deadline > now);

            if needs_redraw && !waiting_for_frame_cap {
                native.window.request_redraw();
            }
            if let Some(active_drag) = native.active_drag {
                let next_poll_at = active_drag.next_poll_at();
                native_drag_deadline = Some(
                    native_drag_deadline
                        .map(|current| current.min(next_poll_at))
                        .unwrap_or(next_poll_at),
                );
            }
            if waiting_for_frame_cap {
                let deadline = next_frame_time.expect("native frame cap deadline should exist");
                native_frame_cap_deadline = Some(
                    native_frame_cap_deadline
                        .map(|current| current.min(deadline))
                        .unwrap_or(deadline),
                );
            }
            if let Some(next_time) = native.app.next_event_time() {
                native_next_event_time = Some(
                    native_next_event_time
                        .map(|current| current.min(next_time))
                        .unwrap_or(next_time),
                );
            }
        }

        // Smart ControlFlow: only Poll when necessary
        #[cfg(feature = "robot")]
        let robot_needs_poll = self.robot_controller.is_some();

        #[cfg(not(feature = "robot"))]
        let robot_needs_poll = false;

        // Poll continuously when:
        // - Active animations are running
        // - Robot test is active
        if robot_needs_poll
            || native_drag_deadline.is_some()
            || native_position_poll_deadline.is_some_and(|deadline| deadline <= now)
        {
            event_loop.set_control_flow(ControlFlow::Poll);
        } else if let Some(deadline) = [
            next_frame_time.filter(|_| waiting_for_frame_cap),
            native_frame_cap_deadline,
            native_position_poll_deadline,
        ]
        .into_iter()
        .flatten()
        .min()
        {
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        } else if has_active_animations || native_has_active_animations {
            event_loop.set_control_flow(ControlFlow::Poll);
        } else if let Some(next_time) = [primary_next_event_time, native_next_event_time]
            .into_iter()
            .flatten()
            .min()
        {
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
pub fn try_run(
    mut settings: AppSettings,
    content: impl FnMut() + 'static,
) -> Result<(), LaunchError> {
    native_window::clear_native_window_requests();
    let event_loop = EventLoop::builder()
        .build()
        .map_err(LaunchError::EventLoopCreate)?;
    let event_proxy = event_loop.create_proxy();
    let launch_error = Rc::new(RefCell::new(None));

    // Spawn test driver if present
    #[cfg(feature = "robot")]
    let robot_controller = if let Some(driver) = settings.test_driver.take() {
        let (controller, robot) = RobotController::new();
        let panic_tx = robot.tx.clone();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                driver(robot);
            }));
            if let Err(payload) = result {
                let _ = panic_tx.send(RobotCommand::DriverPanicked(panic_payload_message(payload)));
            }
        });
        Some(controller)
    } else {
        None
    };

    let mut app = App::new(settings, content, Rc::clone(&launch_error), event_proxy);

    #[cfg(feature = "robot")]
    if let Some(controller) = robot_controller {
        app.set_robot_controller(controller);
    }

    let run_result = event_loop.run_app(app);
    native_window::clear_native_window_requests();
    if let Some(error) = launch_error.borrow_mut().take() {
        return Err(error);
    }

    run_result.map_err(LaunchError::EventLoopRun)
}

#[cfg(feature = "robot")]
fn panic_payload_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

/// Runs a desktop application and exits the process on success.
///
/// Use [`try_run`] when the caller needs to handle launch failures explicitly.
#[allow(unused_mut)]
pub fn run(settings: AppSettings, content: impl FnMut() + 'static) -> ! {
    try_run(settings, content)
        .unwrap_or_else(|error| panic!("failed to launch desktop app: {error}"));
    std::process::exit(0)
}

#[cfg(feature = "robot")]
fn capture_screenshot(app: &mut AppShell<WgpuRenderer>) -> Result<RobotScreenshot, String> {
    let logical_size = app.viewport_size();
    let (width, height, capture_scale) =
        resolve_robot_screenshot_params(app.buffer_size(), Some(logical_size));

    let captured = app
        .renderer()
        .capture_frame_with_scale(width, height, capture_scale)
        .map_err(|err| format!("Failed to capture GPU screenshot: {err:?}"))?;

    let (logical_width, logical_height) = logical_size;

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
    let (logical_width, logical_height) = app.viewport_size();
    let width = (logical_width * scale).ceil().max(1.0) as u32;
    let height = (logical_height * scale).ceil().max(1.0) as u32;

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
    log::trace!(
        target: "cranpose::input",
        "find_text query={query:?} result={:?}",
        result
            .as_ref()
            .map(|result| (result.node_id, result.bounds, result.text.clone()))
    );
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
    log::trace!(
        target: "cranpose::input",
        "find_button query={query:?} result={:?}",
        result
            .as_ref()
            .map(|result| (result.node_id, result.bounds, result.text.clone()))
    );
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
    use super::{
        clamp_rect_to_monitor_delta, native_window_graph_position, nearest_monitor_to_rect,
        physical_surface_rect_contains_pointer, pointer_button_frame_request,
        primary_declaration_host_needs_direct_update, primary_frame_waker_uses_event_proxy,
        primary_surface_redraw_drives_app, primary_viewport_for_surface_size, App, DesktopRect,
        NativeWindowDragSession, NativeWindowGraphPositionSource, NativeWindowOptions,
        NativeWindowPollingDragSession, NativeWindowPositionOrigin, PendingNativeWindowPositions,
    };
    use crate::launcher::AppSettings;
    use std::time::Instant;
    use winit::dpi::{PhysicalPosition, PhysicalSize};

    #[cfg(feature = "robot")]
    use super::{
        find_button_in_trees, find_text_in_trees, panic_payload_message,
        resolve_robot_screenshot_params, semantic_element_from_semantics_node,
        subtree_contains_matching_text, SemanticRect, SemanticTextMatchKind,
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

    #[test]
    fn native_window_screen_position_is_declarative() {
        let options = NativeWindowOptions::new("child", 100.0, 50.0).with_position(10.0, 20.0);
        assert!(App::native_window_options_have_screen_position(&options));
    }

    #[test]
    fn native_window_host_position_needs_resolution() {
        let options =
            NativeWindowOptions::new("child", 100.0, 50.0).with_host_window_position(10.0, 20.0);
        assert_eq!(
            options.position_origin,
            NativeWindowPositionOrigin::HostWindow
        );
        assert!(!App::native_window_options_have_screen_position(&options));
    }

    #[test]
    fn native_window_initial_position_prefers_declaration_over_early_os_position() {
        let options = NativeWindowOptions::new("child", 100.0, 50.0).with_position(10.0, 20.0);

        assert_eq!(
            App::initial_native_window_position(&options, Some((0.0, 0.0))),
            Some((10.0, 20.0))
        );
    }

    #[test]
    fn native_window_initial_position_uses_os_position_without_declaration() {
        let options = NativeWindowOptions::new("child", 100.0, 50.0);

        assert_eq!(
            App::initial_native_window_position(&options, Some((30.0, 40.0))),
            Some((30.0, 40.0))
        );
    }

    #[test]
    fn native_window_group_bounds_move_from_virtual_gap_to_nearest_monitor() {
        let monitors = [
            DesktopRect {
                x: 0.0,
                y: 630.0,
                width: 1420.0,
                height: 800.0,
            },
            DesktopRect {
                x: 1920.0,
                y: 0.0,
                width: 3840.0,
                height: 2160.0,
            },
        ];
        let group_bounds = DesktopRect {
            x: 140.0,
            y: 120.0,
            width: 550.0,
            height: 319.0,
        };

        let monitor = nearest_monitor_to_rect(&monitors, group_bounds);
        let delta = clamp_rect_to_monitor_delta(group_bounds, monitor, 32.0);

        assert_eq!(monitor, monitors[0]);
        assert_eq!(delta, cranpose_ui::Point::new(0.0, 542.0));
    }

    #[test]
    fn native_window_group_bounds_preserve_visible_position() {
        let monitor = DesktopRect {
            x: 0.0,
            y: 630.0,
            width: 1420.0,
            height: 800.0,
        };
        let group_bounds = DesktopRect {
            x: 140.0,
            y: 700.0,
            width: 550.0,
            height: 319.0,
        };

        let delta = clamp_rect_to_monitor_delta(group_bounds, monitor, 32.0);

        assert_eq!(delta, cranpose_ui::Point::new(0.0, 0.0));
    }

    #[test]
    fn visible_primary_surface_drives_redraw_updates() {
        assert!(primary_surface_redraw_drives_app(true, false));
        assert!(!primary_surface_redraw_drives_app(false, false));
        assert!(!primary_surface_redraw_drives_app(true, true));
    }

    #[test]
    fn hidden_primary_frame_waker_uses_event_loop_proxy() {
        assert!(!primary_frame_waker_uses_event_proxy(true, false));
        assert!(primary_frame_waker_uses_event_proxy(false, false));
        assert!(primary_frame_waker_uses_event_proxy(true, true));
    }

    #[test]
    fn hidden_primary_declaration_host_updates_without_redraw_event() {
        assert!(primary_declaration_host_needs_direct_update(
            false, false, true, false
        ));
        assert!(primary_declaration_host_needs_direct_update(
            true, true, true, false
        ));
        assert!(!primary_declaration_host_needs_direct_update(
            true, false, true, false
        ));
        assert!(!primary_declaration_host_needs_direct_update(
            false, false, true, true
        ));
        assert!(!primary_declaration_host_needs_direct_update(
            false, false, false, false
        ));
    }

    #[test]
    fn pointer_button_input_requests_uncapped_frame_when_handled() {
        let request = pointer_button_frame_request(true);

        assert!(request.request_redraw);
        assert!(request.reset_frame_cap);
        assert_eq!(
            pointer_button_frame_request(false),
            super::PointerButtonFrameRequest {
                request_redraw: false,
                reset_frame_cap: false,
            }
        );
    }

    #[test]
    fn pending_native_window_positions_acknowledge_stale_programmatic_moves() {
        let mut pending = PendingNativeWindowPositions::default();
        pending.push((100.0, 200.0));
        pending.push((140.0, 230.0));

        assert!(pending.acknowledge((100.0, 200.0)));
        assert!(pending.acknowledge((140.0, 230.0)));
        assert!(!pending.acknowledge((190.0, 260.0)));
    }

    #[test]
    fn pending_native_window_positions_match_fractional_window_manager_rounding() {
        let mut pending = PendingNativeWindowPositions::default();
        pending.push((100.4, 200.4));

        assert!(pending.acknowledge((101.0, 201.0)));
        assert!(!pending.acknowledge((101.0, 201.0)));
    }

    #[test]
    fn pending_native_window_positions_can_be_cleared_after_external_move() {
        let mut pending = PendingNativeWindowPositions::default();
        pending.push((100.0, 200.0));
        pending.push((140.0, 240.0));

        pending.clear();

        assert!(!pending.acknowledge((100.0, 200.0)));
        assert!(!pending.acknowledge((140.0, 240.0)));
    }

    #[test]
    fn pending_native_window_positions_report_unacknowledged_programmatic_moves() {
        let mut pending = PendingNativeWindowPositions::default();

        assert!(!pending.has_pending());
        pending.push((100.0, 200.0));
        assert!(pending.has_pending());
        assert!(pending.acknowledge((100.0, 200.0)));
        assert!(!pending.has_pending());
    }

    #[test]
    fn native_window_graph_position_keeps_cache_first_for_programmatic_moves() {
        let position = native_window_graph_position(
            None,
            Some((100.0, 200.0)),
            Some((140.0, 240.0)),
            Some((160.0, 260.0)),
            NativeWindowGraphPositionSource::CachedThenCurrent,
        );

        assert_eq!(position, Some(cranpose_ui::Point::new(100.0, 200.0)));
    }

    #[test]
    fn native_window_graph_position_uses_current_position_for_external_moves() {
        let position = native_window_graph_position(
            None,
            Some((100.0, 200.0)),
            Some((140.0, 240.0)),
            Some((160.0, 260.0)),
            NativeWindowGraphPositionSource::CurrentThenCached,
        );

        assert_eq!(position, Some(cranpose_ui::Point::new(140.0, 240.0)));
    }

    #[test]
    fn native_window_graph_position_override_wins_over_position_source() {
        let position = native_window_graph_position(
            Some(cranpose_ui::Point::new(80.0, 90.0)),
            Some((100.0, 200.0)),
            Some((140.0, 240.0)),
            Some((160.0, 260.0)),
            NativeWindowGraphPositionSource::CurrentThenCached,
        );

        assert_eq!(position, Some(cranpose_ui::Point::new(80.0, 90.0)));
    }

    #[test]
    fn native_window_polling_drag_target_is_anchored_to_drag_start() {
        let session = NativeWindowPollingDragSession::new(
            PhysicalPosition::new(100.0, 50.0),
            PhysicalPosition::new(300, 200),
            Instant::now(),
        );

        assert_eq!(
            session.target_for_pointer(PhysicalPosition::new(112.0, 57.0)),
            PhysicalPosition::new(312, 207)
        );
        assert_eq!(
            session.target_for_pointer(PhysicalPosition::new(120.0, 50.0)),
            PhysicalPosition::new(320, 200)
        );
    }

    #[test]
    fn native_window_polling_drag_target_does_not_accumulate_window_manager_lag() {
        let session = NativeWindowPollingDragSession::new(
            PhysicalPosition::new(100.0, 50.0),
            PhysicalPosition::new(300, 200),
            Instant::now(),
        );

        let first_target = session.target_for_pointer(PhysicalPosition::new(112.0, 50.0));
        let second_target = session.target_for_pointer(PhysicalPosition::new(120.0, 50.0));

        assert_eq!(first_target, PhysicalPosition::new(312, 200));
        assert_eq!(
            second_target,
            PhysicalPosition::new(320, 200),
            "the target must be based on the drag start, not on the last reported window position"
        );
    }

    #[test]
    fn native_window_polling_drag_uses_local_release_event() {
        let now = Instant::now();

        assert!(
            !NativeWindowDragSession::Polling(NativeWindowPollingDragSession::new(
                PhysicalPosition::new(100.0, 50.0),
                PhysicalPosition::new(300, 200),
                now,
            ))
            .finishes_on_global_pointer_release()
        );
        assert!(NativeWindowDragSession::platform(now).finishes_on_global_pointer_release());
    }

    #[test]
    fn inferred_native_drag_requires_pointer_over_surface() {
        let outer = PhysicalPosition::new(300, 200);
        let surface = PhysicalPosition::new(8, 28);
        let size = PhysicalSize::new(120, 60);

        assert!(physical_surface_rect_contains_pointer(
            outer,
            surface,
            size,
            PhysicalPosition::new(320.0, 240.0)
        ));
        assert!(!physical_surface_rect_contains_pointer(
            outer,
            surface,
            size,
            PhysicalPosition::new(299.0, 240.0)
        ));
    }

    #[cfg(feature = "robot")]
    #[test]
    fn robot_screenshot_prefers_logical_viewport_size() {
        let resolved = resolve_robot_screenshot_params((1600, 1200), Some((800.0, 600.0)));
        assert_eq!(resolved, (800, 600, 1.0));
    }

    #[cfg(feature = "robot")]
    #[test]
    fn robot_screenshot_uses_ceil_on_fractional_logical_size() {
        let resolved = resolve_robot_screenshot_params((0, 0), Some((801.2, 601.3)));
        assert_eq!(resolved, (802, 602, 1.0));
    }

    #[test]
    fn headless_primary_viewport_uses_requested_launcher_size() {
        let settings = AppSettings {
            initial_width: 1600,
            initial_height: 900,
            headless: true,
            ..AppSettings::default()
        };

        let viewport = primary_viewport_for_surface_size(&settings, 1601, 901, 1.0);

        assert_eq!(viewport, (1600.0, 900.0));
    }

    #[test]
    fn visible_primary_viewport_uses_actual_surface_size() {
        let settings = AppSettings {
            initial_width: 1600,
            initial_height: 900,
            headless: false,
            ..AppSettings::default()
        };

        let viewport = primary_viewport_for_surface_size(&settings, 1601, 901, 2.0);

        assert_eq!(viewport, (800.5, 450.5));
    }

    #[cfg(feature = "robot")]
    #[test]
    fn robot_screenshot_falls_back_to_physical_buffer_when_layout_is_missing() {
        let resolved = resolve_robot_screenshot_params((1600, 1200), None);
        assert_eq!(resolved, (1600, 1200, 1.0));
    }

    #[cfg(feature = "robot")]
    #[test]
    fn robot_driver_panic_payload_formats_static_str() {
        assert_eq!(
            panic_payload_message(Box::new("driver failed")),
            "driver failed"
        );
    }

    #[cfg(feature = "robot")]
    #[test]
    fn robot_driver_panic_payload_formats_string() {
        assert_eq!(
            panic_payload_message(Box::new(String::from("driver failed"))),
            "driver failed"
        );
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
