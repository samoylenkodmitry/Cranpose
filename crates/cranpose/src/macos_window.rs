//! The AppKit window behind a winit window on macOS.
#![expect(unsafe_code)]

use objc2::rc::Retained;
use objc2_app_kit::{NSView, NSWindow, NSWindowAnimationBehavior};
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
///
/// Starting that animation held the main thread 14 ms on an Apple M5, with
/// the launch's first frame waiting behind it (median of six launches).
pub(crate) fn show_without_open_animation(window: &dyn Window) {
    let Some(ns_window) = ns_window(window) else {
        window.set_visible(true);
        return;
    };
    let behavior = ns_window.animationBehavior();
    ns_window.setAnimationBehavior(NSWindowAnimationBehavior::None);
    window.set_visible(true);
    ns_window.setAnimationBehavior(behavior);
}
