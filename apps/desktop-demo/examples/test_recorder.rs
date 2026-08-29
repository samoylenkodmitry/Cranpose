use std::path::PathBuf;

use cranpose::AppLauncher;
use desktop_app::app;

fn main() {
    let recording_path = PathBuf::from("absent_gradient_area_recording.rs");

    println!("=== Recorder Test ===");
    println!("Recording to: {:?}", recording_path);
    println!("Interact with the app, then close it.");
    println!("The recording will be saved automatically.\n");

    if let Err(error) = AppLauncher::new()
        .with_title("Recorder Test")
        .with_size(800, 600)
        .with_recording(&recording_path)
        .try_run(|| {
            app::combined_app();
        })
    {
        eprintln!("Failed to launch recorder example: {error}");
        std::process::exit(1);
    }
}
