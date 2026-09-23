//! Runs Coroflow Notes in a browser canvas.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// Installs logging and panic reporting before anything else runs.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn web_init() {
    wasm_logger::init(wasm_logger::Config::new(log::Level::Info));
    console_error_panic_hook::set_once();
}

/// Starts the app in the canvas with id `coroflow-canvas`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub async fn run_app() -> Result<(), JsValue> {
    coroflow_demo::ui::app::create_app()
        .run_web("coroflow-canvas", coroflow_demo::ui::app::CoroflowDemoApp)
        .await
}
