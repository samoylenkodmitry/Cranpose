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
        .with_fps_counter(cfg!(debug_assertions))
}

/// Shared entry point for desktop
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
pub fn entry_point() {
    #[cfg(feature = "logging")]
    let _ = env_logger::try_init();
    create_app().run(app::combined_app);
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
pub async fn run_app(initial_tab: Option<String>) -> Result<(), JsValue> {
    log::info!("Initializing Cranpose app...");

    let startup_tab = initial_tab
        .as_deref()
        .and_then(app::DemoTab::from_startup_name);
    if let Some(requested) = initial_tab.as_deref() {
        match startup_tab {
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

    create_app()
        .run_web("cranpose-canvas", move || {
            app::combined_app_with_initial_tab(startup_tab);
        })
        .await
}
