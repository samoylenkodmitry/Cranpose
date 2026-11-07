//! The exact GLSL of the WebGL stages the page's first screen compiles,
//! which a build with first-screen records fetches from beside its wasm
//! (`first_screen_glsl.txt`, see `cranpose_render_wgpu::use_first_screen_glsl`).
//!
//! A page that will draw on WebGL starts the fetch as the browser loads the
//! wasm-bindgen glue, which imports this snippet before it fetches the
//! wasm, so the file comes in beside the far larger wasm: one asked for
//! WebGL in its query (`backend_preference`), or one whose browser has no
//! `navigator.gpu`. Where the browser has `navigator.gpu`, only an adapter
//! request tells, and asking for one as the glue loaded put Chrome's first
//! frame on a Mate 20 X (Android 10, `navigator.gpu` but no adapter) 29 ms
//! later, the median of 8 starts. Such a page starts the fetch once its own
//! detection found no adapter ([`auto_instance`]), which main makes at the
//! same moment. A page that will draw on WebGPU leaves the file alone. The
//! renderer takes the file if it came in by the time it starts and never
//! waits for it. The snippet sits in `snippets/<crate>/`, two folders below
//! the wasm's.

use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen(inline_js = r#"
let file;
let fetching = false;
export function fetchFirstScreenGlsl() {
    if (fetching) return;
    fetching = true;
    fetch(new URL("../../first_screen_glsl.txt", import.meta.url))
        .then((response) => (response.ok ? response.arrayBuffer() : null))
        .catch(() => null)
        .then((fetched) => { file = fetched; });
}
const backend = new URLSearchParams(location.search).get("backend");
if (backend === "gl" || (backend !== "webgpu" && !navigator.gpu)) fetchFirstScreenGlsl();
export function firstScreenGlsl() {
    return file;
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = fetchFirstScreenGlsl)]
    fn fetch_file();

    // The file once fetched, `null` where it did not load, `undefined` while
    // the fetch runs or where the page did not start one.
    #[wasm_bindgen(js_name = firstScreenGlsl)]
    fn fetched() -> wasm_bindgen::JsValue;
}

// Hands the renderer the page's first-screen GLSL where it came in.
pub(super) fn take() {
    let file = fetched();
    if file.is_undefined() {
        return;
    }
    if file.is_null() {
        log::warn!("[first-screen-glsl] first_screen_glsl.txt did not load");
        return;
    }
    let file = js_sys::Uint8Array::new(&file).to_vec();
    if let Err(error) = cranpose_render_wgpu::use_first_screen_glsl(file) {
        log::warn!("[first-screen-glsl] {error}");
    }
}

// The instance of a page left to pick its backend, as
// `wgpu::util::new_instance_with_webgpu_detection` makes it: one that finds
// no WebGPU adapter draws on WebGL, and starts fetching the file then.
pub(super) async fn auto_instance(mut descriptor: wgpu::InstanceDescriptor) -> wgpu::Instance {
    if !wgpu::util::is_browser_webgpu_supported().await {
        descriptor.backends.remove(wgpu::Backends::BROWSER_WEBGPU);
        fetch_file();
    }
    wgpu::Instance::new(descriptor)
}
