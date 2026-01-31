# Cranpose Platform Web

Web platform integration layer for Cranpose, targeting WebAssembly (WASM).

## When to Use

This crate enables Cranpose applications to run in a web browser. It binds to the DOM to manage the canvas element and uses requestAnimationFrame for the render loop. It is used implicitly when the `web` feature is enabled.

## Key Concepts

-   **Canvas Binding**: Attaches the renderer to an HTML `<canvas>` element specified by ID.
-   **Wasm Bindgen**: Uses `wasm-bindgen` and `web-sys` to interact with JavaScript APIs.
-   **Event Bridging**: Listens for DOM events (mousedown, touchstart, keydown) and dispatches them to the Cranpose event system.

## Example

```rust
use cranpose::prelude::*;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub async fn run_app() -> Result<(), JsValue> {
    // "canvas-id" must match the id of a canvas element in your index.html
    AppLauncher::new()
        .run_web("canvas-id", MyApp)
        .await
}
```
