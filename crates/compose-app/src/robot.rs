//! Robot testing infrastructure for controlling the application from tests.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::Duration;

/// Commands sent from the test driver (Robot) to the application.
#[derive(Debug)]
pub enum RobotCommand {
    /// Wait for the application to be idle (no pending layout/effects).
    WaitForIdle,
    /// Find a node matching the given text.
    FindNodeWithText(String),
    /// Perform a touch down event at the given coordinates.
    TouchDown(f32, f32),
    /// Perform a touch move event to the given coordinates.
    TouchMove(f32, f32),
    /// Perform a touch up event at the given coordinates.
    TouchUp(f32, f32),
    /// Get the scroll value of a node (if applicable).
    /// This is a bit hacky for now, assuming we can inspect state by some ID or mechanism.
    /// For the MVP, we might just inspect the semantic tree dump or similar.
    /// Or we can add a specific "GetScrollState" command if we can identify the scroll container.
    GetScrollValue,
    /// Terminate the application.
    Exit,
}

/// Responses sent from the application to the test driver.
#[derive(Debug)]
pub enum RobotResponse {
    /// Command completed successfully.
    Ok,
    /// Command failed with an error message.
    Error(String),
    /// Result of a query (e.g., scroll value).
    Value(String),
}

/// The driver side of the robot. Used by tests to control the app.
pub struct Robot {
    tx: Sender<RobotCommand>,
    rx: Receiver<RobotResponse>,
}

impl Robot {
    /// Create a new Robot driver.
    pub fn new(tx: Sender<RobotCommand>, rx: Receiver<RobotResponse>) -> Self {
        Self { tx, rx }
    }

    /// Wait for the application to become idle.
    pub fn wait_for_idle(&self) -> Result<(), String> {
        self.send_command(RobotCommand::WaitForIdle)
    }

    /// Find a node containing the specified text.
    ///
    /// Returns an error if the node is not found.
    pub fn find_node_with_text(&self, text: &str) -> Result<(), String> {
        self.send_command(RobotCommand::FindNodeWithText(text.to_string()))
    }

    /// Simulate a touch down event at (x, y).
    pub fn touch_down(&self, x: f32, y: f32) -> Result<(), String> {
        self.send_command(RobotCommand::TouchDown(x, y))
    }

    /// Simulate a touch move event to (x, y).
    pub fn touch_move(&self, x: f32, y: f32) -> Result<(), String> {
        self.send_command(RobotCommand::TouchMove(x, y))
    }

    /// Simulate a touch up event at (x, y).
    pub fn touch_up(&self, x: f32, y: f32) -> Result<(), String> {
        self.send_command(RobotCommand::TouchUp(x, y))
    }

    /// Simulate a swipe up gesture (scrolling down).
    pub fn swipe_up(&self, start_x: f32, start_y: f32, distance: f32) -> Result<(), String> {
        self.touch_down(start_x, start_y)?;
        // Simulate a few move events for realism
        let steps = 10;
        for i in 1..=steps {
            let progress = i as f32 / steps as f32;
            let y = start_y - (distance * progress);
            self.touch_move(start_x, y)?;
            thread::sleep(Duration::from_millis(16)); // ~60fps
        }
        self.touch_up(start_x, start_y - distance)?;
        Ok(())
    }

    /// Simulate a swipe left gesture (scrolling right).
    pub fn swipe_left(&self, start_x: f32, start_y: f32, distance: f32) -> Result<(), String> {
        self.touch_down(start_x, start_y)?;
        // Simulate a few move events for realism
        let steps = 10;
        for i in 1..=steps {
            let progress = i as f32 / steps as f32;
            let x = start_x - (distance * progress);
            self.touch_move(x, start_y)?;
            thread::sleep(Duration::from_millis(16)); // ~60fps
        }
        self.touch_up(start_x - distance, start_y)?;
        Ok(())
    }

    /// Exit the application.
    pub fn exit(&self) -> Result<(), String> {
        let _ = self.tx.send(RobotCommand::Exit);
        Ok(())
    }

    fn send_command(&self, cmd: RobotCommand) -> Result<(), String> {
        self.tx.send(cmd).map_err(|e| e.to_string())?;
        match self.rx.recv().map_err(|e| e.to_string())? {
            RobotResponse::Ok => Ok(()),
            RobotResponse::Error(e) => Err(e),
            RobotResponse::Value(v) => {
                println!("Received value: {}", v);
                Ok(())
            }
        }
    }
}

/// The controller side of the robot. Lives in the app's event loop.
pub struct RobotController {
    /// Channel to send responses to the driver.
    pub tx: Sender<RobotResponse>,
    /// Channel to receive commands from the driver.
    pub rx: Receiver<RobotCommand>,
}

impl RobotController {
    /// Create a new RobotController and its corresponding Robot driver.
    pub fn new() -> (Self, Robot) {
        let (cmd_tx, cmd_rx) = channel();
        let (resp_tx, resp_rx) = channel();

        (
            Self {
                tx: resp_tx,
                rx: cmd_rx,
            },
            Robot::new(cmd_tx, resp_rx),
        )
    }
}
