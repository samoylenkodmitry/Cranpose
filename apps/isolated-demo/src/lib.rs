#![deny(unsafe_code)]

#[cfg(any(
    target_os = "android",
    all(feature = "web", target_arch = "wasm32")
))]
mod app;
#[cfg(any(
    target_os = "android",
    all(feature = "web", target_arch = "wasm32")
))]
mod fonts;

#[cfg(target_os = "android")]
mod native_entry;

#[cfg(all(feature = "web", target_arch = "wasm32"))]
use wasm_bindgen::prelude::*;

#[cfg(all(feature = "web", target_arch = "wasm32"))]
#[wasm_bindgen(start)]
pub fn web_init() {
    // Keep wasm console focused on actionable issues; dependency debug traces are too noisy.
    wasm_logger::init(wasm_logger::Config::new(log::Level::Info));
    console_error_panic_hook::set_once();
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
#[wasm_bindgen]
pub async fn run_app() -> Result<(), JsValue> {
    app::create_app()
        .run_web("cranpose-isolated-canvas", app::IsolatedDemoApp)
        .await
}
