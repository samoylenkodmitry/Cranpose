# Cranpose Desktop Winit Adapter

`cranpose-platform-desktop-winit` converts winit pointer and scroll input to
Cranpose logical coordinates. Framework and desktop-shell authors use
`DesktopWinitPlatform` at the event-loop boundary. App authors select the
`desktop`, `desktop-x11` or `desktop-wayland` feature on
[`cranpose`](https://docs.rs/cranpose/latest/cranpose/).

```rust
use cranpose_platform_desktop_winit::DesktopWinitPlatform;
use winit::dpi::PhysicalPosition;

let platform = DesktopWinitPlatform::new(2.0);
let position = platform.pointer_position(PhysicalPosition::new(640.0, 480.0));
assert_eq!((position.x, position.y), (320.0, 240.0));
```

The adapter provides `pointer_event` and `scroll_delta` alongside coordinate
conversion. Cargo enables X11 and Wayland by default; app authors can choose
`desktop-x11` or `desktop-wayland` on `cranpose`. See the
[`DesktopWinitPlatform` API](https://docs.rs/cranpose-platform-desktop-winit/latest/cranpose_platform_desktop_winit/struct.DesktopWinitPlatform.html),
the [desktop shell](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose/src)
and the [crate source](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose-platform/desktop-winit).
