//! Whether the browser gives the page a WebGPU adapter, asked as the page
//! loads (`web_webgpu_ask.js`, which the app's module imports before it
//! fetches its WebAssembly). In Chrome on a Mate 20 X, which has no adapter
//! to give, asking once the module ran kept the page from making its WebGL
//! context for 20-30 ms on every start.

use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen(module = "/src/web_webgpu_ask.js")]
extern "C" {
    #[wasm_bindgen(js_name = cranposeWebGpuPresent)]
    fn webgpu_present_answer() -> js_sys::Promise;
}

/// Whether the browser gave an adapter when the page loaded.
pub(crate) async fn webgpu_present() -> bool {
    wasm_bindgen_futures::JsFuture::from(webgpu_present_answer())
        .await
        .ok()
        .and_then(|answer| answer.as_bool())
        .unwrap_or(false)
}
