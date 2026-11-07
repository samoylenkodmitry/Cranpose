//! The exact GLSL of the WebGL stages the page's first screen compiles,
//! which a build with first-screen records fetches from beside its wasm
//! (`first_screen_glsl.txt`, see `cranpose_render_wgpu::use_first_screen_glsl`).
//!
//! Only a page that will draw on WebGL fetches the file: on Chrome on an M3
//! Pro a WebGPU page that fetched it drew its first frame 24 ms later on a
//! second start (median of 8). A page asked for WebGL in its query, or one
//! whose browser has no `navigator.gpu`, starts the fetch as the browser
//! loads the wasm-bindgen glue, which imports this snippet before it
//! fetches the wasm. Where the browser has `navigator.gpu` only an adapter
//! request tells, and one made as the glue loaded put Chrome's first frame
//! on a Mate 20 X (Android 10, `navigator.gpu` but no adapter) 29 ms later
//! (median of 8 starts). The snippet asks once the page's first frame is
//! out, two animation frames in, and fetches the file where no adapter
//! came. The page's own detection takes that answer, or asks then where no
//! frame came yet, so a page asks for one adapter as before
//! ([`auto_instance`]). The renderer never waits for the file. The snippet
//! sits in `snippets/<crate>/`, two folders below the wasm's.

use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen(inline_js = r#"
let file;
let fetching = false;
function fetchFile() {
    if (fetching) return;
    fetching = true;
    fetch(new URL("../../first_screen_glsl.txt", import.meta.url))
        .then((response) => (response.ok ? response.arrayBuffer() : null))
        .catch(() => null)
        .then((fetched) => { file = fetched; });
}
let adapter;
export function firstScreenWebGpu() {
    if (!adapter) {
        adapter = navigator.gpu
            ? navigator.gpu.requestAdapter().then((found) => !!found, () => false)
            : Promise.resolve(false);
        adapter.then((found) => { if (!found) fetchFile(); });
    }
    return adapter;
}
const backend = new URLSearchParams(location.search).get("backend");
if (backend === "gl" || (backend !== "webgpu" && !navigator.gpu)) fetchFile();
else if (backend !== "webgpu") requestAnimationFrame(() => requestAnimationFrame(firstScreenWebGpu));
export function firstScreenGlsl() {
    return file;
}
"#)]
extern "C" {
    // Whether the browser gave the page a WebGPU adapter, asked once.
    #[wasm_bindgen(js_name = firstScreenWebGpu)]
    fn webgpu_adapter() -> js_sys::Promise;

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
// `wgpu::util::new_instance_with_webgpu_detection` makes it, from the
// snippet's one adapter request.
pub(super) async fn auto_instance(mut descriptor: wgpu::InstanceDescriptor) -> wgpu::Instance {
    let found = wasm_bindgen_futures::JsFuture::from(webgpu_adapter())
        .await
        .is_ok_and(|found| found.is_truthy());
    if !found {
        descriptor.backends.remove(wgpu::Backends::BROWSER_WEBGPU);
    }
    wgpu::Instance::new(descriptor)
}
