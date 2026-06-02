#![deny(unsafe_code)]

pub mod app;
pub mod fonts;

#[cfg(test)]
mod tests;

pub mod test_screens;

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use crate::fonts::DEMO_FONTS;
#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use cranpose::AppLauncher;

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn create_app() -> AppLauncher {
    AppLauncher::new()
        .with_title("Cranpose Demo")
        .with_size(800, 600)
        .with_fonts(DEMO_FONTS)
        .with_fps_counter(true)
        .with_frame_pacing_controls(true)
}

/// Shared entry point for desktop
#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub fn entry_point() {
    #[cfg(feature = "logging")]
    let _ = env_logger::try_init();
    if let Err(error) = create_app().try_run(app::combined_app) {
        eprintln!("Failed to launch Cranpose Demo: {error}");
        std::process::exit(1);
    }
}
