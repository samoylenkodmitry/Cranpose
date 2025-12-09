pub mod app;
pub mod fonts;

#[cfg(test)]
mod tests;

use crate::fonts::DEMO_FONTS;
use compose_app::AppLauncher;

fn create_app() -> AppLauncher {
    AppLauncher::new()
        .with_title("Compose Demo")
        .with_size(800, 600)
        .with_fonts(&DEMO_FONTS)
}

/// Shared entry point for desktop
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
pub fn entry_point() {
    #[cfg(feature = "logging")]
    let _ = env_logger::try_init();
    create_app().run(app::combined_app);
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
    wasm_logger::init(wasm_logger::Config::default());
    console_error_panic_hook::set_once();
    log::info!("🚀 BUILD-ID-XYZ123-2047 🚀 Compose demo starting in browser...");
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
#[wasm_bindgen]
pub async fn run_app() -> Result<(), JsValue> {
    log::info!("Initializing Compose app...");

    create_app()
        .run_web("compose-canvas", app::combined_app)
        .await
}
