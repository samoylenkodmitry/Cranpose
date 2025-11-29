# Web Build Technical Details

## Rendering Backend

The RS-Compose web demo uses **WebGL2** as the rendering backend via wgpu's GL backend. This provides excellent compatibility with all modern browsers while maintaining the same rendering code used on desktop platforms.

## Why WebGL Instead of WebGPU?

While WebGPU is the future of web graphics, we use WebGL2 for the following reasons:

1. **Universal Browser Support**: WebGL2 is supported by all modern browsers (Chrome, Firefox, Edge, Safari) without requiring experimental flags.

2. **Avoiding Spec Incompatibilities**: wgpu 0.19 uses newer WebGPU specification field names (e.g., `maxInterStageShaderComponents`) that Chrome stable doesn't recognize yet (it expects `maxInterStageShaderVariables`). This would require users to install Chrome Canary/Dev.

3. **Same Codebase**: wgpu provides a unified API that works identically on both WebGL and WebGPU backends, so the same Rust rendering code works everywhere.

4. **Good Performance**: WebGL2 is hardware-accelerated and provides good performance for UI rendering.

## Browser Requirements

- **Chrome/Edge**: Version 56+ (released 2017)
- **Firefox**: Version 51+ (released 2017)
- **Safari**: Version 15+ (released 2021)

Essentially any browser from the last few years works out of the box.

## Building for Web

```bash
cd apps/desktop-demo
./build-web.sh

# Start a local server
python3 -m http.server 8080

# Open in any modern browser
# http://localhost:8080
```

## Technical Implementation

In `crates/compose-app/src/web.rs`, we initialize wgpu with the GL backend:

```rust
let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
    backends: wgpu::Backends::GL,  // Use WebGL backend
    ..Default::default()
});
```

The `webgl` feature is enabled in `crates/compose-render/wgpu/Cargo.toml`:

```toml
wgpu = { version = "0.19", features = ["webgl"] }
```

This tells wgpu to use the `glow` library (OpenGL/WebGL wrapper) instead of the browser's `navigator.gpu` WebGPU API.

## Future: Switching to WebGPU

When WebGPU support stabilizes across browsers, we can:

1. **Runtime Detection**: Try WebGPU first, fall back to WebGL if it fails
2. **Build-time Flag**: Allow users to choose which backend at build time
3. **Automatic Selection**: Use WebGPU when available, WebGL otherwise

The benefit of using wgpu is that switching between backends requires minimal code changes - just changing the `backends` parameter.
