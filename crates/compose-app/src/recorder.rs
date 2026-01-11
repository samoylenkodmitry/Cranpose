//! Input event recorder for generating robot tests from manual interactions.
//!
//! This module captures mouse and keyboard events with precise timestamps,
//! then generates Rust robot test code that can replay the exact interaction.
//!
//! # Example
//!
//! ```no_run
//! use compose_app::AppLauncher;
//!
//! AppLauncher::new()
//!     .with_recording("/tmp/my_test.rs")
//!     .run(|| {
//!         // Your app - interact with it, then close
//!     });
//! // Generated test file will be at /tmp/my_test.rs
//! ```

use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

/// A recorded input event with timestamp
#[derive(Debug, Clone)]
pub enum RecordedEvent {
    /// Mouse cursor moved
    MouseMove {
        /// Timestamp in milliseconds since recording started
        time_ms: u64,
        /// X coordinate in logical pixels
        x: f32,
        /// Y coordinate in logical pixels
        y: f32,
    },
    /// Left mouse button pressed
    MouseDown {
        /// Timestamp in milliseconds since recording started
        time_ms: u64,
    },
    /// Left mouse button released
    MouseUp {
        /// Timestamp in milliseconds since recording started
        time_ms: u64,
    },
    /// Key pressed
    KeyDown {
        /// Timestamp in milliseconds since recording started
        time_ms: u64,
        /// Key name
        key: String,
    },
    /// Key released
    KeyUp {
        /// Timestamp in milliseconds since recording started
        time_ms: u64,
        /// Key name
        key: String,
    },
}

/// Input recorder that captures events with timestamps
pub struct InputRecorder {
    /// When recording started
    start_time: Instant,
    /// All recorded events
    events: Vec<RecordedEvent>,
    /// Output file path
    output_path: PathBuf,
    /// Last mouse position (to avoid duplicate moves)
    last_mouse_pos: Option<(f32, f32)>,
}

impl InputRecorder {
    /// Create a new recorder that will save to the given path
    pub fn new(output_path: impl Into<PathBuf>) -> Self {
        let path = output_path.into();
        eprintln!("[Recorder] Recording started - will save to {:?}", path);
        Self {
            start_time: Instant::now(),
            events: Vec::new(),
            output_path: path,
            last_mouse_pos: None,
        }
    }

    /// Get elapsed time in milliseconds since recording started
    fn elapsed_ms(&self) -> u64 {
        self.start_time.elapsed().as_millis() as u64
    }

    /// Record a mouse move event
    pub fn record_mouse_move(&mut self, x: f32, y: f32) {
        // Skip if same position (within 0.5px)
        if let Some((lx, ly)) = self.last_mouse_pos {
            if (x - lx).abs() < 0.5 && (y - ly).abs() < 0.5 {
                return;
            }
        }
        self.last_mouse_pos = Some((x, y));
        let time_ms = self.elapsed_ms();
        self.events.push(RecordedEvent::MouseMove { time_ms, x, y });
    }

    /// Record a mouse down event
    pub fn record_mouse_down(&mut self) {
        let time_ms = self.elapsed_ms();
        self.events.push(RecordedEvent::MouseDown { time_ms });
    }

    /// Record a mouse up event
    pub fn record_mouse_up(&mut self) {
        let time_ms = self.elapsed_ms();
        self.events.push(RecordedEvent::MouseUp { time_ms });
    }

    /// Record a key press event
    pub fn record_key_down(&mut self, key: &str) {
        let time_ms = self.elapsed_ms();
        self.events.push(RecordedEvent::KeyDown {
            time_ms,
            key: key.to_string(),
        });
    }

    /// Record a key release event
    pub fn record_key_up(&mut self, key: &str) {
        let time_ms = self.elapsed_ms();
        self.events.push(RecordedEvent::KeyUp {
            time_ms,
            key: key.to_string(),
        });
    }

    /// Finish recording and generate the robot test file
    pub fn finish(&self) -> std::io::Result<()> {
        if self.events.is_empty() {
            eprintln!("[Recorder] No events recorded, skipping file generation");
            return Ok(());
        }

        eprintln!(
            "[Recorder] Generating robot test with {} events to {:?}",
            self.events.len(),
            self.output_path
        );

        let mut file = std::fs::File::create(&self.output_path)?;

        // Write header
        writeln!(file, "//! Auto-generated robot test from recording")?;
        writeln!(file, "//! Generated at: {}", chrono_lite())?;
        writeln!(file, "//! Events: {}", self.events.len())?;
        writeln!(file)?;
        writeln!(file, "use compose_app::AppLauncher;")?;
        writeln!(file, "use std::time::Duration;")?;
        writeln!(file)?;
        writeln!(file, "fn main() {{")?;
        writeln!(file, "    AppLauncher::new()")?;
        writeln!(file, "        .with_headless(true)")?;
        writeln!(file, "        .with_test_driver(|robot| {{")?;
        writeln!(
            file,
            "            std::thread::sleep(Duration::from_millis(500));"
        )?;
        writeln!(file, "            let _ = robot.wait_for_idle();")?;
        writeln!(file)?;

        let mut last_time_ms = 0u64;

        for event in &self.events {
            let (time_ms, code) = match event {
                RecordedEvent::MouseMove { time_ms, x, y } => (
                    *time_ms,
                    format!("            let _ = robot.mouse_move({:.1}, {:.1});", x, y),
                ),
                RecordedEvent::MouseDown { time_ms } => (
                    *time_ms,
                    "            let _ = robot.mouse_down();".to_string(),
                ),
                RecordedEvent::MouseUp { time_ms } => (
                    *time_ms,
                    "            let _ = robot.mouse_up();".to_string(),
                ),
                RecordedEvent::KeyDown { time_ms, key } => (
                    *time_ms,
                    format!("            let _ = robot.send_key(\"{}\");", key),
                ),
                RecordedEvent::KeyUp { time_ms: _, key: _ } => {
                    // Skip key up for now - send_key does press+release
                    continue;
                }
            };

            // Add sleep for timing
            let delta = time_ms.saturating_sub(last_time_ms);
            if delta > 5 {
                writeln!(
                    file,
                    "            std::thread::sleep(Duration::from_millis({}));",
                    delta
                )?;
            }

            writeln!(file, "{}", code)?;
            last_time_ms = time_ms;
        }

        writeln!(file)?;
        writeln!(
            file,
            "            std::thread::sleep(Duration::from_secs(1));"
        )?;
        writeln!(file, "            let _ = robot.exit();")?;
        writeln!(file, "        }})")?;
        writeln!(file, "        .run(|| {{")?;
        writeln!(
            file,
            "            // TODO: Replace with your app's composable"
        )?;
        writeln!(file, "            // desktop_app::app::combined_app();")?;
        writeln!(file, "        }});")?;
        writeln!(file, "}}")?;

        eprintln!("[Recorder] Robot test saved to {:?}", self.output_path);
        Ok(())
    }
}

impl Drop for InputRecorder {
    fn drop(&mut self) {
        if let Err(e) = self.finish() {
            eprintln!("[Recorder] Failed to save recording: {}", e);
        }
    }
}

/// Simple timestamp without chrono
fn chrono_lite() -> String {
    // Just use a placeholder - actual time would require chrono crate
    "timestamp".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_recorder_generates_code() {
        let temp_path = std::env::temp_dir().join("test_recording.rs");
        {
            let mut recorder = InputRecorder::new(&temp_path);
            recorder.record_mouse_move(100.0, 200.0);
            recorder.record_mouse_down();
            recorder.record_mouse_up();
            // finish() called on drop
        }

        let mut content = String::new();
        std::fs::File::open(&temp_path)
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();

        assert!(content.contains("robot.mouse_move(100.0, 200.0)"));
        assert!(content.contains("robot.mouse_down()"));
        assert!(content.contains("robot.mouse_up()"));

        std::fs::remove_file(&temp_path).ok();
    }
}
