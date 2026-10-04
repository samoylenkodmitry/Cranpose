//! # Platform hosts
//!
//! Share composable functions across platform hosts. Cargo features select the
//! host, renderer and optional device backends.
//!
//! | Target | Features | Setup |
//! | --- | --- | --- |
//! | Linux, macOS, Windows | `desktop`, `renderer-wgpu` | [`AppLauncher::try_run`](https://docs.rs/cranpose/latest/cranpose/struct.AppLauncher.html#method.try_run) |
//! | Android and Wear OS | `android`, `renderer-wgpu` | [Android plugin](https://github.com/samoylenkodmitry/Cranpose/blob/main/crates/cranpose/android/README.md) |
//! | iOS | `ios`, `renderer-wgpu` | [iOS template](https://github.com/samoylenkodmitry/cranpose-showcase/tree/main/ios) |
//! | Web | `web`, `renderer-wgpu` | [Web template](https://github.com/samoylenkodmitry/cranpose-showcase) |
//! | watchOS, experimental | `watchos` | [watchOS host](https://github.com/samoylenkodmitry/Cranpose/blob/main/crates/cranpose/watchos/README.md) |
//!
//! ## Desktop
//!
//! The `desktop` feature selects both X11 and Wayland on Linux. Use
//! `desktop-x11` or `desktop-wayland` for one backend. When both are present,
//! a reachable X display takes precedence; `env -u DISPLAY` selects Wayland
//! for a single launch. Native window position and drag behavior depend on the
//! display protocol.
//!
//! A Windows app can select the GUI subsystem in `main.rs`:
//!
//! ```rust
//! #![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! # fn main() {}
//! ```
//!
//! This attribute hides the console for a release build. The
//! [`windowless_command`](crate::windowless_command) helper applies the same
//! window policy to child commands.
//!
//! ## Renderer and fonts
//!
//! `renderer-wgpu` uses the GPU. `renderer-wgpu-gles` adds GL/GLES; Android
//! already selects this backend. Web defaults to GL, and `?backend=webgpu`
//! requests WebGPU. `?backend=auto` tries WebGPU with a GL fallback.
//! `renderer-pixels` provides software output for custom hosts and watchOS.
//!
//! The default feature embeds a font. Apps can supply fonts through
//! [`AppLauncher`](crate::AppLauncher) methods and select
//! `default-features = false` to omit the embedded asset.
//!
//! `CRANPOSE_PRESENT_MODE` selects `fifo`, `mailbox`, `immediate`,
//! `auto_vsync` or `auto_no_vsync` at runtime for the GPU host.
//!
//! ## Native integration
//!
//! [`cranpose-native`](https://docs.rs/cranpose-native/latest/cranpose_native/)
//! places Cranpose screens inside Android Compose and UIKit apps. `NativeView`
//! reserves layout space for controls supplied by the host.
//!
//! The `webview` feature adds `WebView`. Desktop needs the platform web runtime;
//! Android and iOS use native browser views, and web uses an iframe. Native
//! browser views own input and accessibility above the Cranpose surface.
//! Browser frame rules and cross-origin policies govern web content.
