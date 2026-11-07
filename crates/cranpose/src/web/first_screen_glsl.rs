//! The exact GLSL of the WebGL stages the page's first screen compiles,
//! which a build with first-screen records fetches from beside its wasm
//! (`first_screen_glsl.txt`, see `cranpose_render_wgpu::use_first_screen_glsl`).
//!
//! The fetch starts as the browser loads the wasm-bindgen glue, which
//! imports this snippet before it fetches the wasm, so the file comes in
//! beside the far larger wasm and is there when the renderer starts. A file
//! that has not come in by then is left out, so the page never waits for
//! it. A page that will draw on WebGPU leaves it alone: one asked for WebGPU
//! in its query (`backend_preference`), or one whose browser gives it a
//! WebGPU adapter, which the page then draws with ([`auto_instance`]).
//! Chrome on a Mate 20 X (Android 10) has `navigator.gpu` but no adapter.
//! The snippet sits in `snippets/<crate>/`, two folders below the wasm's.

use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen(inline_js = r#"
const backend = new URLSearchParams(location.search).get("backend");
const webgpu = backend === "webgpu" ? Promise.resolve(true)
    : backend === "gl" || !navigator.gpu ? Promise.resolve(false)
    : navigator.gpu.requestAdapter().then((adapter) => !!adapter, () => false);
let file;
webgpu
    .then((found) => found
        ? null
        : fetch(new URL("../../first_screen_glsl.txt", import.meta.url))
            .then((response) => (response.ok ? response.arrayBuffer() : null)))
    .catch(() => null)
    .then((fetched) => { file = fetched; });
export function firstScreenWebGpu() {
    return webgpu;
}
export function firstScreenGlsl() {
    return file;
}
"#)]
extern "C" {
    // Whether the browser gave the snippet a WebGPU adapter.
    #[wasm_bindgen(js_name = firstScreenWebGpu)]
    fn webgpu_adapter() -> js_sys::Promise;

    // The file once fetched, `null` where it did not load or the page left it
    // alone, `undefined` while the fetch runs.
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

// A page left to pick its backend draws on WebGPU where the browser gives it
// an adapter. The snippet asked for one as the glue loaded, so the page does
// not ask again, as `wgpu::util::new_instance_with_webgpu_detection` would:
// on Chrome on an M3 Pro a second early request cost the page about 1 ms of
// CPU a frame.
pub(super) async fn auto_instance(mut descriptor: wgpu::InstanceDescriptor) -> wgpu::Instance {
    let found = wasm_bindgen_futures::JsFuture::from(webgpu_adapter())
        .await
        .is_ok_and(|found| found.is_truthy());
    if !found {
        descriptor.backends.remove(wgpu::Backends::BROWSER_WEBGPU);
    }
    wgpu::Instance::new(descriptor)
}
