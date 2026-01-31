# Cranpose Platform Desktop (Winit)

Desktop platform integration layer for Cranpose, built on top of `winit`.

## When to Use

This crate provides window management and event handling for Linux, macOS, and Windows. It is the default platform backend for desktop applications.

## Key Concepts

-   **Event Loop**: Manages the `winit` event loop, handling window resizing, mouse/keyboard input, and close requests.
-   **Window Creation**: Configures the initial window state (title, size, decorations).
-   **Redraw Scheduling**: Coordinates with the renderer to redraw frames only when necessary (dirty regions or animations).

## Example

Typically usage is handled by the `AppLauncher`:

```rust
fn main() {
    AppLauncher::new()
        .with_title("Desktop App")
        .with_inner_size(800.0, 600.0)
        .run(MyApp);
}
```
