pub mod app;
pub mod fonts;

#[cfg(test)]
mod tests;

pub mod test_screens;

use crate::fonts::DEMO_FONTS;
use cranpose::AppLauncher;

fn create_app() -> AppLauncher {
    AppLauncher::new()
        .with_title("Cranpose Demo")
        .with_size(800, 600)
        .with_fonts(DEMO_FONTS)
        .with_fps_counter(true)
        .with_frame_pacing_controls(true)
}

/// Shared entry point for desktop
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
pub fn entry_point() {
    #[cfg(feature = "logging")]
    let _ = env_logger::try_init();
    if let Err(error) = create_app().try_run(app::combined_app) {
        eprintln!("Failed to launch Cranpose Demo: {error}");
        std::process::exit(1);
    }
}

/// iOS entry point — called from Xcode project's main.m
#[cfg(target_os = "ios")]
#[no_mangle]
pub extern "C" fn ios_main() {
    entry_point();
}

/// Android entry point
#[cfg(target_os = "android")]
#[no_mangle]
pub fn android_main(app: android_activity::AndroidApp) {
    create_app().run(app, app::combined_app);
}

/// Web entry point
#[cfg(all(feature = "web", target_arch = "wasm32"))]
use wasm_bindgen::prelude::*;

#[cfg(all(feature = "web", target_arch = "wasm32"))]
#[wasm_bindgen(start)]
pub fn web_init() {
    // Set up logging
    // Keep wasm console focused on actionable issues; dependency debug traces are too noisy.
    wasm_logger::init(wasm_logger::Config::new(log::Level::Info));
    console_error_panic_hook::set_once();
    log::info!("🚀 BUILD-ID-XYZ123-2047 🚀 Cranpose demo starting in browser...");
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
#[wasm_bindgen]
pub async fn run_app(
    initial_tab: Option<String>,
    initial_shader_section: Option<String>,
) -> Result<(), JsValue> {
    log::info!("Initializing Cranpose app...");

    let requested_tab = initial_tab
        .as_deref()
        .and_then(app::DemoTab::from_startup_name);
    let requested_shader_section = initial_shader_section
        .as_deref()
        .and_then(app::ShaderSection::from_startup_name);
    let startup = app::StartupSelection::from_requested(requested_tab, requested_shader_section);

    if let Some(requested) = initial_tab.as_deref() {
        match requested_tab {
            Some(tab) => {
                log::info!(
                    "Applying startup tab override: requested='{}' resolved='{}'",
                    requested,
                    tab.label()
                );
            }
            None => {
                log::warn!(
                    "Ignoring unknown startup tab override '{}'; continuing with default tab",
                    requested
                );
            }
        }
    }
    if let Some(requested) = initial_shader_section.as_deref() {
        match requested_shader_section {
            Some(section) if startup.initial_shader_section == Some(section) => {
                log::info!(
                    "Applying startup shader section override: requested='{}' resolved='{}'",
                    requested,
                    section.label()
                );
            }
            Some(section) => {
                log::warn!(
                    "Ignoring startup shader section override '{}' ('{}') because the active startup tab is '{}'",
                    requested,
                    section.label(),
                    startup.initial_tab.unwrap_or(app::DemoTab::Counter).label()
                );
            }
            None => {
                log::warn!(
                    "Ignoring unknown startup shader section override '{}'; continuing with default shaders content",
                    requested
                );
            }
        }
    }

    create_app()
        .run_web("cranpose-canvas", move || {
            app::combined_app_with_startup(startup);
        })
        .await
}
