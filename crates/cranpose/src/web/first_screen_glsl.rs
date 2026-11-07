//! The exact GLSL of the WebGL stages the page's first screen compiles,
//! which a build with first-screen records fetches from beside its wasm
//! (`first_screen_glsl.txt`, see `cranpose_render_wgpu::use_first_screen_glsl`).
//!
//! The fetch starts as the browser loads the wasm-bindgen glue, which
//! imports this snippet before it fetches the wasm, at low priority, so the
//! file comes in after the wasm or beside it and is there when the renderer
//! starts. Every page but one asked for WebGPU in its query fetches it:
//! which backend a page gets takes an adapter request to tell, and one made
//! as the glue loaded put Chrome's first frame on a Mate 20 X (Android 10,
//! `navigator.gpu` but no adapter) 29 ms later, the median of 8 starts. A
//! fetch started after the page's own detection came in after its renderer
//! started there. A page on WebGPU never reads the file, and the browser
//! keeps it in its cache. The renderer never waits for the file. The
//! snippet sits in `snippets/<crate>/`, two folders below the wasm's.

use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen(inline_js = r#"
let file;
if (new URLSearchParams(location.search).get("backend") !== "webgpu") {
    fetch(new URL("../../first_screen_glsl.txt", import.meta.url), { priority: "low" })
        .then((response) => (response.ok ? response.arrayBuffer() : null))
        .catch(() => null)
        .then((fetched) => { file = fetched; });
}
export function firstScreenGlsl() {
    return file;
}
"#)]
extern "C" {
    // The file once fetched, `null` where it did not load, `undefined` while
    // the fetch runs or on a page asked for WebGPU.
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
