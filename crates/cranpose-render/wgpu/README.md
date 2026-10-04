# Cranpose WGPU Renderer

`cranpose-render-wgpu` produces Cranpose scene output with `wgpu`.
Platform-shell authors connect the renderer to a device, queue and surface.
App authors usually select a renderer feature on
[`cranpose`](https://docs.rs/cranpose/latest/cranpose/) and use
`AppLauncher`.

`WgpuRenderer::new` accepts app font data. The host then supplies the WGPU
device, queue and surface format:

```rust
use cranpose_render_wgpu::WgpuRenderer;

static APP_FONTS: &[&[u8]] = &[];
let renderer = WgpuRenderer::new(APP_FONTS);
```

Platform-shell authors call `init_gpu` and the render methods for GPU setup
and frame presentation. The
[desktop shell](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose/src)
and [web shell](https://github.com/samoylenkodmitry/Cranpose/blob/main/crates/cranpose/src/web.rs)
show platform setup and surface presentation.

Default features are empty. `backend-gles` adds the GL-class
backend on native targets. Web builds include the WebGL fallback. The optional
`embedded-default-font` feature supplies the framework fallback font. App
projects select WGPU with the `renderer-wgpu` feature on `cranpose`; the
`renderer-wgpu-gles` feature also enables `backend-gles` for native builds.

See the [`WgpuRenderer` API](https://docs.rs/cranpose-render-wgpu/latest/cranpose_render_wgpu/struct.WgpuRenderer.html),
the [renderer architecture guide](https://github.com/samoylenkodmitry/Cranpose/blob/main/docs/render_arch.md),
the [feature list](https://docs.rs/crate/cranpose/latest/features)
and the [crate source](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose-render/wgpu).
