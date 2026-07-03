//! Renderer-agnostic robot testing harness shared by the desktop shells.
//!
//! The [`Robot`] handle, its command/response protocol, the semantic-tree
//! extraction, and the screenshot payload live here so both the wgpu desktop
//! shell (`desktop.rs`) and the slim Vulkan desktop shell (`desktop_slim.rs`)
//! drive the exact same test surface. Each shell owns the event-loop side of
//! the channel and translates commands against its own presentation model; the
//! renderer-neutral pieces are gathered in this module to avoid duplicating the
//! protocol per shell.

use std::any::Any;
use std::collections::HashMap;
use std::sync::mpsc;

use cranpose_app_shell::{AppShell, KeyCode, RuntimeLeakDebugStats};
use cranpose_render_common::Renderer;
use cranpose_ui::{SemanticsAction, SemanticsNode, SemanticsRole};

#[cfg(feature = "renderer-wgpu")]
use cranpose_render_wgpu::{DebugCpuAllocationStats, RenderStatsSnapshot};

/// Serializable semantic element combining semantics + geometry
///
/// This structure combines semantic information (role, text, actions) with
/// geometric bounds from the layout tree, enabling robot scripts to find
/// and interact with UI elements by their semantic properties.
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
    /// Whether this element represents editable text.
    pub editable_text: bool,
    /// Text selection range as UTF-8 byte offsets.
    pub text_selection: Option<(usize, usize)>,
    /// Child semantic elements
    pub children: Vec<SemanticElement>,
}

/// Geometric bounds for a semantic element
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SemanticTextMatchKind {
    Contains,
    Exact,
    Prefix,
}

#[derive(Debug, Clone)]
pub(crate) struct SemanticQueryResult {
    pub(crate) node_id: cranpose_core::NodeId,
    pub(crate) bounds: SemanticRect,
    pub(crate) text: Option<String>,
}

pub(crate) type TextMatchBounds = (f32, f32, f32, f32, String);

/// RGBA screenshot captured from the current render scene.
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
#[derive(Debug)]
pub(crate) enum RobotCommand {
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
    MouseScrollAndWaitForFrame {
        delta_x: f32,
        delta_y: f32,
    },
    MouseScrollSequenceAndWaitForFrames {
        delta_x: f32,
        delta_y: f32,
        count: u32,
    },
    TouchDown {
        x: f32,
        y: f32,
    },
    TouchMove {
        x: f32,
        y: f32,
    },
    TouchMoveAndWaitForFrame {
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
    WaitForPresentFrame,
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
    #[cfg(feature = "renderer-wgpu")]
    GetRenderStats,
    GetFpsStats,
    ResetFpsStats,
    GetLastFlingVelocity,
    ResetLastFlingVelocity,
    #[cfg(feature = "renderer-wgpu")]
    GetRenderCpuAllocationStats,
    GetRuntimeLeakDebugStats,
    MeasureText {
        text: String,
        style: Box<cranpose_ui::text::TextStyle>,
    },
    HasFocusedTextField,
    SetSemanticsEnabled(bool),
    InvokeAppHook {
        name: String,
        argument: String,
    },
    DriverPanicked(String),
    Exit,
}

/// Robot response
#[derive(Debug)]
pub(crate) enum RobotResponse {
    Ok,
    Semantics(Vec<SemanticElement>),
    SemanticQuery(Option<SemanticQueryResult>),
    Screenshot(RobotScreenshot),
    #[cfg(feature = "renderer-wgpu")]
    RenderStats(Box<Option<RenderStatsSnapshot>>),
    FpsStats(cranpose_app_shell::FpsStats),
    F32(f32),
    #[cfg(feature = "renderer-wgpu")]
    RenderCpuAllocationStats(Box<DebugCpuAllocationStats>),
    RuntimeLeakDebugStats(Box<RuntimeLeakDebugStats>),
    TextMetrics(cranpose_ui::text::TextMetrics),
    Bool(bool),
    AppHookResult(Option<String>),
    Error(String),
}

/// Event-loop side of the robot channel: the shell receives commands on `rx`
/// and answers on `tx`. Created together with its paired [`Robot`] handle.
pub(crate) struct RobotChannel {
    pub(crate) rx: mpsc::Receiver<RobotCommand>,
    pub(crate) tx: mpsc::Sender<RobotResponse>,
}

impl RobotChannel {
    pub(crate) fn new() -> (Self, Robot) {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (resp_tx, resp_rx) = mpsc::channel();

        let channel = RobotChannel {
            rx: cmd_rx,
            tx: resp_tx,
        };

        let robot = Robot {
            tx: cmd_tx,
            rx: resp_rx,
        };

        (channel, robot)
    }
}

/// Robot handle for test drivers
pub struct Robot {
    tx: mpsc::Sender<RobotCommand>,
    rx: mpsc::Receiver<RobotResponse>,
}

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

    /// Dispatch a mouse wheel / trackpad scroll delta and wait for the frame it caused.
    pub fn mouse_scroll_and_wait_for_frame(
        &self,
        delta_x: f32,
        delta_y: f32,
    ) -> Result<(), String> {
        self.tx
            .send(RobotCommand::MouseScrollAndWaitForFrame { delta_x, delta_y })
            .map_err(|e| format!("Failed to send mouse scroll command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Ok) => Ok(()),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Dispatch a sequence of mouse wheel / trackpad scroll deltas, advancing
    /// only after each caused frame is presented.
    pub fn mouse_scroll_sequence_and_wait_for_frames(
        &self,
        delta_x: f32,
        delta_y: f32,
        count: u32,
    ) -> Result<(), String> {
        self.tx
            .send(RobotCommand::MouseScrollSequenceAndWaitForFrames {
                delta_x,
                delta_y,
                count,
            })
            .map_err(|e| format!("Failed to send mouse scroll sequence command: {}", e))?;
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

    /// Perform a drag gesture and wait for each intermediate move to present.
    ///
    /// This is intended for performance contracts where every drag step must be
    /// measured as visible window output.
    pub fn drag_and_wait_for_frames(
        &self,
        from_x: f32,
        from_y: f32,
        to_x: f32,
        to_y: f32,
        steps: u32,
    ) -> Result<(), String> {
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

        let steps = steps.max(1);
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let x = from_x + (to_x - from_x) * t;
            let y = from_y + (to_y - from_y) * t;

            self.tx
                .send(RobotCommand::TouchMoveAndWaitForFrame { x, y })
                .map_err(|e| format!("Failed to send touch move: {}", e))?;
            match self.rx.recv() {
                Ok(RobotResponse::Ok) => {}
                Ok(RobotResponse::Error(e)) => return Err(e),
                Ok(_) => return Err("Unexpected response".to_string()),
                Err(e) => return Err(format!("Failed to receive response: {}", e)),
            }
        }

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

    /// Wait for the visible primary surface to present one more frame.
    pub fn wait_for_present_frame(&self) -> Result<(), String> {
        self.tx
            .send(RobotCommand::WaitForPresentFrame)
            .map_err(|e| format!("Failed to send present-frame wait command: {}", e))?;
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
    #[cfg(feature = "renderer-wgpu")]
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

    /// Get a snapshot of the owning app shell's FPS monitor.
    pub fn fps_stats(&self) -> Result<cranpose_app_shell::FpsStats, String> {
        self.tx
            .send(RobotCommand::GetFpsStats)
            .map_err(|e| format!("Failed to send FPS stats command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::FpsStats(stats)) => Ok(stats),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Reset the owning app shell's FPS monitor.
    pub fn reset_fps_stats(&self) -> Result<(), String> {
        self.tx
            .send(RobotCommand::ResetFpsStats)
            .map_err(|e| format!("Failed to send FPS stats reset command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Ok) => Ok(()),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Query the owning app context's latest recorded fling velocity.
    pub fn last_fling_velocity(&self) -> Result<f32, String> {
        self.tx
            .send(RobotCommand::GetLastFlingVelocity)
            .map_err(|e| format!("Failed to send fling velocity command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::F32(value)) => Ok(value),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Reset the owning app context's recorded fling velocity.
    pub fn reset_last_fling_velocity(&self) -> Result<(), String> {
        self.tx
            .send(RobotCommand::ResetLastFlingVelocity)
            .map_err(|e| format!("Failed to send fling velocity reset command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Ok) => Ok(()),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Get a snapshot of CPU-side renderer allocation capacities.
    #[cfg(feature = "renderer-wgpu")]
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

    /// Measure text on the app thread using the active shell text service.
    pub fn measure_text(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
    ) -> Result<cranpose_ui::text::TextMetrics, String> {
        self.tx
            .send(RobotCommand::MeasureText {
                text: text.text.clone(),
                style: Box::new(style.clone()),
            })
            .map_err(|e| format!("Failed to send measure_text command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::TextMetrics(metrics)) => Ok(metrics),
            Ok(RobotResponse::Error(e)) => Err(e),
            Ok(_) => Err("Unexpected response".to_string()),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }

    /// Query focused text-field state on the app thread.
    pub fn has_focused_text_field(&self) -> Result<bool, String> {
        self.tx
            .send(RobotCommand::HasFocusedTextField)
            .map_err(|e| format!("Failed to send focus query command: {}", e))?;
        match self.rx.recv() {
            Ok(RobotResponse::Bool(value)) => Ok(value),
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

    /// Clone of the command channel. Shells use this to surface a panicking
    /// test driver back into the event loop as a `DriverPanicked` command.
    pub(crate) fn command_sender(&self) -> mpsc::Sender<RobotCommand> {
        self.tx.clone()
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
                    format_args!(
                        "{prefix}role={} bounds=({:.1},{:.1},{:.1},{:.1}){}{}\n",
                        elem.role,
                        elem.bounds.x,
                        elem.bounds.y,
                        elem.bounds.width,
                        elem.bounds.height,
                        text_info,
                        clickable
                    ),
                );
                format_semantics_into(output, &elem.children, indent + 1);
            }
        }

        let mut output = String::new();
        format_semantics_into(&mut output, elements, indent);
        output
    }
}

/// Format the payload of a caught test-driver panic into a readable message.
pub(crate) fn panic_payload_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

/// Whether `wait_for_idle` may finish while a continuous animation keeps the
/// shell producing frames. Robot idle waits must not hang on infinite
/// animation loops once the structure has settled and at least one clean
/// visual frame has been observed.
pub(crate) fn robot_wait_for_idle_animation_loop_only(
    has_active_animations: bool,
    waiting_for_present: bool,
    idle_iterations: u32,
    idle_structure_clean_frames: u32,
) -> bool {
    has_active_animations
        && !waiting_for_present
        && idle_iterations > 0
        && idle_structure_clean_frames > 0
}

/// Map a robot key name (e.g. "Up", "Return", "a") to a shell `KeyCode` and
/// the character text it produces. Shared by every shell so synthetic key
/// events behave identically across renderers.
pub(crate) fn robot_key_code_and_text(key: &str) -> (KeyCode, String) {
    match key {
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
    }
}

/// Map a character to a KeyCode for robot typing
pub(crate) fn char_to_key_code(ch: char) -> KeyCode {
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

/// Extract semantic elements by combining semantic tree with an on-demand layout snapshot.
pub(crate) fn extract_semantics<R>(app: &mut AppShell<R>) -> Vec<SemanticElement>
where
    R: Renderer,
    R::Error: std::fmt::Debug,
{
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

pub(crate) fn semantic_element_from_semantics_node<F>(
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
        editable_text: sem_node.editable_text,
        text_selection: sem_node
            .text_selection
            .map(|range| (range.start, range.end)),
        children,
    }
}

pub(crate) fn find_text_in_app<R>(
    app: &mut AppShell<R>,
    query: &str,
    match_kind: SemanticTextMatchKind,
) -> Option<SemanticQueryResult>
where
    R: Renderer,
    R::Error: std::fmt::Debug,
{
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

pub(crate) fn find_button_in_app<R>(
    app: &mut AppShell<R>,
    query: &str,
    match_kind: SemanticTextMatchKind,
) -> Option<SemanticQueryResult>
where
    R: Renderer,
    R::Error: std::fmt::Debug,
{
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

fn semantics_text_matches(actual: &str, query: &str, match_kind: SemanticTextMatchKind) -> bool {
    match match_kind {
        SemanticTextMatchKind::Contains => actual.contains(query),
        SemanticTextMatchKind::Exact => actual == query,
        SemanticTextMatchKind::Prefix => actual.starts_with(query),
    }
}

fn semantics_node_text(sem_node: &SemanticsNode) -> Option<&str> {
    match &sem_node.role {
        SemanticsRole::Text { value } => Some(value.as_str()),
        _ => sem_node.description.as_deref(),
    }
}

fn semantics_node_clickable(sem_node: &SemanticsNode) -> bool {
    sem_node
        .actions
        .iter()
        .any(|action| matches!(action, SemanticsAction::Click { .. }))
}

fn build_semantic_bounds_index(
    root: &cranpose_ui::LayoutBox,
) -> HashMap<cranpose_core::NodeId, SemanticRect> {
    let mut bounds = HashMap::new();
    collect_semantic_bounds(root, &mut bounds);
    bounds
}

fn collect_semantic_bounds(
    layout_box: &cranpose_ui::LayoutBox,
    bounds: &mut HashMap<cranpose_core::NodeId, SemanticRect>,
) {
    bounds.insert(layout_box.node_id, bounds_from_layout_box(layout_box));
    for child in &layout_box.children {
        collect_semantic_bounds(child, bounds);
    }
}

fn bounds_from_layout_box(layout_box: &cranpose_ui::LayoutBox) -> SemanticRect {
    SemanticRect {
        x: layout_box.rect.x,
        y: layout_box.rect.y,
        width: layout_box.rect.width,
        height: layout_box.rect.height,
    }
}

pub(crate) fn subtree_contains_matching_text(
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

#[cfg(test)]
mod tests {
    use super::{
        bounds_from_layout_box, panic_payload_message, robot_wait_for_idle_animation_loop_only,
        semantic_element_from_semantics_node, semantics_node_clickable, semantics_node_text,
        semantics_text_matches, subtree_contains_matching_text, SemanticQueryResult, SemanticRect,
        SemanticTextMatchKind,
    };
    use cranpose_core::NodeId;
    use cranpose_ui::{
        LayoutBox, LayoutNodeData, LayoutNodeKind, Modifier, ModifierNodeSlices, Point, Rect,
        ResolvedModifiers, SemanticsAction, SemanticsCallback, SemanticsNode, SemanticsRole,
    };
    use std::rc::Rc;

    fn find_text_in_trees(
        sem_node: &SemanticsNode,
        layout_box: &LayoutBox,
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

    fn find_button_in_trees(
        sem_node: &SemanticsNode,
        layout_box: &LayoutBox,
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
            editable_text: false,
            text_selection: None,
        }
    }

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

    #[test]
    fn robot_idle_wait_animation_loop_only_requires_settled_frames() {
        assert!(robot_wait_for_idle_animation_loop_only(true, false, 1, 1));
        assert!(!robot_wait_for_idle_animation_loop_only(true, true, 1, 1));
        assert!(!robot_wait_for_idle_animation_loop_only(true, false, 0, 1));
        assert!(!robot_wait_for_idle_animation_loop_only(true, false, 1, 0));
        assert!(!robot_wait_for_idle_animation_loop_only(false, false, 1, 1));
    }

    #[test]
    fn robot_driver_panic_payload_formats_static_str() {
        assert_eq!(
            panic_payload_message(Box::new("driver failed")),
            "driver failed"
        );
    }

    #[test]
    fn robot_driver_panic_payload_formats_string() {
        assert_eq!(
            panic_payload_message(Box::new(String::from("driver failed"))),
            "driver failed"
        );
    }

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
