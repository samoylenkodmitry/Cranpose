//! The AppKit window behind a winit window on macOS.
#![expect(unsafe_code)]

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
    let behavior = ns_window.animationBehavior();
    ns_window.setAnimationBehavior(NSWindowAnimationBehavior::None);
    window.set_visible(true);
    ns_window.setAnimationBehavior(behavior);
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
