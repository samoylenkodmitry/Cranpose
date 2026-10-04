# Cranpose web platform

This internal platform crate connects Cranpose to browser APIs on
`wasm32`. The crate binds the renderer to an HTML canvas, requests frames from
the browser, and translates browser input events for the shared runtime.
Applications enable `web` and `renderer-wgpu` on `cranpose`.

Call `AppLauncher::run_web` with the canvas element's ID and root composable:

```rust,ignore
use cranpose::AppLauncher;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub async fn run_app() -> Result<(), JsValue> {
    AppLauncher::new()
        .run_web("app-canvas", || {
            MyApp();
        })
        .await
}
```
