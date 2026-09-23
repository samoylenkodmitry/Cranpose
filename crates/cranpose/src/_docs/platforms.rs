//! # Platforms
//!
//! One codebase targets desktop, Android including Wear OS, iOS, and the web
//! through WebAssembly, rendering through wgpu on all of them. The target is
//! selected by feature, not by a separate crate:
//!
//! | Target | Features |
//! | --- | --- |
//! | Desktop (Linux, macOS, Windows) | `desktop`, `renderer-wgpu` |
//! | Desktop, one display server | `desktop-x11` or `desktop-wayland` |
//! | Android and Wear OS | `android`, `renderer-wgpu` |
//! | iOS | `ios`, `renderer-wgpu` |
//! | Web | `web`, `renderer-wgpu`, with `default-features = false` |
//!
//! On Linux with both backends compiled, the application runs on the X
//! display named by `DISPLAY` whenever it can reach one, including XWayland
//! inside a Wayland session, because native windows are placed and dragged
//! in global screen coordinates that only X11 exposes. It runs on Wayland
//! when no X display is reachable; `env -u DISPLAY` selects Wayland for a
//! single run.
//!
//! On Windows a release build is a GUI program when its `main.rs` starts with
//! `#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]`;
//! without it Windows opens a terminal window beside the application. The
//! attribute belongs to the binary, so the framework cannot set it. Helper
//! programs the framework runs start without a window of their own, and
//! `cranpose_services::windowless_command` starts an application's own the
//! same way, so none flashes a terminal on screen.
//!
//! `renderer-wgpu-gles` adds a GL/GLES fallback for machines without a working
//! Vulkan driver; Android enables it on its own. `renderer-pixels` is the
//! software renderer.
//!
//! The default feature set embeds a fallback font of about 1.3 MiB. An
//! application that ships its own fonts through `AppLauncher::with_fonts`
//! should build with `default-features = false` to drop it.
//!
//! `CRANPOSE_PRESENT_MODE` selects the swapchain present mode at runtime
//! (`fifo`, `mailbox`, `immediate`, `auto_vsync`, `auto_no_vsync`), which is
//! how the performance harness measures an unthrottled frame rate.
