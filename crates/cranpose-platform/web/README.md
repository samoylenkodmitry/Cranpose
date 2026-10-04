# Cranpose Web Platform Adapter

`cranpose-platform-web` converts browser pointer coordinates and event kinds
to Cranpose input values. Framework and browser-shell authors use
`WebPlatform` at the DOM event boundary. App authors select `web` and a
renderer feature on [`cranpose`](https://docs.rs/cranpose/latest/cranpose/).

```rust
use cranpose_foundation::PointerEventKind;
use cranpose_platform_web::WebPlatform;

let platform = WebPlatform::new(1.0);
let event = platform.pointer_event(PointerEventKind::Move, 24.0, 32.0);
assert_eq!((event.position.x, event.position.y), (24.0, 32.0));
```

The browser shell owns DOM registration, canvas state and frame presentation.
The [web shell](https://github.com/samoylenkodmitry/Cranpose/blob/main/crates/cranpose/src/web.rs)
shows the browser event path and WGPU backend selection. See the
[`WebPlatform` API](https://docs.rs/cranpose-platform-web/latest/cranpose_platform_web/struct.WebPlatform.html),
and the [crate source](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose-platform/web).
