# Cranpose Render Pixels

A software rendering backend for Cranpose, powered by the `pixels` crate.

## When to Use

Use this renderer when:
-   Hardware acceleration is unavailable or unstable.
-   You are debugging rendering issues and want a simpler reference implementation.
-   You are targeting a platform where `wgpu` is not yet supported.

## Key Concepts

-   **Software Rasterization**: All shapes and text are drawn to a CPU buffer internally.
-   **Pixels**: The library used to blit the CPU buffer to the window surface.

## Usage

To use this backend, disable default features and enable `renderer-pixels` in your `Cargo.toml`:

```toml
[dependencies]
cranpose = { version = "...", default-features = false, features = ["desktop", "renderer-pixels"] }
```
