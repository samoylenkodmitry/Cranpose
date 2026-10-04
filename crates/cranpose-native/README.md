# Cranpose Native

`cranpose-native` embeds a Cranpose composition in a native host view. Native
application authors create a `NativeSession`, exchange named events, and read
frame updates. The session owns a worker for composition state. Framework
integrators can use `NativeView` to mount platform controls inside Cranpose UI.

## Create a session

The factory creates content on the session's worker. The helper below wraps a
root UI closure in `NativeContent`:

```rust
use cranpose_native::{NativeContent, NativeSession};
use std::sync::Arc;

fn create_session(root: impl FnMut() + Send + 'static) -> Arc<NativeSession> {
    NativeSession::new(move || NativeContent::new(root))
}
```

The host drives `frame`, forwards input with `touch` or `send_event`, and calls
`shutdown` when the view leaves the host. A platform adapter can attach a
`FrameListener` and request work from its UI loop. The crate selects the WGPU
renderer and embedded fallback font through its `cranpose` dependency. Cargo
feature selection lives on `cranpose`.

See the [`NativeSession` API](https://docs.rs/cranpose-native/latest/cranpose_native/struct.NativeSession.html),
the [native demo](https://github.com/samoylenkodmitry/Cranpose/tree/main/apps/native-demo),
the [host integration guide](https://github.com/samoylenkodmitry/Cranpose/blob/main/docs/guide.md#compose-integration)
and the [crate source](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose-native).
