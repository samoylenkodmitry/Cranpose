# Cranpose Pixels Renderer

`cranpose-render-pixels` draws Cranpose scenes into a caller-owned RGBA byte
buffer. Renderer and host authors use `PixelsRenderer` when a software frame
fits the target surface. App authors can select the `renderer-pixels` feature
on [`cranpose`](https://docs.rs/cranpose/latest/cranpose/).

`PixelsRenderer::draw_scaled` writes the current scene into a host-owned RGBA
buffer at the requested device scale:

```rust
use cranpose_render_pixels::PixelsRenderer;

let renderer = PixelsRenderer::new();
let mut rgba = vec![0; 64 * 64 * 4];
renderer.draw_scaled(&mut rgba, 64, 64, 2.0);
```

`AppShell` supplies the scene before each frame. The host presents the image.
The [watchOS host](https://github.com/samoylenkodmitry/Cranpose/blob/main/crates/cranpose/src/watchos.rs)
shows the complete integration.

Default features are empty. The optional
`embedded-default-font` feature supplies a fallback font. The
[`renderer-pixels` feature](https://docs.rs/cranpose/latest/cranpose/#select-a-platform)
selects this backend through `cranpose`; the `watchos` feature selects the same
backend.

See the [`PixelsRenderer` API](https://docs.rs/cranpose-render-pixels/latest/cranpose_render_pixels/struct.PixelsRenderer.html),
the [renderer architecture guide](https://github.com/samoylenkodmitry/Cranpose/blob/main/docs/render_arch.md)
and the [crate source](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose-render/pixels).
