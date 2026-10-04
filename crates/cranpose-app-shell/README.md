# Cranpose App Shell

`cranpose-app-shell` coordinates Cranpose composition, frame cadence, input dispatch, focus, layout, and redraw. Platform adapters route window events and display callbacks into UI work. Most app authors use [`cranpose::AppLauncher`](https://docs.rs/cranpose/latest/cranpose/struct.AppLauncher.html); platform, renderer, and test authors use this crate.

## Frame-rate policy

Platform frame drivers can use `FrameRatePreference` to choose a display vote. `Auto` uses a quiet rate for ordinary frames and the panel maximum while a gesture boost remains active. `Exact` requests a fixed rate.

```rust
use cranpose_app_shell::FrameRatePreference;

let requested_hz = FrameRatePreference::Exact(90.0)
    .desired_rate_hz(true, false, Some(120.0));
assert_eq!(requested_hz, 90.0);
```

The shell exposes `FrameScheduler`, `FrameSchedule`, `PlatformFrameDriver`, `RootSurface`, and input event types for host implementations. `clipboard-native` enables the desktop clipboard adapter. `test-support` exposes placed semantics and accessibility audit modules used by `cranpose-testing`.

## Links

- [API documentation](https://docs.rs/cranpose-app-shell/latest/cranpose_app_shell/)
- [Test APIs](https://docs.rs/cranpose-testing/latest/cranpose_testing/)
- [Source](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose-app-shell)
- [Mobile frame architecture](https://github.com/samoylenkodmitry/Cranpose/blob/main/docs/mobile_60fps_architecture.md)
