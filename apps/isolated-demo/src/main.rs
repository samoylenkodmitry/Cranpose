#![deny(unsafe_code)]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod fonts;
mod screens;
mod theme;

fn main() {
    #[cfg(feature = "logging")]
    let _ = env_logger::try_init();
    if let Err(error) = app::create_app().try_run(app::IsolatedDemoApp) {
        eprintln!("Failed to launch Cranpose Isolated Demo: {error}");
        std::process::exit(1);
    }
}
