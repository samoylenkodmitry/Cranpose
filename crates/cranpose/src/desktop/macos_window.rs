//! The AppKit window behind a winit window on macOS.
#![expect(unsafe_code, reason = "Objective-C messages to AppKit")]

use objc2::rc::Retained;
use objc2_app_kit::{NSView, NSWindow, NSWindowAnimationBehavior};
use objc2_quartz_core::CATransaction;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

pub(crate) fn ns_window(window: &dyn Window) -> Option<Retained<NSWindow>> {
    let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
        return None;
    };
    // SAFETY: winit hands out the content view of a window that is still open,
    // and the event loop this is called from runs on the main thread AppKit
    // views belong to.
    let view: &NSView = unsafe { handle.ns_view.cast().as_ref() };
    view.window()
}

/// Puts `window` on the screen fully transparent and out of the mouse's
/// reach, without the open animation, until [`reveal`] makes it opaque.
///
/// wgpu hands out no Metal drawable while the window server reports a window
/// not visible (gfx-rs/wgpu#8309), and it reports a shown window visible 75
/// to 85 ms after it was shown on an Apple M5. A window shown once its
/// content is composed held its first frame back that long. Staged as it is
/// created, the window is reported visible while the app composes, and it
/// becomes opaque with its first frame: the gauntlet's first frame came
/// 168-172 ms after exec instead of 179-199 (five launches each, shaders
/// cached), 169-220 instead of 188-232 with none cached.
pub(crate) fn stage(window: &dyn Window) {
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    ns_window.setAlphaValue(0.0);
    let _ = window.set_cursor_hittest(false);
    set_visible_without_open_animation(&ns_window, window);
}

/// Makes a window [`stage`] put on the screen opaque and reachable by the
/// mouse.
pub(crate) fn reveal(window: &dyn Window) {
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    ns_window.setAlphaValue(1.0);
    let _ = window.set_cursor_hittest(true);
}

/// Shows `window` without the animation AppKit plays as a window opens, then
/// hands the window its own animation back for the times it is shown later.
/// A window whose first frame `builds_pipelines` goes to the window server at
/// once, before that frame.
///
/// Starting that animation held the main thread 14 ms on an Apple M5, with
/// the launch's first frame waiting behind it (median of six launches).
pub(crate) fn show_without_open_animation(window: &dyn Window, builds_pipelines: bool) {
    let Some(ns_window) = ns_window(window) else {
        window.set_visible(true);
        return;
    };
    set_visible_without_open_animation(&ns_window, window);
    // AppKit sends a shown window to the window server when the run loop
    // pass ends, and draws its view inside that commit: a first frame that
    // compiles its shaders kept the window off the screen for as long. On an
    // Apple M5 with no shader cached, the gauntlet's window came up at
    // ~200 ms after exec with the flush and ~450-600 without. A first frame
    // drawn from built pipelines comes ~10 ms later, and the window comes up
    // with it, not with one frame of the clear color before it.
    if builds_pipelines {
        CATransaction::flush();
    }
}

fn set_visible_without_open_animation(ns_window: &NSWindow, window: &dyn Window) {
    let behavior = ns_window.animationBehavior();
    ns_window.setAnimationBehavior(NSWindowAnimationBehavior::None);
    window.set_visible(true);
    ns_window.setAnimationBehavior(behavior);
}
