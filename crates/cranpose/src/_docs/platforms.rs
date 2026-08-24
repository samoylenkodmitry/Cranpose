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
