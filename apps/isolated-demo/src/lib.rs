#[cfg(any(target_os = "android", all(feature = "web", target_arch = "wasm32")))]
mod app;
#[cfg(any(target_os = "android", all(feature = "web", target_arch = "wasm32")))]
mod fonts;

#[cfg(target_os = "android")]
#[no_mangle]
pub fn android_main(app_handle: android_activity::AndroidApp) {
    app::create_app().run(app_handle, app::IsolatedDemoApp);
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
use wasm_bindgen::prelude::*;

#[cfg(all(feature = "web", target_arch = "wasm32"))]
#[wasm_bindgen(start)]
pub fn web_init() {
    wasm_logger::init(wasm_logger::Config::default());
    console_error_panic_hook::set_once();
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
#[wasm_bindgen]
pub async fn run_app() -> Result<(), JsValue> {
    app::create_app()
        .run_web("cranpose-isolated-canvas", app::IsolatedDemoApp)
        .await
}
