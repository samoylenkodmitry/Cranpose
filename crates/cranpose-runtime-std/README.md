# Cranpose Runtime Std

`cranpose-runtime-std` implements Cranpose's clock and scheduler contracts
with Rust standard-library services. App-shell and custom host authors use
`StdRuntime` to supply a `cranpose_core::Runtime`. App developers use
[`cranpose::AppLauncher`](https://docs.rs/cranpose/latest/cranpose/struct.AppLauncher.html),
which wires the runtime into a platform shell.

## Drive runtime callbacks

A host polls for scheduled frames and drains callbacks with the platform
frame timestamp:

```rust
use cranpose_runtime_std::StdRuntime;

fn on_frame(runtime: &StdRuntime, frame_time_nanos: u64) {
    if runtime.take_frame_request() {
        runtime.drain_frame_callbacks(frame_time_nanos);
    }
}
```

Native hosts can connect `set_frame_waker` to their event loop. The
`internal` feature exposes `StdRuntime::frame_clock` for framework integration.
Default features are empty. See the
[`StdRuntime` API](https://docs.rs/cranpose-runtime-std/latest/cranpose_runtime_std/struct.StdRuntime.html),
the [app-shell source](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose-app-shell)
and the [crate source](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose-runtime-std).
