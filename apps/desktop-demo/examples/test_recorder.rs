//! Simple test app to verify recorder functionality
//!
//! Runs the app with recording enabled, records interactions, and then
//! verifies the generated robot test file.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example test_recorder
//! ```

use compose_app::AppLauncher;
use desktop_app::app;
use std::path::PathBuf;

fn main() {
    let recording_path = PathBuf::from("no_fling_recording.rs");

    println!("=== Recorder Test ===");
    println!("Recording to: {:?}", recording_path);
    println!("Interact with the app, then close it.");
    println!("The recording will be saved automatically.\n");

    AppLauncher::new()
        .with_title("Recorder Test")
        .with_size(800, 600)
        .with_recording(&recording_path)
        .run(|| {
            app::combined_app();
        });
}
