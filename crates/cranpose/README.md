# Cranpose

Cranpose is a declarative UI framework for Rust. It is the primary entry point for building applications using the Cranpose system, re-exporting necessary types and macros from core, UI, and foundation crates.

## When to Use

Use this crate when you are building an end-user application. It provides the `AppLauncher` for bootstrapping the runtime and the `prelude` module which contains the most commonly used widgets (`Column`, `Row`, `Text`) and modifiers.

If you are developing a custom widget library or a low-level extension, you might prefer depending on `cranpose-core` or `cranpose-ui` directly to reduce compile times or dependency footprint.

## Key Concepts

-   **AppLauncher**: The entry point that initializes the platform-specific window (via `winit`, Android Activity, or HTML Canvas) and starts the composition loop.
-   **Prelude**: A convenience module that brings `Composer`, `Modifier`, `Element`, and core widgets into scope.
-   **Feature Flags**: Controls which platform backends (`desktop`, `android`, `web`) and renderers (`wgpu`, `pixels`) are compiled.

## Feature Flags

-   `desktop` (default): Application shell for Linux, macOS, and Windows.
-   `android`: Bindings for Android Activity.
-   `web`: Bindings for WASM/WebGL2.
-   `renderer-wgpu` (default): Hardware-accelerated rendering using `wgpu`.
-   `renderer-pixels`: Software rendering fallback using `pixels`.

## Architecture

Cranpose is composed of several crates:

-   `cranpose-core`: The composition runtime, slot table, and state snapshot system.
-   `cranpose-ui`: UI primitives, layout protocol, and high-level widgets.
-   `cranpose-foundation`: Essential building blocks (Box, Row, Column) and the Modifier system.
-   `cranpose-animation`: Physics-based animation system.

## Example

```rust
use cranpose::prelude::*;

#[composable]
fn CounterApp() {
    let count = useState(|| 0);

    Column(Modifier.fill_max_size().padding(20.dp), || {
        Text(format!("Count: {}", count.value()));
        
        Button(
            onClick = move || count.set(count.value() + 1), 
            || Text("Increment")
        );
    });
}

fn main() {
    AppLauncher::new()
        .with_title("Counter Demo")
        .run(CounterApp);
}
```
